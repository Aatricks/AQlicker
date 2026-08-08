//! Which application currently has keyboard focus.
//!
//! # Why this API and not `NSWorkspace`
//!
//! The frontmost lookup runs on the input worker thread once per press tick, so
//! it must never block on or marshal to the macOS main thread — the main thread
//! belongs to the AppKit event loop and the service dispatcher already depends
//! on it. `NSWorkspace.frontmostApplication` is AppKit and main-thread affine,
//! so it is not usable here.
//!
//! Two thread-safe C-level alternatives were measured on a background thread
//! before this module was written (see the report in the pull request):
//!
//! * `AXUIElementCopyAttributeValue(AXUIElementCreateSystemWide(),
//!   kAXFocusedApplicationAttribute)` returned `kAXErrorCannotComplete`
//!   (`-25204`) on every call even with Accessibility granted, and it is a
//!   synchronous message to the target application, so an unresponsive target
//!   could stall the input worker for the messaging timeout. Rejected.
//! * `CGWindowListCopyWindowInfo` worked from a background thread, returned real
//!   `kCGWindowOwnerName`/`kCGWindowOwnerPID` values with no Screen Recording
//!   prompt (only `kCGWindowName`, which this module never reads, is gated on
//!   that permission), tracked application switches correctly, and cost
//!   0.5-2 ms per call. Chosen.
//!
//! The window list is returned front-to-back, so the frontmost application owns
//! the first normal-layer (`kCGWindowLayer == 0`) window. Stage Manager keeps a
//! permanent normal-layer window in front of everything, so `com.apple.
//! WindowManager` is skipped. Known limitation: an application that is frontmost
//! with every window minimised owns no on-screen window, so the lookup reports
//! whichever application is visible behind it and the run stays paused.

use serde::Serialize;

/// A user-visible application the run can be restricted to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    pub id: String,
    pub name: String,
}

pub trait FocusProbe: Send {
    /// Stable identifier of the application that currently has keyboard focus,
    /// or `None` when it cannot be determined.
    fn frontmost(&mut self) -> Option<String>;

    /// Currently running, user-visible applications.
    fn running_apps(&mut self) -> Vec<RunningApp>;
}

/// The probe every `RunController` gets unless the system one is installed.
/// Keeps unrestricted runs and the whole test suite off the operating system.
pub struct UnknownFocus;

impl FocusProbe for UnknownFocus {
    fn frontmost(&mut self) -> Option<String> {
        None
    }

