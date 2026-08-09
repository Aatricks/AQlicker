use std::{fmt, str::FromStr, sync::Arc};

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const ACTIVE_RUN_SHORTCUT: &str = "Escape";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    ToggleRun,
    StopRun,
    /// Switches to the next preset, wrapping around. App-level, exactly like
    /// the toggle: it is not stored inside a preset.
    CyclePreset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutError {
    pub code: &'static str,
}

impl ShortcutError {
    pub const fn new(code: &'static str) -> Self {
        Self { code }
    }
}

impl fmt::Display for ShortcutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for ShortcutError {}

pub trait ShortcutRegistry: Send {
    fn register(&mut self, shortcut: &str, action: ShortcutAction) -> Result<(), ShortcutError>;
    fn unregister(&mut self, shortcut: &str) -> Result<(), ShortcutError>;
    fn unregister_all(&mut self) -> Result<(), ShortcutError>;
    fn is_registered(&self, shortcut: &str) -> bool;
}

pub trait ShortcutController: Send {
    fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError>;
    fn replace_cycle(&mut self, shortcut: Option<&str>) -> Result<(), ShortcutError>;
    fn active(&self) -> Option<&str>;
    fn cycle(&self) -> Option<&str>;
    fn toggle_registered(&self) -> bool;
    fn cycle_registered(&self) -> bool;
    fn unregister_toggle(&mut self) -> Result<(), ShortcutError>;
    fn register_escape(&mut self) -> Result<(), ShortcutError>;
    fn unregister_escape(&mut self) -> Result<(), ShortcutError>;
    fn escape_registered(&self) -> bool;
    fn unregister_all(&mut self) -> Result<(), ShortcutError>;
}

pub struct ShortcutManager<R: ShortcutRegistry> {
    registry: R,
    /// The two app-level accelerators, in `TOGGLE`/`CYCLE` order. They share one
    /// registration and rollback path so a conflict behaves the same for both.
    slots: [Option<String>; 2],
    escape_registered: bool,
}

const TOGGLE: usize = 0;
const CYCLE: usize = 1;

impl<R: ShortcutRegistry> ShortcutManager<R> {
    pub fn new(mut registry: R, shortcut: &str) -> Result<Self, ShortcutError> {
        ensure_not_reserved(shortcut)?;
        registry.register(shortcut, ShortcutAction::ToggleRun)?;
        Ok(Self {
            registry,
            slots: [Some(shortcut.to_owned()), None],
            escape_registered: false,
        })
    }

    pub fn without_toggle(registry: R) -> Self {
        Self {
            registry,
            slots: [None, None],
            escape_registered: false,
        }
    }

    /// Registers `shortcut` in `slot`, releasing whatever was there. A failure
    /// puts the previous accelerator back and reports the error, so the caller
    /// keeps the shortcut it already had.
    fn assign(
        &mut self,
        slot: usize,
        action: ShortcutAction,
        shortcut: Option<&str>,
    ) -> Result<(), ShortcutError> {
        if let Some(shortcut) = shortcut {
            ensure_not_reserved(shortcut)?;
            // One accelerator cannot mean two things.
            if self.slots[1 - slot].as_deref() == Some(shortcut) {
                return Err(ShortcutError::new("shortcut-conflict"));
            }
        }
        if self.slots[slot].as_deref() == shortcut
            && shortcut.is_none_or(|shortcut| self.registry.is_registered(shortcut))
        {
            return Ok(());
        }

        let previous = self.slots[slot].take();
        if let Some(previous) = previous.as_deref() {
            if let Err(error) = self.registry.unregister(previous) {
                self.slots[slot] = Some(previous.to_owned());
                return Err(error);
            }
        }

        let Some(shortcut) = shortcut else {
            return Ok(());
        };
        match self.registry.register(shortcut, action) {
            Ok(()) => {
                self.slots[slot] = Some(shortcut.to_owned());
                Ok(())
            }
            Err(error) => {
                if let Some(previous) = previous {
                    if self.registry.register(&previous, action).is_err() {
                        return Err(ShortcutError::new("shortcut-rollback-failed"));
                    }
                    self.slots[slot] = Some(previous);
                }
                Err(error)
            }
        }
    }

    pub fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError> {
        self.assign(TOGGLE, ShortcutAction::ToggleRun, Some(shortcut))?;
        Ok(shortcut.to_owned())
    }

    /// `None` clears the preset-cycling shortcut, which is where it starts.
    pub fn replace_cycle(&mut self, shortcut: Option<&str>) -> Result<(), ShortcutError> {
        self.assign(CYCLE, ShortcutAction::CyclePreset, shortcut)
    }

    pub fn register_escape(&mut self) -> Result<(), ShortcutError> {
        if self.escape_registered && self.registry.is_registered(ACTIVE_RUN_SHORTCUT) {
            return Ok(());
        }
        self.registry
            .register(ACTIVE_RUN_SHORTCUT, ShortcutAction::StopRun)?;
        self.escape_registered = true;
        Ok(())
    }

    pub fn unregister_toggle(&mut self) -> Result<(), ShortcutError> {
        self.assign(TOGGLE, ShortcutAction::ToggleRun, None)
    }

