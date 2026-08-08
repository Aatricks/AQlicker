use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::Emitter;

use crate::{
    AppConfig, ConfigRepository, ConfigRepositoryError, RecoveryNotice, RunController, RunObserver,
    RunSnapshot, RunStatus,
    permission::{PermissionProvider, PermissionStatus},
    shortcuts::{ShortcutAction, ShortcutController, ShortcutError},
};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
pub const RUN_STATE_EVENT: &str = "aqlicker://run-state";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutRegistrationStatus {
    pub shortcut: String,
    pub registered: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub config: AppConfig,
    pub recovery_notice: Option<RecoveryNotice>,
    pub permission: PermissionStatus,
    pub shortcut: ShortcutRegistrationStatus,
    pub run: RunSnapshot,
}

impl CommandError {
    fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }
}

impl From<ShortcutError> for CommandError {
    fn from(error: ShortcutError) -> Self {
        Self::new(error.code)
    }
}

pub trait RuntimeService: Send + Sync {
    fn set_observer(&mut self, observer: RunObserver);
    fn start(&self, config: AppConfig) -> Result<bool, CommandError>;
    fn stop(&self) -> bool;
    fn snapshot(&self) -> RunSnapshot;
    fn shutdown(&self, timeout: Duration) -> Result<RunSnapshot, CommandError>;
}

impl RuntimeService for RunController {
    fn set_observer(&mut self, observer: RunObserver) {
        RunController::set_observer(self, observer);
    }

    fn start(&self, config: AppConfig) -> Result<bool, CommandError> {
        RunController::start(self, config).map_err(|error| CommandError::new(error.code))
    }

    fn stop(&self) -> bool {
        RunController::stop(self)
    }

    fn snapshot(&self) -> RunSnapshot {
        RunController::snapshot(self)
    }

    fn shutdown(&self, timeout: Duration) -> Result<RunSnapshot, CommandError> {
        RunController::shutdown(self, timeout).map_err(|error| CommandError::new(error.code))
    }
}

pub trait RunEventEmitter: Send + Sync {
    fn emit(&self, snapshot: &RunSnapshot);
}

pub struct TauriRunEventEmitter {
    app: tauri::AppHandle,
}

impl TauriRunEventEmitter {
    pub const fn new(app: tauri::AppHandle) -> Self {
        Self { app }
    }
}

impl RunEventEmitter for TauriRunEventEmitter {
    fn emit(&self, snapshot: &RunSnapshot) {
        let _ = self.app.emit(RUN_STATE_EVENT, snapshot.clone());
    }
}

pub struct DesktopRuntime {
    controller: Mutex<Option<RunController>>,
    observer: Mutex<RunObserver>,
}

impl Default for DesktopRuntime {
    fn default() -> Self {
        Self {
            controller: Mutex::new(None),
            observer: Mutex::new(Arc::new(|_| {})),
        }
    }
}

impl RuntimeService for DesktopRuntime {
    fn set_observer(&mut self, observer: RunObserver) {
        *lock(&self.observer) = observer;
    }

    fn start(&self, config: AppConfig) -> Result<bool, CommandError> {
        let mut controller = lock(&self.controller);
        if controller.is_none() {
            let mut created =
                RunController::new().map_err(|_| CommandError::new("input-unavailable"))?;
            created.set_observer(Arc::clone(&lock(&self.observer)));
            *controller = Some(created);
        }
        controller
            .as_ref()
            .unwrap()
            .start(config)
            .map_err(|error| CommandError::new(error.code))
    }

    fn stop(&self) -> bool {
        lock(&self.controller)
            .as_ref()
            .is_some_and(RunController::stop)
    }

    fn snapshot(&self) -> RunSnapshot {
        lock(&self.controller)
            .as_ref()
            .map_or_else(RunSnapshot::idle, RunController::snapshot)
    }

    fn shutdown(&self, timeout: Duration) -> Result<RunSnapshot, CommandError> {
        lock(&self.controller).as_ref().map_or_else(
            || Ok(RunSnapshot::idle()),
            |controller| {
                controller
                    .shutdown(timeout)
                    .map_err(|error| CommandError::new(error.code))
            },
        )
    }
}

pub struct AppService {
    repository: Mutex<ConfigRepository>,
    permission: Mutex<Box<dyn PermissionProvider>>,
    shortcuts: Arc<Mutex<Box<dyn ShortcutController>>>,
    runtime: Box<dyn RuntimeService>,
    emitter: Arc<dyn RunEventEmitter>,
    current_config: Mutex<Option<AppConfig>>,
}

