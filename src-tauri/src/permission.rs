use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionStatus {
    pub granted: bool,
    pub same_integrity_only: bool,
}

pub trait PermissionProbe: Send {
    fn check(&mut self, prompt: bool) -> bool;
}

pub trait PermissionProvider: Send {
    fn status(&mut self) -> PermissionStatus;
    fn request_access(&mut self) -> PermissionStatus;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionPlatform {
    MacOs,
    Windows,
    Other,
}

pub struct PermissionManager<P: PermissionProbe> {
    probe: P,
    platform: PermissionPlatform,
}

impl<P: PermissionProbe> PermissionManager<P> {
    pub const fn new(probe: P, platform: PermissionPlatform) -> Self {
        Self { probe, platform }
    }

    pub fn status(&mut self) -> PermissionStatus {
        self.check(false)
    }

    pub fn request_access(&mut self) -> PermissionStatus {
        self.check(true)
    }

    fn check(&mut self, request: bool) -> PermissionStatus {
        match self.platform {
            PermissionPlatform::MacOs => PermissionStatus {
                granted: self.probe.check(request),
                same_integrity_only: false,
            },
            PermissionPlatform::Windows => PermissionStatus {
                granted: true,
                same_integrity_only: true,
            },
            PermissionPlatform::Other => PermissionStatus {
                granted: self.probe.check(false),
                same_integrity_only: false,
            },
        }
    }
}

impl<P: PermissionProbe> PermissionProvider for PermissionManager<P> {
    fn status(&mut self) -> PermissionStatus {
        PermissionManager::status(self)
    }

    fn request_access(&mut self) -> PermissionStatus {
        PermissionManager::request_access(self)
    }
}

pub struct EnigoPermissionProbe;

impl PermissionProbe for EnigoPermissionProbe {
    fn check(&mut self, prompt: bool) -> bool {
        enigo::Enigo::new(&enigo_settings(prompt)).is_ok()
    }
}

pub fn system_permission_provider() -> Box<dyn PermissionProvider> {
    Box::new(PermissionManager::new(
        EnigoPermissionProbe,
        current_platform(),
    ))
}

fn enigo_settings(prompt: bool) -> enigo::Settings {
    enigo::Settings {
        release_keys_when_dropped: true,
        open_prompt_to_get_permissions: prompt,
        independent_of_keyboard_state: true,
        ..enigo::Settings::default()
    }
}

#[cfg(target_os = "macos")]
const fn current_platform() -> PermissionPlatform {
    PermissionPlatform::MacOs
}

#[cfg(target_os = "windows")]
const fn current_platform() -> PermissionPlatform {
    PermissionPlatform::Windows
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const fn current_platform() -> PermissionPlatform {
    PermissionPlatform::Other
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct RecordingProbe {
        prompts: Arc<Mutex<Vec<bool>>>,
        granted: bool,
    }

    impl PermissionProbe for RecordingProbe {
        fn check(&mut self, prompt: bool) -> bool {
            self.prompts.lock().unwrap().push(prompt);
            self.granted
        }
    }

    #[test]
    fn macos_checks_passively_until_access_is_requested() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let mut permission = PermissionManager::new(
            RecordingProbe {
                prompts: Arc::clone(&prompts),
                granted: false,
            },
            PermissionPlatform::MacOs,
        );

        assert!(!permission.status().granted);
        assert!(!permission.request_access().granted);
        assert_eq!(*prompts.lock().unwrap(), vec![false, true]);
    }

    #[test]
    fn windows_reports_normal_access_with_the_same_integrity_limit() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let mut permission = PermissionManager::new(
            RecordingProbe {
                prompts: Arc::clone(&prompts),
                granted: false,
            },
            PermissionPlatform::Windows,
        );

        assert_eq!(
            permission.status(),
            PermissionStatus {
                granted: true,
                same_integrity_only: true,
            }
        );
        assert_eq!(permission.request_access(), permission.status());
        assert!(prompts.lock().unwrap().is_empty());
    }
}