    pub fn unregister_escape(&mut self) -> Result<(), ShortcutError> {
        if self.escape_registered {
            self.registry.unregister(ACTIVE_RUN_SHORTCUT)?;
            self.escape_registered = false;
        }
        Ok(())
    }

    pub fn unregister_all(&mut self) -> Result<(), ShortcutError> {
        self.registry.unregister_all()?;
        self.slots = [None, None];
        self.escape_registered = false;
        Ok(())
    }

    pub fn active(&self) -> Option<&str> {
        self.slots[TOGGLE].as_deref()
    }

    pub fn cycle(&self) -> Option<&str> {
        self.slots[CYCLE].as_deref()
    }

    pub fn toggle_registered(&self) -> bool {
        self.registered(TOGGLE)
    }

    pub fn cycle_registered(&self) -> bool {
        self.registered(CYCLE)
    }

    fn registered(&self, slot: usize) -> bool {
        self.slots[slot]
            .as_deref()
            .is_some_and(|shortcut| self.registry.is_registered(shortcut))
    }

    pub fn escape_registered(&self) -> bool {
        self.escape_registered && self.registry.is_registered(ACTIVE_RUN_SHORTCUT)
    }

    pub fn registry(&self) -> &R {
        &self.registry
    }
}

impl<R: ShortcutRegistry> ShortcutController for ShortcutManager<R> {
    fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError> {
        ShortcutManager::replace(self, shortcut)
    }

    fn replace_cycle(&mut self, shortcut: Option<&str>) -> Result<(), ShortcutError> {
        ShortcutManager::replace_cycle(self, shortcut)
    }

    fn active(&self) -> Option<&str> {
        ShortcutManager::active(self)
    }

    fn cycle(&self) -> Option<&str> {
        ShortcutManager::cycle(self)
    }

    fn toggle_registered(&self) -> bool {
        ShortcutManager::toggle_registered(self)
    }

    fn cycle_registered(&self) -> bool {
        ShortcutManager::cycle_registered(self)
    }

    fn unregister_toggle(&mut self) -> Result<(), ShortcutError> {
        ShortcutManager::unregister_toggle(self)
    }

    fn register_escape(&mut self) -> Result<(), ShortcutError> {
        ShortcutManager::register_escape(self)
    }

    fn unregister_escape(&mut self) -> Result<(), ShortcutError> {
        ShortcutManager::unregister_escape(self)
    }

    fn escape_registered(&self) -> bool {
        ShortcutManager::escape_registered(self)
    }

    fn unregister_all(&mut self) -> Result<(), ShortcutError> {
        ShortcutManager::unregister_all(self)
    }
}

fn ensure_not_reserved(shortcut: &str) -> Result<(), ShortcutError> {
    if shortcut.trim().eq_ignore_ascii_case(ACTIVE_RUN_SHORTCUT) {
        Err(ShortcutError::new("shortcut-reserved"))
    } else {
        Ok(())
    }
}

pub type ShortcutHandler = Arc<dyn Fn(ShortcutAction) + Send + Sync>;

pub struct TauriShortcutRegistry {
    app: tauri::AppHandle,
    handler: ShortcutHandler,
}

impl TauriShortcutRegistry {
    pub fn new(app: tauri::AppHandle, handler: ShortcutHandler) -> Self {
        Self { app, handler }
    }
}