impl AppService {
    pub fn new(
        repository: ConfigRepository,
        permission: Box<dyn PermissionProvider>,
        shortcuts: Box<dyn ShortcutController>,
        mut runtime: Box<dyn RuntimeService>,
        emitter: Arc<dyn RunEventEmitter>,
    ) -> Self {
        let shortcuts = Arc::new(Mutex::new(shortcuts));
        let observed_shortcuts = Arc::clone(&shortcuts);
        let observed_emitter = Arc::clone(&emitter);
        runtime.set_observer(Arc::new(move |snapshot| {
            if matches!(snapshot.status, RunStatus::Idle | RunStatus::Failed) {
                let _ = lock(&observed_shortcuts).unregister_escape();
            }
            observed_emitter.emit(&snapshot);
        }));
        Self {
            repository: Mutex::new(repository),
            permission: Mutex::new(permission),
            shortcuts,
            runtime,
            emitter,
            current_config: Mutex::new(None),
        }
    }

    pub fn bootstrap(&self) -> Result<BootstrapPayload, CommandError> {
        let loaded = lock(&self.repository)
            .load()
            .map_err(|_| CommandError::new("config-load-failed"))?;
        let shortcut = {
            let mut shortcuts = lock(&self.shortcuts);
            match shortcuts.replace(&loaded.config.global_shortcut) {
                Ok(shortcut) => ShortcutRegistrationStatus {
                    shortcut,
                    registered: shortcuts.toggle_registered(),
                    error: None,
                },
                Err(error) => ShortcutRegistrationStatus {
                    shortcut: loaded.config.global_shortcut.clone(),
                    registered: false,
                    error: Some(error.code.to_owned()),
                },
            }
        };
        let permission = lock(&self.permission).status();
        let run = self.runtime.snapshot();
        self.emitter.emit(&run);
        *lock(&self.current_config) = Some(loaded.config.clone());
        Ok(BootstrapPayload {
            config: loaded.config,
            recovery_notice: loaded.notice,
            permission,
            shortcut,
            run,
        })
    }

    pub fn start(&self, config: AppConfig) -> Result<RunSnapshot, CommandError> {
        if !config.validate_for_start().is_empty() {
            return Err(CommandError::new("invalid-config"));
        }
        if !lock(&self.permission).status().granted {
            return Err(CommandError::new("permission-required"));
        }
        {
            let mut shortcuts = lock(&self.shortcuts);
            if shortcuts.active() != Some(config.global_shortcut.as_str())
                || !shortcuts.toggle_registered()
            {
                return Err(CommandError::new("shortcut-unavailable"));
            }
            shortcuts
                .register_escape()
                .map_err(|_| CommandError::new("escape-unavailable"))?;
            if !shortcuts.escape_registered() {
                return Err(CommandError::new("escape-unavailable"));
            }
        }

        match self.runtime.start(config.clone()) {
            Ok(true) => *lock(&self.current_config) = Some(config),
            Ok(false) => {}
            Err(error) => {
                let _ = lock(&self.shortcuts).unregister_escape();
                return Err(error);
            }
        }
        Ok(self.runtime.snapshot())
    }

    pub fn save_config(&self, config: AppConfig) -> Result<(), CommandError> {
        if !config.validate().is_empty() {
            return Err(CommandError::new("invalid-config"));
        }

        let mut shortcuts = lock(&self.shortcuts);
        let previous = shortcuts.active().map(str::to_owned);
        shortcuts.replace(&config.global_shortcut)?;
        if let Err(error) = lock(&self.repository).save(&config) {
            let rollback = match previous {
                Some(previous) => shortcuts.replace(&previous).map(|_| ()),
                None => shortcuts.unregister_toggle(),
            };
            if rollback.is_err() {
                return Err(CommandError::new("shortcut-rollback-failed"));
            }
            return Err(match error {
                ConfigRepositoryError::InvalidConfig => CommandError::new("invalid-config"),
                _ => CommandError::new("config-save-failed"),
            });
        }
        drop(shortcuts);
        *lock(&self.current_config) = Some(config);
        Ok(())
    }

    pub fn stop(&self) -> Result<RunSnapshot, CommandError> {
        self.runtime.stop();
        Ok(self.runtime.snapshot())
    }

    pub fn request_access(&self) -> PermissionStatus {
        lock(&self.permission).request_access()
    }

    pub fn permission_status(&self) -> PermissionStatus {
        lock(&self.permission).status()
    }

    pub fn set_shortcut(&self, shortcut: String) -> Result<String, CommandError> {
        let mut config = self.current_or_loaded_config()?;
        config.global_shortcut = shortcut.clone();
        self.save_config(config)?;
        Ok(shortcut)
    }