    fn running_apps(&mut self) -> Vec<RunningApp> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
pub fn system_focus_probe() -> Box<dyn FocusProbe> {
    Box::new(macos::MacFocusProbe::default())
}

#[cfg(target_os = "windows")]
pub fn system_focus_probe() -> Box<dyn FocusProbe> {
    Box::new(windows::WindowsFocusProbe)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn system_focus_probe() -> Box<dyn FocusProbe> {
    Box::new(UnknownFocus)
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{collections::HashMap, ffi::c_void};

    use objc2_core_foundation::{
        CFArray, CFBundle, CFDictionary, CFNumber, CFNumberType, CFString, CFURL,
    };
    use objc2_core_graphics::{
        CGWindowListCopyWindowInfo, CGWindowListOption, kCGWindowLayer, kCGWindowOwnerName,
        kCGWindowOwnerPID,
    };

    use super::{FocusProbe, RunningApp};

    /// Stage Manager owns a permanent normal-layer window in front of every
    /// application window, so it can never be the frontmost application.
    const WINDOW_MANAGER_BUNDLE_ID: &str = "com.apple.WindowManager";

    unsafe extern "C" {
        fn proc_pidpath(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
    }

    #[derive(Default)]
    pub struct MacFocusProbe {
        /// Resolving a bundle identifier touches the file system, so the result
        /// is cached per process for the lifetime of the probe.
        identifiers: HashMap<i32, Option<String>>,
    }

    impl MacFocusProbe {
        /// Visits the on-screen normal-layer windows, front to back, skipping
        /// system window management. Returns `(identifier, owner name)`.
        fn visible_windows(&mut self) -> Vec<(String, String)> {
            let Some(list) = CGWindowListCopyWindowInfo(
                CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements,
                0,
            ) else {
                return Vec::new();
            };
            let mut found = Vec::new();
            for index in 0..list.count() {
                let Some(entry) = window_at(&list, index) else {
                    continue;
                };
                if number(entry, unsafe { kCGWindowLayer }) != Some(0) {
                    continue;
                }
                let (Some(pid), Some(name)) = (
                    number(entry, unsafe { kCGWindowOwnerPID }),
                    text(entry, unsafe { kCGWindowOwnerName }),
                ) else {
                    continue;
                };
                let Some(identifier) = self.identifier(pid as i32) else {
                    continue;
                };
                if identifier == WINDOW_MANAGER_BUNDLE_ID {
                    continue;
                }
                found.push((identifier, name));
            }
            found
        }

        fn identifier(&mut self, pid: i32) -> Option<String> {
            self.identifiers
                .entry(pid)
                .or_insert_with(|| bundle_identifier(pid))
                .clone()
        }
    }

    impl FocusProbe for MacFocusProbe {
        fn frontmost(&mut self) -> Option<String> {
            self.visible_windows()
                .into_iter()
                .next()
                .map(|(identifier, _)| identifier)
        }

        fn running_apps(&mut self) -> Vec<RunningApp> {
            let mut apps: Vec<RunningApp> = Vec::new();
            for (id, name) in self.visible_windows() {
                if !apps.iter().any(|app| app.id == id) {
                    apps.push(RunningApp { id, name });
                }
            }
            apps.sort_by_key(|app| app.name.to_lowercase());
            apps
        }
    }

    /// Falls back to the executable path when a process has no bundle, so an
    /// unbundled helper still gets a stable identifier rather than none.
    fn bundle_identifier(pid: i32) -> Option<String> {
        let executable = executable_path(pid)?;
        let Some((bundle_root, _)) = executable.split_once(".app/Contents/MacOS/") else {
            return Some(executable);
        };
        let path = format!("{bundle_root}.app");
        let url = unsafe {
            CFURL::from_file_system_representation(None, path.as_ptr(), path.len() as isize, true)
        }?;
        CFBundle::new(None, Some(&url))
            .and_then(|bundle| bundle.identifier())
            .map(|identifier| identifier.to_string())
            .or(Some(executable))
    }

    fn executable_path(pid: i32) -> Option<String> {
        const MAX_PATH_LENGTH: usize = 4096;
        let mut buffer = vec![0_u8; MAX_PATH_LENGTH];
        let written =
            unsafe { proc_pidpath(pid, buffer.as_mut_ptr().cast(), MAX_PATH_LENGTH as u32) };
        if written <= 0 {
            return None;
        }
        buffer.truncate(written as usize);
        String::from_utf8(buffer).ok()
    }

    fn window_at(list: &CFArray, index: isize) -> Option<&CFDictionary> {
        let entry = unsafe { list.value_at_index(index) };
        (!entry.is_null()).then(|| unsafe { &*entry.cast::<CFDictionary>() })
    }

    fn value(window: &CFDictionary, key: &CFString) -> Option<*const c_void> {
        let value = unsafe { window.value((key as *const CFString).cast::<c_void>()) };
        (!value.is_null()).then_some(value)
    }

    fn number(window: &CFDictionary, key: &CFString) -> Option<i64> {
        let value = value(window, key)?;
        let mut out: i64 = 0;
        let read = unsafe {
            (*value.cast::<CFNumber>())
                .value(CFNumberType::SInt64Type, (&raw mut out).cast::<c_void>())
        };
        read.then_some(out)
    }

    fn text(window: &CFDictionary, key: &CFString) -> Option<String> {
        let value = value(window, key)?;
        Some(unsafe { (*value.cast::<CFString>()).to_string() })
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::ffi::c_void;

    use super::{FocusProbe, RunningApp};

    type Handle = *mut c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetForegroundWindow() -> Handle;
        fn GetWindowThreadProcessId(window: Handle, process_id: *mut u32) -> u32;
        fn EnumWindows(
            callback: unsafe extern "system" fn(Handle, isize) -> i32,
            parameter: isize,
        ) -> i32;
        fn IsWindowVisible(window: Handle) -> i32;
        fn GetWindowTextW(window: Handle, text: *mut u16, count: i32) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> Handle;
        fn CloseHandle(object: Handle) -> i32;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const MAX_PATH_LENGTH: u32 = 4096;

    pub struct WindowsFocusProbe;

    impl FocusProbe for WindowsFocusProbe {
        fn frontmost(&mut self) -> Option<String> {
            let window = unsafe { GetForegroundWindow() };
            if window.is_null() {
                return None;
            }
            executable_name(process_id(window)?)
        }

        fn running_apps(&mut self) -> Vec<RunningApp> {
            let mut collected: Vec<(Handle, String)> = Vec::new();
            unsafe {
                EnumWindows(collect, (&raw mut collected) as isize);
            }
            let mut apps: Vec<RunningApp> = Vec::new();
            for (window, name) in collected {
                let Some(id) = process_id(window).and_then(executable_name) else {
                    continue;
                };
                if !apps.iter().any(|app| app.id == id) {
                    apps.push(RunningApp { id, name });
                }
            }
            apps.sort_by_key(|app| app.name.to_lowercase());
            apps
        }
    }

    unsafe extern "system" fn collect(window: Handle, parameter: isize) -> i32 {
        if unsafe { IsWindowVisible(window) } != 0 {
            let mut buffer = [0_u16; 512];
            let written =
                unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            if written > 0 {
                let title = String::from_utf16_lossy(&buffer[..written as usize]);
                let collected = unsafe { &mut *(parameter as *mut Vec<(Handle, String)>) };
                collected.push((window, title));
            }
        }
        1
    }

    fn process_id(window: Handle) -> Option<u32> {
        let mut process_id: u32 = 0;
        unsafe { GetWindowThreadProcessId(window, &raw mut process_id) };
        (process_id != 0).then_some(process_id)
    }

    /// The executable file name is the stable identifier on Windows, which has
    /// no bundle identifiers.
    fn executable_name(process_id: u32) -> Option<String> {
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if process.is_null() {
            return None;
        }
        let mut buffer = vec![0_u16; MAX_PATH_LENGTH as usize];
        let mut size = MAX_PATH_LENGTH;
        let read =
            unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &raw mut size) };
        unsafe { CloseHandle(process) };
        if read == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_probe_reports_nothing_without_touching_the_platform() {
        let mut probe = UnknownFocus;
        assert_eq!(probe.frontmost(), None);
        assert_eq!(probe.running_apps(), Vec::new());
    }

    /// Guards the constraint that made this module exist: the lookup has to work
    /// off the main thread. It runs the real platform probe, so it only asserts
    /// what holds on an unattended machine — that it returns without hanging.
    #[test]
    fn the_system_probe_answers_from_a_background_thread() {
        let answered = std::thread::spawn(|| {
            let mut probe = system_focus_probe();
            let apps = probe.running_apps();
            let frontmost = probe.frontmost();
            frontmost.is_none_or(|id| apps.is_empty() || apps.iter().any(|app| app.id == id))
        })
        .join()
        .unwrap();
        assert!(answered);
    }
}
