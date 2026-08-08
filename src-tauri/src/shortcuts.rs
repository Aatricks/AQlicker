use std::{fmt, str::FromStr, sync::Arc};

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub const ACTIVE_RUN_SHORTCUT: &str = "Escape";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    ToggleRun,
    StopRun,
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
    fn active(&self) -> Option<&str>;
    fn toggle_registered(&self) -> bool;
    fn unregister_toggle(&mut self) -> Result<(), ShortcutError>;
    fn register_escape(&mut self) -> Result<(), ShortcutError>;
    fn unregister_escape(&mut self) -> Result<(), ShortcutError>;
    fn escape_registered(&self) -> bool;
    fn unregister_all(&mut self) -> Result<(), ShortcutError>;
}

pub struct ShortcutManager<R: ShortcutRegistry> {
    registry: R,
    active: Option<String>,
    escape_registered: bool,
}

impl<R: ShortcutRegistry> ShortcutManager<R> {
    pub fn new(mut registry: R, shortcut: &str) -> Result<Self, ShortcutError> {
        ensure_toggle_is_not_escape(shortcut)?;
        registry.register(shortcut, ShortcutAction::ToggleRun)?;
        Ok(Self {
            registry,
            active: Some(shortcut.to_owned()),
            escape_registered: false,
        })
    }

    pub fn without_toggle(registry: R) -> Self {
        Self {
            registry,
            active: None,
            escape_registered: false,
        }
    }

    pub fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError> {
        ensure_toggle_is_not_escape(shortcut)?;
        if self.active.as_deref() == Some(shortcut) && self.registry.is_registered(shortcut) {
            return Ok(shortcut.to_owned());
        }

        let previous = self.active.take();
        if let Some(previous) = previous.as_deref() {
            if let Err(error) = self.registry.unregister(previous) {
                self.active = Some(previous.to_owned());
                return Err(error);
            }
        }

        match self.registry.register(shortcut, ShortcutAction::ToggleRun) {
            Ok(()) => {
                self.active = Some(shortcut.to_owned());
                Ok(shortcut.to_owned())
            }
            Err(error) => {
                if let Some(previous) = previous {
                    if self
                        .registry
                        .register(&previous, ShortcutAction::ToggleRun)
                        .is_err()
                    {
                        return Err(ShortcutError::new("shortcut-rollback-failed"));
                    }
                    self.active = Some(previous);
                }
                Err(error)
            }
        }
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
        if let Some(shortcut) = self.active.take() {
            if let Err(error) = self.registry.unregister(&shortcut) {
                self.active = Some(shortcut);
                return Err(error);
            }
        }
        Ok(())
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
        self.active = None;
        self.escape_registered = false;
        Ok(())
    }

    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }

    pub fn toggle_registered(&self) -> bool {
        self.active
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

    fn active(&self) -> Option<&str> {
        ShortcutManager::active(self)
    }

    fn toggle_registered(&self) -> bool {
        ShortcutManager::toggle_registered(self)
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

fn ensure_toggle_is_not_escape(shortcut: &str) -> Result<(), ShortcutError> {
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
    use std::collections::HashSet;

    use super::*;

    struct FakeShortcutRegistry {
        registered: HashSet<String>,
        conflicts: HashSet<String>,
    }

    impl FakeShortcutRegistry {
        fn with_conflict(shortcut: &str) -> Self {
            Self {
                registered: HashSet::new(),
                conflicts: HashSet::from([shortcut.to_owned()]),
            }
        }
    }

    impl ShortcutRegistry for FakeShortcutRegistry {
        fn register(
            &mut self,
            shortcut: &str,
            _action: ShortcutAction,
        ) -> Result<(), ShortcutError> {
            if self.conflicts.contains(shortcut) {
                Err(ShortcutError::new("shortcut-conflict"))
            } else {
                self.registered.insert(shortcut.to_owned());
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
            self.registered.contains(shortcut)
        }
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