    pub fn handle_shortcut(&self, action: ShortcutAction) -> Result<RunSnapshot, CommandError> {
        match action {
            ShortcutAction::StopRun => self.stop(),
            ShortcutAction::ToggleRun => match self.runtime.snapshot().status {
                RunStatus::Running => self.stop(),
                RunStatus::Stopping | RunStatus::Failed => Ok(self.runtime.snapshot()),
                RunStatus::Idle => self.start(self.current_or_loaded_config()?),
            },
        }
    }

    fn current_or_loaded_config(&self) -> Result<AppConfig, CommandError> {
        match lock(&self.current_config).clone() {
            Some(config) => Ok(config),
            None => lock(&self.repository)
                .load()
                .map(|loaded| loaded.config)
                .map_err(|_| CommandError::new("config-load-failed")),
        }
    }

    pub fn run_snapshot(&self) -> RunSnapshot {
        self.runtime.snapshot()
    }

    pub fn shutdown(&self) -> Result<(), CommandError> {
        let runtime_result = self.runtime.shutdown(SHUTDOWN_TIMEOUT).map(|_| ());
        let shortcut_result = lock(&self.shortcuts)
            .unregister_all()
            .map_err(CommandError::from);
        runtime_result.and(shortcut_result)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[tauri::command]
pub fn bootstrap(state: tauri::State<'_, AppService>) -> Result<BootstrapPayload, CommandError> {
    state.bootstrap()
}

#[tauri::command]
pub fn save_config(
    config: AppConfig,
    state: tauri::State<'_, AppService>,
) -> Result<(), CommandError> {
    state.save_config(config)
}

#[tauri::command]
pub fn start_run(
    config: AppConfig,
    state: tauri::State<'_, AppService>,
) -> Result<RunSnapshot, CommandError> {
    state.start(config)
}

#[tauri::command]
pub fn stop_run(state: tauri::State<'_, AppService>) -> Result<RunSnapshot, CommandError> {
    state.stop()
}

#[tauri::command]
pub fn request_access(
    state: tauri::State<'_, AppService>,
) -> Result<PermissionStatus, CommandError> {
    Ok(state.request_access())
}

#[tauri::command]
pub fn permission_status(
    state: tauri::State<'_, AppService>,
) -> Result<PermissionStatus, CommandError> {
    Ok(state.permission_status())
}

#[tauri::command]
pub fn set_shortcut(
    shortcut: String,
    state: tauri::State<'_, AppService>,
) -> Result<String, CommandError> {
    state.set_shortcut(shortcut)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    use super::*;
    use crate::{KeyEntry, LogicalKey, StopReason};

    struct FakePermission {
        granted: bool,
    }

    impl PermissionProvider for FakePermission {
        fn status(&mut self) -> PermissionStatus {
            PermissionStatus {
                granted: self.granted,
                same_integrity_only: false,
            }
        }

        fn request_access(&mut self) -> PermissionStatus {
            self.status()
        }
    }

    #[test]
    fn start_requires_permission_before_spawning_input() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let service = test_service(
            directory.path(),
            Box::new(FakePermission { granted: false }),
            Arc::clone(&starts),
        );

        let error = service.start(valid_config()).unwrap_err();

        assert_eq!(error.code, "permission-required");
        assert_eq!(*starts.lock().unwrap(), 0);
        assert_eq!(service.run_snapshot().status, RunStatus::Idle);
    }

    #[test]
    fn start_rejects_invalid_config_before_spawning_input() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let service = test_service(
            directory.path(),
            Box::new(FakePermission { granted: true }),
            Arc::clone(&starts),
        );

        assert_eq!(
            service.start(AppConfig::default()).unwrap_err().code,
            "invalid-config"
        );
        assert_eq!(*starts.lock().unwrap(), 0);
        assert_eq!(service.run_snapshot().status, RunStatus::Idle);
    }