impl ShortcutRegistry for TauriShortcutRegistry {
    fn register(&mut self, shortcut: &str, action: ShortcutAction) -> Result<(), ShortcutError> {
        let shortcut =
            Shortcut::from_str(shortcut).map_err(|_| ShortcutError::new("shortcut-invalid"))?;
        let handler = Arc::clone(&self.handler);
        self.app
            .global_shortcut()
            .on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state() == ShortcutState::Pressed {
                    handler(action);
                }
            })
            .map_err(|_| ShortcutError::new("shortcut-conflict"))
    }

    fn unregister(&mut self, shortcut: &str) -> Result<(), ShortcutError> {
        let shortcut =
            Shortcut::from_str(shortcut).map_err(|_| ShortcutError::new("shortcut-invalid"))?;
        self.app
            .global_shortcut()
            .unregister(shortcut)
            .map_err(|_| ShortcutError::new("shortcut-unregister-failed"))
    }

    fn unregister_all(&mut self) -> Result<(), ShortcutError> {
        self.app
            .global_shortcut()
            .unregister_all()
            .map_err(|_| ShortcutError::new("shortcut-unregister-failed"))
    }

    fn is_registered(&self, shortcut: &str) -> bool {
        Shortcut::from_str(shortcut)
            .is_ok_and(|shortcut| self.app.global_shortcut().is_registered(shortcut))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    struct FakeShortcutRegistry {
        registered: HashMap<String, ShortcutAction>,
        conflicts: HashSet<String>,
    }

    impl FakeShortcutRegistry {
        fn with_conflict(shortcut: &str) -> Self {
            Self {
                registered: HashMap::new(),
                conflicts: HashSet::from([shortcut.to_owned()]),
            }
        }

        fn action_for(&self, shortcut: &str) -> Option<ShortcutAction> {
            self.registered.get(shortcut).copied()
        }
    }

    impl ShortcutRegistry for FakeShortcutRegistry {
        fn register(
            &mut self,
            shortcut: &str,
            action: ShortcutAction,
        ) -> Result<(), ShortcutError> {
            if self.conflicts.contains(shortcut) {
                Err(ShortcutError::new("shortcut-conflict"))
            } else {
                self.registered.insert(shortcut.to_owned(), action);
                Ok(())
            }
        }

        fn unregister(&mut self, shortcut: &str) -> Result<(), ShortcutError> {
            self.registered.remove(shortcut);
            Ok(())
        }

        fn unregister_all(&mut self) -> Result<(), ShortcutError> {
            self.registered.clear();
            Ok(())
        }

        fn is_registered(&self, shortcut: &str) -> bool {
            self.registered.contains_key(shortcut)
        }
    }

    impl FakeShortcutRegistry {
        fn free() -> Self {
            Self {
                registered: HashMap::new(),
                conflicts: HashSet::new(),
            }
        }
    }

    #[test]
    fn cycle_shortcut_conflict_keeps_the_previous_one_registered() {
        let mut manager =
            ShortcutManager::new(FakeShortcutRegistry::with_conflict("Alt+2"), "Alt+T").unwrap();
        manager.replace_cycle(Some("Alt+1")).unwrap();

        let error = manager.replace_cycle(Some("Alt+2")).unwrap_err();

        assert_eq!(error.code, "shortcut-conflict");
        assert_eq!(manager.cycle(), Some("Alt+1"));
        assert!(manager.cycle_registered());
        // The rollback must not disturb the other slot.
        assert_eq!(manager.active(), Some("Alt+T"));
        assert!(manager.toggle_registered());
    }

    #[test]
    fn cycle_shortcut_clears_back_to_unassigned() {
        let mut manager = ShortcutManager::new(FakeShortcutRegistry::free(), "Alt+T").unwrap();
        manager.replace_cycle(Some("Alt+1")).unwrap();

        manager.replace_cycle(None).unwrap();

        assert_eq!(manager.cycle(), None);
        assert!(!manager.cycle_registered());
        assert!(!manager.registry().is_registered("Alt+1"));
        assert!(manager.toggle_registered());
    }

    #[test]
    fn the_two_app_shortcuts_cannot_share_one_accelerator() {
        let mut manager = ShortcutManager::new(FakeShortcutRegistry::free(), "Alt+T").unwrap();

        assert_eq!(
            manager.replace_cycle(Some("Alt+T")).unwrap_err().code,
            "shortcut-conflict"
        );
        assert_eq!(manager.cycle(), None);

        manager.replace_cycle(Some("Alt+1")).unwrap();
        assert_eq!(
            manager.replace("Alt+1").unwrap_err().code,
            "shortcut-conflict"
        );
        assert_eq!(manager.active(), Some("Alt+T"));
        assert!(manager.toggle_registered());
    }

    #[test]
    fn escape_stays_reserved_for_the_cycle_shortcut_too() {
        let mut manager = ShortcutManager::new(FakeShortcutRegistry::free(), "Alt+T").unwrap();

        assert_eq!(
            manager.replace_cycle(Some("Escape")).unwrap_err().code,
            "shortcut-reserved"
        );
        assert_eq!(manager.cycle(), None);
    }

    #[test]
    fn unregister_all_releases_both_app_shortcuts() {
        let mut manager = ShortcutManager::new(FakeShortcutRegistry::free(), "Alt+T").unwrap();
        manager.replace_cycle(Some("Alt+1")).unwrap();

        manager.unregister_all().unwrap();

        assert_eq!(manager.active(), None);
        assert_eq!(manager.cycle(), None);
        assert!(!manager.registry().is_registered("Alt+1"));
        assert!(!manager.registry().is_registered("Alt+T"));
    }

    #[test]
    fn the_cycle_shortcut_reaches_the_handler_as_a_cycle_action() {
        let mut manager = ShortcutManager::new(FakeShortcutRegistry::free(), "Alt+T").unwrap();
        manager.replace_cycle(Some("Alt+1")).unwrap();

        assert_eq!(
            manager.registry().action_for("Alt+1"),
            Some(ShortcutAction::CyclePreset)
        );
        assert_eq!(
            manager.registry().action_for("Alt+T"),
            Some(ShortcutAction::ToggleRun)
        );
    }

    #[test]
    fn shortcut_conflict_restores_previous_registration() {
        let registry = FakeShortcutRegistry::with_conflict("CommandOrControl+Alt+P");
        let mut manager = ShortcutManager::new(registry, "CommandOrControl+Shift+K").unwrap();

        let error = manager.replace("CommandOrControl+Alt+P").unwrap_err();

        assert_eq!(error.code, "shortcut-conflict");
        assert_eq!(manager.active(), Some("CommandOrControl+Shift+K"));
        assert!(manager.registry().is_registered("CommandOrControl+Shift+K"));
    }
}