    #[test]
    fn start_rejects_unavailable_toggle_and_escape_before_spawning_input() {
        for (shortcuts, expected) in [
            (FakeShortcuts::without_toggle(), "shortcut-unavailable"),
            (FakeShortcuts::without_escape(), "escape-unavailable"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let starts = Arc::new(Mutex::new(0));
            let service = AppService::new(
                ConfigRepository::new(directory.path()),
                Box::new(FakePermission { granted: true }),
                Box::new(shortcuts),
                Box::new(FakeRuntime::new(Arc::clone(&starts))),
                Arc::new(RecordingEmitter::default()),
            );

            assert_eq!(service.start(valid_config()).unwrap_err().code, expected);
            assert_eq!(*starts.lock().unwrap(), 0);
            assert_eq!(service.run_snapshot().status, RunStatus::Idle);
        }
    }

    #[test]
    fn bootstrap_returns_idle_service_state_and_emits_its_snapshot() {
        let directory = tempfile::tempdir().unwrap();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(FakeShortcuts::without_toggle()),
            Box::new(FakeRuntime::new(Arc::new(Mutex::new(0)))),
            Arc::new(RecordingEmitter {
                emitted: Arc::clone(&emitted),
            }),
        );

        let payload = service.bootstrap().unwrap();

        assert_eq!(payload.config, AppConfig::default());
        assert!(payload.recovery_notice.is_none());
        assert!(payload.permission.granted);
        assert!(payload.shortcut.registered);
        assert_eq!(payload.run.status, RunStatus::Idle);
        assert_eq!(emitted.lock().unwrap().as_slice(), &[payload.run]);
    }

    #[test]
    fn save_config_persists_valid_data_and_replaces_the_toggle() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let service = test_service(
            directory.path(),
            Box::new(FakePermission { granted: true }),
            Arc::clone(&starts),
        );
        let config = AppConfig {
            global_shortcut: "CommandOrControl+Alt+P".to_owned(),
            ..valid_config()
        };

        service.save_config(config.clone()).unwrap();

        assert_eq!(
            ConfigRepository::new(directory.path())
                .load()
                .unwrap()
                .config,
            config
        );
        service.start(config).unwrap();
        assert_eq!(*starts.lock().unwrap(), 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn escape_action_only_stops_and_never_starts() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let service = test_service(
            directory.path(),
            Box::new(FakePermission { granted: true }),
            Arc::clone(&starts),
        );
        service.save_config(valid_config()).unwrap();

        service.handle_shortcut(ShortcutAction::StopRun).unwrap();

        assert_eq!(*starts.lock().unwrap(), 0);
        assert_eq!(service.run_snapshot().status, RunStatus::Idle);
    }

    #[test]
    fn configured_toggle_starts_then_stops_the_current_config() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let service = test_service(
            directory.path(),
            Box::new(FakePermission { granted: true }),
            Arc::clone(&starts),
        );
        service.save_config(valid_config()).unwrap();

        let started = service.handle_shortcut(ShortcutAction::ToggleRun).unwrap();
        assert_eq!(started.status, RunStatus::Running);
        service.handle_shortcut(ShortcutAction::ToggleRun).unwrap();
        service.shutdown().unwrap();

        assert_eq!(*starts.lock().unwrap(), 1);
        assert_eq!(service.run_snapshot().status, RunStatus::Idle);
    }

    #[test]
    fn escape_is_registered_only_for_the_active_run() {
        let directory = tempfile::tempdir().unwrap();
        let escape_registered = Arc::new(AtomicBool::new(false));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(FakeShortcuts::with_escape_state(Arc::clone(
                &escape_registered,
            ))),
            Box::new(FakeRuntime::new(Arc::new(Mutex::new(0)))),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        assert!(!escape_registered.load(Ordering::SeqCst));

        service.start(valid_config()).unwrap();
        assert!(escape_registered.load(Ordering::SeqCst));
        service.stop().unwrap();
        service.shutdown().unwrap();

        assert!(!escape_registered.load(Ordering::SeqCst));
    }

    #[test]
    fn every_runtime_snapshot_is_forwarded_to_the_run_state_emitter() {
        let directory = tempfile::tempdir().unwrap();
        let emitted = Arc::new(Mutex::new(Vec::new()));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(FakeShortcuts::available()),
            Box::new(FakeRuntime::new(Arc::new(Mutex::new(0)))),
            Arc::new(RecordingEmitter {
                emitted: Arc::clone(&emitted),
            }),
        );
        service.save_config(valid_config()).unwrap();

        service.start(valid_config()).unwrap();
        service.stop().unwrap();

        assert_eq!(
            emitted
                .lock()
                .unwrap()
                .iter()
                .map(|snapshot| snapshot.status)
                .collect::<Vec<_>>(),
            vec![RunStatus::Running, RunStatus::Stopping, RunStatus::Idle]
        );
    }

    fn test_service(
        directory: &std::path::Path,
        permission: Box<dyn PermissionProvider>,
        starts: Arc<Mutex<usize>>,
    ) -> AppService {
        AppService::new(
            ConfigRepository::new(directory),
            permission,
            Box::new(FakeShortcuts::available()),
            Box::new(FakeRuntime::new(starts)),
            Arc::new(RecordingEmitter::default()),
        )
    }

    fn valid_config() -> AppConfig {
        AppConfig {
            keys: vec![KeyEntry::new(LogicalKey::KeyA)],
            stop_after: Some(1),
            ..AppConfig::default()
        }
    }

    struct FakeShortcuts {
        active: String,
        toggle_registered: bool,
        escape_available: bool,
        escape_registered: Arc<AtomicBool>,
    }

    impl FakeShortcuts {
        fn available() -> Self {
            Self {
                active: "CommandOrControl+Shift+K".to_owned(),
                toggle_registered: true,
                escape_available: true,
                escape_registered: Arc::new(AtomicBool::new(false)),
            }
        }

        fn with_escape_state(escape_registered: Arc<AtomicBool>) -> Self {
            Self {
                escape_registered,
                ..Self::available()
            }
        }

        fn without_toggle() -> Self {
            Self {
                toggle_registered: false,
                ..Self::available()
            }
        }

        fn without_escape() -> Self {
            Self {
                escape_available: false,
                ..Self::available()
            }
        }
    }

    struct FakeRuntime {
        snapshot: Mutex<RunSnapshot>,
        observer: Mutex<RunObserver>,
        starts: Arc<Mutex<usize>>,
    }

    impl FakeRuntime {
        fn new(starts: Arc<Mutex<usize>>) -> Self {
            Self {
                snapshot: Mutex::new(RunSnapshot::idle()),
                observer: Mutex::new(Arc::new(|_| {})),
                starts,
            }
        }

        fn publish(&self, snapshot: RunSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot.clone();
            let observer = Arc::clone(&self.observer.lock().unwrap());
            observer(snapshot);
        }
    }

    impl RuntimeService for FakeRuntime {
        fn set_observer(&mut self, observer: RunObserver) {
            *self.observer.lock().unwrap() = observer;
        }

        fn start(&self, config: AppConfig) -> Result<bool, CommandError> {
            *self.starts.lock().unwrap() += 1;
            if self.snapshot.lock().unwrap().status != RunStatus::Idle {
                return Ok(false);
            }
            self.publish(RunSnapshot {
                status: RunStatus::Running,
                mode: Some(config.mode),
                remaining_ms: config.stop_after.map(|seconds| u64::from(seconds) * 1_000),
                ..RunSnapshot::idle()
            });
            Ok(true)
        }

        fn stop(&self) -> bool {
            if self.snapshot.lock().unwrap().status != RunStatus::Running {
                return false;
            }
            let mut stopping = self.snapshot();
            stopping.status = RunStatus::Stopping;
            self.publish(stopping.clone());
            stopping.status = RunStatus::Idle;
            stopping.stop_reason = Some(StopReason::Requested);
            self.publish(stopping);
            true
        }

        fn snapshot(&self) -> RunSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn shutdown(&self, _timeout: Duration) -> Result<RunSnapshot, CommandError> {
            self.stop();
            Ok(self.snapshot())
        }
    }

    #[derive(Default)]
    struct RecordingEmitter {
        emitted: Arc<Mutex<Vec<RunSnapshot>>>,
    }

    impl RunEventEmitter for RecordingEmitter {
        fn emit(&self, snapshot: &RunSnapshot) {
            self.emitted.lock().unwrap().push(snapshot.clone());
        }
    }

    impl ShortcutController for FakeShortcuts {
        fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError> {
            self.active = shortcut.to_owned();
            self.toggle_registered = true;
            Ok(shortcut.to_owned())
        }

        fn active(&self) -> Option<&str> {
            Some(&self.active)
        }

        fn toggle_registered(&self) -> bool {
            self.toggle_registered
        }

        fn unregister_toggle(&mut self) -> Result<(), ShortcutError> {
            self.toggle_registered = false;
            Ok(())
        }

        fn register_escape(&mut self) -> Result<(), ShortcutError> {
            if self.escape_available {
                self.escape_registered.store(true, Ordering::SeqCst);
                Ok(())
            } else {
                Err(ShortcutError::new("shortcut-conflict"))
            }
        }

        fn unregister_escape(&mut self) -> Result<(), ShortcutError> {
            self.escape_registered.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn escape_registered(&self) -> bool {
            self.escape_registered.load(Ordering::SeqCst)
        }

        fn unregister_all(&mut self) -> Result<(), ShortcutError> {
            self.toggle_registered = false;
            self.escape_registered.store(false, Ordering::SeqCst);
            Ok(())
        }
    }
}
