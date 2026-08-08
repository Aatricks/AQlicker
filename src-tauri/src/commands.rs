use std::{
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde::Serialize;
use tauri::Emitter;

use crate::{
    AppConfig, ConfigRepository, ConfigRepositoryError, RecoveryNotice, RunController, RunError,
    RunSnapshot, RunStatus, StartOutcome,
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

pub type RuntimeObserver = Arc<dyn Fn(u64, u64, RunSnapshot) + Send + Sync>;

pub trait RuntimeService: Send + Sync {
    fn set_observer(&mut self, observer: RuntimeObserver);
    fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError>;
    fn stop(&self) -> bool;
    fn snapshot(&self) -> RunSnapshot;
    fn shutdown(&self, timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError>;
}

impl RuntimeService for RunController {
    fn set_observer(&mut self, observer: RuntimeObserver) {
        RunController::set_tagged_observer(self, observer);
    }

    fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
        RunController::start(self, config).map_err(|error| CommandError::new(error.code))
    }

    fn stop(&self) -> bool {
        RunController::stop(self)
    }

    fn snapshot(&self) -> RunSnapshot {
        RunController::snapshot(self)
    }

    fn shutdown(&self, timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
        RunController::shutdown(self, timeout)
            .map(|snapshot| (self.generation(), self.revision(), snapshot))
            .map_err(|error| CommandError::new(error.code))
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
    observer: Mutex<RuntimeObserver>,
}

impl Default for DesktopRuntime {
    fn default() -> Self {
        Self {
            controller: Mutex::new(None),
            observer: Mutex::new(Arc::new(|_, _, _| {})),
        }
    }
}

impl RuntimeService for DesktopRuntime {
    fn set_observer(&mut self, observer: RuntimeObserver) {
        *lock(&self.observer) = observer;
    }

    fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
        let mut controller = lock(&self.controller);
        if controller.is_none() {
            let mut created =
                RunController::new().map_err(|_| CommandError::new("input-unavailable"))?;
            created.set_tagged_observer(Arc::clone(&lock(&self.observer)));
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

    fn shutdown(&self, timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
        lock(&self.controller).as_ref().map_or_else(
            || Ok((0, 0, RunSnapshot::idle())),
            |controller| {
                controller
                    .shutdown(timeout)
                    .map(|snapshot| (controller.generation(), controller.revision(), snapshot))
                    .map_err(|error| CommandError::new(error.code))
            },
        )
    }
}

pub struct AppService {
    submission: Mutex<SubmissionState>,
    shutdown_ready: Condvar,
    dispatcher: Mutex<Option<JoinHandle<()>>>,
    ingress: Arc<ServiceIngress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceLifecycle {
    Running,
    ShuttingDown,
    ShutdownFailed,
    Closed,
}

struct SubmissionState {
    lifecycle: ServiceLifecycle,
    sender: Option<Sender<ServiceAction>>,
}

struct ServiceIngress {
    senders: Mutex<Option<IngressSenders>>,
}

struct IngressSenders {
    actions: Sender<ServiceAction>,
    runtime: Sender<RuntimeEvent>,
}

impl ServiceIngress {
    fn publish(&self, generation: u64, revision: u64, snapshot: RunSnapshot) {
        if let Some(senders) = lock(&self.senders).as_ref() {
            let _ = senders.runtime.send((generation, revision, snapshot));
            let _ = senders.actions.send(ServiceAction::RuntimeWake);
        }
    }

    fn close(&self) {
        lock(&self.senders).take();
    }
}

type Reply<T> = Sender<Result<T, CommandError>>;
type RuntimeEvent = (u64, u64, RunSnapshot);

enum ServiceAction {
    Bootstrap(Reply<BootstrapPayload>),
    Start(AppConfig, Reply<RunSnapshot>),
    Save(AppConfig, Reply<()>),
    Stop(Reply<RunSnapshot>),
    RequestAccess(Reply<PermissionStatus>),
    PermissionStatus(Reply<PermissionStatus>),
    SetShortcut(String, Reply<String>),
    Shortcut(ShortcutAction, Option<Reply<RunSnapshot>>),
    Snapshot(Sender<RunSnapshot>),
    RuntimeWake,
    Shutdown(Reply<()>),
}

impl ServiceAction {
    fn allowed_while_shutting_down(&self) -> bool {
        matches!(
            self,
            Self::Stop(_) | Self::Shortcut(_, _) | Self::Snapshot(_) | Self::PermissionStatus(_)
        )
    }
}

struct ServiceCore {
    repository: ConfigRepository,
    permission: Box<dyn PermissionProvider>,
    shortcuts: Box<dyn ShortcutController>,
    runtime: Box<dyn RuntimeService>,
    emitter: Arc<dyn RunEventEmitter>,
    current_config: Option<AppConfig>,
    visible_run: RunSnapshot,
    active_generation: Option<u64>,
    last_event_generation: u64,
    last_event_revision: u64,
    escape_cleanup_required: bool,
    shutdown_pending: bool,
}

impl AppService {
    pub fn new(
        repository: ConfigRepository,
        permission: Box<dyn PermissionProvider>,
        shortcuts: Box<dyn ShortcutController>,
        mut runtime: Box<dyn RuntimeService>,
        emitter: Arc<dyn RunEventEmitter>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel();
        let (runtime_sender, runtime_receiver) = mpsc::channel();
        let ingress = Arc::new(ServiceIngress {
            senders: Mutex::new(Some(IngressSenders {
                actions: sender.clone(),
                runtime: runtime_sender,
            })),
        });
        let observed_ingress = Arc::downgrade(&ingress);
        runtime.set_observer(Arc::new(move |generation, revision, snapshot| {
            if let Some(ingress) = observed_ingress.upgrade() {
                ingress.publish(generation, revision, snapshot);
            }
        }));
        let visible_run = runtime.snapshot();
        let core = ServiceCore {
            repository,
            permission,
            shortcuts,
            runtime,
            emitter,
            current_config: None,
            visible_run,
            active_generation: None,
            last_event_generation: 0,
            last_event_revision: 0,
            escape_cleanup_required: false,
            shutdown_pending: false,
        };
        let dispatcher = thread::Builder::new()
            .name("aqlicker-service".to_owned())
            .spawn(move || dispatch_actions(receiver, runtime_receiver, core))
            .expect("failed to start AQlicker service dispatcher");
        Self {
            submission: Mutex::new(SubmissionState {
                lifecycle: ServiceLifecycle::Running,
                sender: Some(sender),
            }),
            shutdown_ready: Condvar::new(),
            dispatcher: Mutex::new(Some(dispatcher)),
            ingress,
        }
    }

    pub fn bootstrap(&self) -> Result<BootstrapPayload, CommandError> {
        self.request(ServiceAction::Bootstrap)
    }

    pub fn start(&self, config: AppConfig) -> Result<RunSnapshot, CommandError> {
        self.request(|reply| ServiceAction::Start(config, reply))
    }

    pub fn save_config(&self, config: AppConfig) -> Result<(), CommandError> {
        self.request(|reply| ServiceAction::Save(config, reply))
    }

    pub fn stop(&self) -> Result<RunSnapshot, CommandError> {
        self.request(ServiceAction::Stop)
    }

    pub fn request_access(&self) -> Result<PermissionStatus, CommandError> {
        self.request(ServiceAction::RequestAccess)
    }

    pub fn permission_status(&self) -> Result<PermissionStatus, CommandError> {
        self.request(ServiceAction::PermissionStatus)
    }

    pub fn set_shortcut(&self, shortcut: String) -> Result<String, CommandError> {
        self.request(|reply| ServiceAction::SetShortcut(shortcut, reply))
    }

    pub fn handle_shortcut(&self, action: ShortcutAction) -> Result<RunSnapshot, CommandError> {
        self.request(|reply| ServiceAction::Shortcut(action, Some(reply)))
    }

    pub fn enqueue_shortcut(&self, action: ShortcutAction) -> Result<(), CommandError> {
        self.enqueue(ServiceAction::Shortcut(action, None))
    }

    fn request<T>(
        &self,
        action: impl FnOnce(Reply<T>) -> ServiceAction,
    ) -> Result<T, CommandError> {
        let (reply, result) = mpsc::channel();
        self.enqueue(action(reply))?;
        result
            .recv()
            .map_err(|_| CommandError::new("service-unavailable"))?
    }

    fn enqueue(&self, action: ServiceAction) -> Result<(), CommandError> {
        let submission = lock(&self.submission);
        let admitted = submission.lifecycle == ServiceLifecycle::Running
            || (matches!(
                submission.lifecycle,
                ServiceLifecycle::ShuttingDown | ServiceLifecycle::ShutdownFailed
            ) && action.allowed_while_shutting_down());
        if !admitted {
            return Err(CommandError::new("service-shutting-down"));
        }
        submission
            .sender
            .as_ref()
            .ok_or_else(|| CommandError::new("service-unavailable"))?
            .send(action)
            .map_err(|_| CommandError::new("service-unavailable"))
    }

    pub fn run_snapshot(&self) -> RunSnapshot {
        let (reply, result) = mpsc::channel();
        if self.enqueue(ServiceAction::Snapshot(reply)).is_err() {
            return RunSnapshot::idle();
        }
        result.recv().unwrap_or_else(|_| RunSnapshot::idle())
    }

    pub fn shutdown(&self) -> Result<(), CommandError> {
        loop {
            let result = {
                let mut submission = lock(&self.submission);
                match submission.lifecycle {
                    ServiceLifecycle::Closed => return Ok(()),
                    ServiceLifecycle::ShuttingDown => {
                        drop(
                            self.shutdown_ready
                                .wait(submission)
                                .unwrap_or_else(|poisoned| poisoned.into_inner()),
                        );
                        continue;
                    }
                    ServiceLifecycle::Running | ServiceLifecycle::ShutdownFailed => {
                        let (reply, result) = mpsc::channel();
                        submission.lifecycle = ServiceLifecycle::ShuttingDown;
                        let sent = submission.sender.as_ref().is_some_and(|sender| {
                            sender.send(ServiceAction::Shutdown(reply)).is_ok()
                        });
                        if !sent {
                            submission.lifecycle = ServiceLifecycle::ShutdownFailed;
                            self.shutdown_ready.notify_all();
                            return Err(CommandError::new("service-unavailable"));
                        }
                        result
                    }
                }
            };

            let shutdown = result
                .recv()
                .unwrap_or_else(|_| Err(CommandError::new("service-unavailable")));
            if shutdown.is_ok() {
                self.ingress.close();
                lock(&self.submission).sender.take();
                if let Some(dispatcher) = lock(&self.dispatcher).take() {
                    let _ = dispatcher.join();
                }
            }
            let mut submission = lock(&self.submission);
            submission.lifecycle = if shutdown.is_ok() {
                ServiceLifecycle::Closed
            } else {
                ServiceLifecycle::ShutdownFailed
            };
            self.shutdown_ready.notify_all();
            return shutdown;
        }
    }
}

impl Drop for AppService {
    fn drop(&mut self) {
        let already_failed = lock(&self.submission).lifecycle == ServiceLifecycle::ShutdownFailed;
        if already_failed || self.shutdown().is_err() {
            {
                let mut submission = lock(&self.submission);
                submission.lifecycle = ServiceLifecycle::Closed;
                submission.sender.take();
            }
            self.ingress.close();
            if let Some(dispatcher) = lock(&self.dispatcher).take() {
                let deadline = std::time::Instant::now() + Duration::from_millis(100);
                while !dispatcher.is_finished() && std::time::Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(1));
                }
                if dispatcher.is_finished() {
                    let _ = dispatcher.join();
                }
            }
        }
    }
}

impl ServiceCore {
    fn bootstrap(&mut self) -> Result<BootstrapPayload, CommandError> {
        let loaded = self
            .repository
            .load()
            .map_err(|_| CommandError::new("config-load-failed"))?;
        let shortcut = match self.shortcuts.replace(&loaded.config.global_shortcut) {
            Ok(shortcut) => ShortcutRegistrationStatus {
                shortcut,
                registered: self.shortcuts.toggle_registered(),
                error: None,
            },
            Err(error) => ShortcutRegistrationStatus {
                shortcut: loaded.config.global_shortcut.clone(),
                registered: false,
                error: Some(error.code.to_owned()),
            },
        };
        let permission = self.permission.status();
        self.visible_run = self.runtime.snapshot();
        self.emitter.emit(&self.visible_run);
        self.current_config = Some(loaded.config.clone());
        Ok(BootstrapPayload {
            config: loaded.config,
            recovery_notice: loaded.notice,
            permission,
            shortcut,
            run: self.visible_run.clone(),
        })
    }

    fn start(&mut self, config: AppConfig) -> Result<RunSnapshot, CommandError> {
        self.retry_escape_cleanup()?;
        if !config.validate_for_start().is_empty() {
            return Err(CommandError::new("invalid-config"));
        }
        if !self.permission.status().granted {
            return Err(CommandError::new("permission-required"));
        }
        if self.shortcuts.active() != Some(config.global_shortcut.as_str())
            || !self.shortcuts.toggle_registered()
        {
            return Err(CommandError::new("shortcut-unavailable"));
        }
        self.shortcuts
            .register_escape()
            .map_err(|_| CommandError::new("escape-unavailable"))?;
        if !self.shortcuts.escape_registered() {
            return Err(CommandError::new("escape-unavailable"));
        }

        let was_active = self.active_generation.is_some();
        match self.runtime.start(config.clone()) {
            Ok(StartOutcome::Started { generation }) => {
                self.active_generation = Some(generation);
                self.current_config = Some(config);
            }
            Ok(StartOutcome::TerminalPending) => {
                if !was_active {
                    self.unregister_escape_or_fail()?;
                }
                return Err(CommandError::new("run-terminal-pending"));
            }
            Ok(StartOutcome::BusyRunning) => {
                self.visible_run = self.runtime.snapshot();
                if !matches!(
                    self.visible_run.status,
                    RunStatus::Running | RunStatus::Stopping
                ) {
                    if !was_active {
                        self.unregister_escape_or_fail()?;
                    }
                    return Err(CommandError::new("run-busy"));
                }
            }
            Err(error) => {
                self.unregister_escape_or_fail()?;
                return Err(error);
            }
        }
        self.visible_run = self.runtime.snapshot();
        Ok(self.visible_run.clone())
    }

    fn save_config(&mut self, config: AppConfig) -> Result<(), CommandError> {
        if !config.validate().is_empty() {
            return Err(CommandError::new("invalid-config"));
        }
        let previous = self.shortcuts.active().map(str::to_owned);
        self.shortcuts.replace(&config.global_shortcut)?;
        if let Err(error) = self.repository.save(&config) {
            let rollback = match previous {
                Some(previous) => self.shortcuts.replace(&previous).map(|_| ()),
                None => self.shortcuts.unregister_toggle(),
            };
            if rollback.is_err() {
                return Err(CommandError::new("shortcut-rollback-failed"));
            }
            return Err(match error {
                ConfigRepositoryError::InvalidConfig => CommandError::new("invalid-config"),
                _ => CommandError::new("config-save-failed"),
            });
        }
        self.current_config = Some(config);
        Ok(())
    }

    fn stop(&mut self) -> RunSnapshot {
        self.runtime.stop();
        self.visible_run = self.runtime.snapshot();
        self.visible_run.clone()
    }

    fn current_or_loaded_config(&mut self) -> Result<AppConfig, CommandError> {
        match self.current_config.clone() {
            Some(config) => Ok(config),
            None => self
                .repository
                .load()
                .map(|loaded| loaded.config)
                .map_err(|_| CommandError::new("config-load-failed")),
        }
    }

    fn handle_shortcut(&mut self, action: ShortcutAction) -> Result<RunSnapshot, CommandError> {
        if self.shutdown_pending {
            return Ok(self.stop());
        }
        match action {
            ShortcutAction::StopRun => Ok(self.stop()),
            ShortcutAction::ToggleRun => match self.runtime.snapshot().status {
                RunStatus::Running => Ok(self.stop()),
                RunStatus::Stopping | RunStatus::Failed => Ok(self.runtime.snapshot()),
                RunStatus::Idle => {
                    let config = self.current_or_loaded_config()?;
                    self.start(config)
                }
            },
        }
    }

    fn runtime_snapshot(&mut self, generation: u64, revision: u64, snapshot: RunSnapshot) {
        if generation < self.last_event_generation
            || (generation == self.last_event_generation && revision <= self.last_event_revision)
        {
            return;
        }
        self.last_event_generation = generation;
        self.last_event_revision = revision;
        if self
            .active_generation
            .is_some_and(|active| active != generation)
        {
            return;
        }
        if self.shutdown_pending {
            self.visible_run = snapshot;
            self.emitter.emit(&self.visible_run);
            return;
        }
        if matches!(snapshot.status, RunStatus::Idle | RunStatus::Failed)
            && self.active_generation == Some(generation)
        {
            if self.shortcuts.unregister_escape().is_err() {
                self.active_generation = None;
                self.escape_cleanup_required = true;
                self.publish_cleanup_failure();
                return;
            }
            self.active_generation = None;
            self.escape_cleanup_required = false;
        }
        if !self.escape_cleanup_required {
            self.visible_run = snapshot;
            self.emitter.emit(&self.visible_run);
        }
    }

    fn retry_escape_cleanup(&mut self) -> Result<(), CommandError> {
        if !self.escape_cleanup_required {
            return Ok(());
        }
        self.unregister_escape_or_fail()
    }

    fn unregister_escape_or_fail(&mut self) -> Result<(), CommandError> {
        match self.shortcuts.unregister_escape() {
            Ok(()) => {
                self.escape_cleanup_required = false;
                Ok(())
            }
            Err(_) => {
                self.escape_cleanup_required = true;
                self.publish_cleanup_failure();
                Err(CommandError::new("escape-cleanup-failed"))
            }
        }
    }

    fn publish_cleanup_failure(&mut self) {
        self.visible_run.status = RunStatus::Failed;
        self.visible_run.error = Some(RunError {
            code: "escape-cleanup-failed".to_owned(),
            key: None,
            message: "failed to unregister the active-run Escape shortcut".to_owned(),
        });
        self.emitter.emit(&self.visible_run);
    }

    fn shutdown(&mut self) -> Result<(), CommandError> {
        self.shutdown_pending = true;
        let (generation, revision, snapshot) = self.runtime.shutdown(SHUTDOWN_TIMEOUT)?;
        self.runtime_snapshot(generation, revision, snapshot);
        self.shortcuts
            .unregister_all()
            .map_err(CommandError::from)?;
        self.active_generation = None;
        self.escape_cleanup_required = false;
        self.shutdown_pending = false;
        Ok(())
    }
}

fn dispatch_actions(
    receiver: Receiver<ServiceAction>,
    runtime_receiver: Receiver<RuntimeEvent>,
    mut core: ServiceCore,
) {
    while let Ok(action) = receiver.recv() {
        while let Ok((generation, revision, snapshot)) = runtime_receiver.try_recv() {
            core.runtime_snapshot(generation, revision, snapshot);
        }
        match action {
            ServiceAction::Bootstrap(reply) => {
                let _ = reply.send(core.bootstrap());
            }
            ServiceAction::Start(config, reply) => {
                let _ = reply.send(core.start(config));
            }
            ServiceAction::Save(config, reply) => {
                let _ = reply.send(core.save_config(config));
            }
            ServiceAction::Stop(reply) => {
                let _ = reply.send(Ok(core.stop()));
            }
            ServiceAction::RequestAccess(reply) => {
                let _ = reply.send(Ok(core.permission.request_access()));
            }
            ServiceAction::PermissionStatus(reply) => {
                let _ = reply.send(Ok(core.permission.status()));
            }
            ServiceAction::SetShortcut(shortcut, reply) => {
                let result = core.current_or_loaded_config().and_then(|mut config| {
                    config.global_shortcut = shortcut.clone();
                    core.save_config(config).map(|()| shortcut)
                });
                let _ = reply.send(result);
            }
            ServiceAction::Shortcut(action, reply) => {
                let result = core.handle_shortcut(action);
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            ServiceAction::Snapshot(reply) => {
                let _ = reply.send(core.visible_run.clone());
            }
            ServiceAction::RuntimeWake => {}
            ServiceAction::Shutdown(reply) => {
                let result = core.shutdown();
                let complete = result.is_ok();
                let _ = reply.send(result);
                if complete {
                    break;
                }
            }
        }
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
    state.request_access()
}

#[tauri::command]
pub fn permission_status(
    state: tauri::State<'_, AppService>,
) -> Result<PermissionStatus, CommandError> {
    state.permission_status()
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
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use super::*;
    use crate::{KeyEntry, LogicalKey, StopReason};

    type Signal = Arc<(Mutex<bool>, Condvar)>;

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

    struct BlockingPermission {
        entered: Signal,
        released: Signal,
    }

    impl PermissionProvider for BlockingPermission {
        fn status(&mut self) -> PermissionStatus {
            signal(&self.entered);
            let (lock, ready) = &*self.released;
            let released = lock.lock().unwrap();
            let _released = ready.wait_while(released, |released| !*released).unwrap();
            PermissionStatus {
                granted: true,
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
        let _ = service.run_snapshot();

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

    #[test]
    fn terminal_cleanup_precedes_a_queued_new_start() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let operations = Arc::new(Mutex::new(Vec::new()));
        let escape_registered = Arc::new(AtomicBool::new(false));
        let shortcuts = FakeShortcuts {
            operations: Arc::clone(&operations),
            escape_registered: Arc::clone(&escape_registered),
            ..FakeShortcuts::available()
        };
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(shortcuts),
            Box::new(FakeRuntime::new(Arc::clone(&starts))),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        service.start(valid_config()).unwrap();

        service.enqueue_shortcut(ShortcutAction::StopRun).unwrap();
        service.enqueue_shortcut(ShortcutAction::ToggleRun).unwrap();
        let snapshot = service.run_snapshot();

        assert_eq!(snapshot.status, RunStatus::Running);
        assert!(escape_registered.load(Ordering::SeqCst));
        assert_eq!(*starts.lock().unwrap(), 2);
        assert_eq!(
            operations.lock().unwrap().as_slice(),
            ["register-escape", "unregister-escape", "register-escape"]
        );
        service.shutdown().unwrap();
    }

    #[test]
    fn stale_terminal_generation_cannot_remove_the_new_runs_escape_shortcut() {
        let directory = tempfile::tempdir().unwrap();
        let escape_registered = Arc::new(AtomicBool::new(true));
        let mut core = ServiceCore {
            repository: ConfigRepository::new(directory.path()),
            permission: Box::new(FakePermission { granted: true }),
            shortcuts: Box::new(FakeShortcuts::with_escape_state(Arc::clone(
                &escape_registered,
            ))),
            runtime: Box::new(FakeRuntime::new(Arc::new(Mutex::new(0)))),
            emitter: Arc::new(RecordingEmitter::default()),
            current_config: Some(valid_config()),
            visible_run: RunSnapshot {
                status: RunStatus::Running,
                ..RunSnapshot::idle()
            },
            active_generation: Some(2),
            last_event_generation: 2,
            last_event_revision: 1,
            escape_cleanup_required: false,
            shutdown_pending: false,
        };
        let stale_terminal = RunSnapshot {
            status: RunStatus::Idle,
            stop_reason: Some(StopReason::Requested),
            ..RunSnapshot::idle()
        };

        core.runtime_snapshot(1, 9, stale_terminal);

        assert_eq!(core.active_generation, Some(2));
        assert_eq!(core.visible_run.status, RunStatus::Running);
        assert!(escape_registered.load(Ordering::SeqCst));
    }

    #[test]
    fn delayed_stopping_snapshot_cannot_overwrite_same_generation_terminal_state() {
        let directory = tempfile::tempdir().unwrap();
        let escape_registered = Arc::new(AtomicBool::new(true));
        let mut core = ServiceCore {
            repository: ConfigRepository::new(directory.path()),
            permission: Box::new(FakePermission { granted: true }),
            shortcuts: Box::new(FakeShortcuts::with_escape_state(Arc::clone(
                &escape_registered,
            ))),
            runtime: Box::new(FakeRuntime::new(Arc::new(Mutex::new(0)))),
            emitter: Arc::new(RecordingEmitter::default()),
            current_config: Some(valid_config()),
            visible_run: RunSnapshot {
                status: RunStatus::Running,
                ..RunSnapshot::idle()
            },
            active_generation: Some(7),
            last_event_generation: 7,
            last_event_revision: 1,
            escape_cleanup_required: false,
            shutdown_pending: false,
        };
        let terminal = RunSnapshot {
            status: RunStatus::Idle,
            stop_reason: Some(StopReason::DurationComplete),
            ..RunSnapshot::idle()
        };
        let stopping = RunSnapshot {
            status: RunStatus::Stopping,
            ..RunSnapshot::idle()
        };

        core.runtime_snapshot(7, 3, terminal);
        core.runtime_snapshot(7, 2, stopping);

        assert_eq!(core.visible_run.status, RunStatus::Idle);
        assert_eq!(
            core.visible_run.stop_reason,
            Some(StopReason::DurationComplete)
        );
    }

    #[test]
    fn terminal_snapshot_before_worker_finish_returns_truthful_pending_start() {
        let directory = tempfile::tempdir().unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let starts = Arc::new(AtomicUsize::new(0));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(FakeShortcuts::available()),
            Box::new(TerminalPendingRuntime::new(
                Arc::clone(&finished),
                Arc::clone(&starts),
            )),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        service.start(valid_config()).unwrap();
        assert_eq!(service.run_snapshot().status, RunStatus::Idle);

        let pending = service.start(valid_config()).unwrap_err();

        assert_eq!(pending.code, "run-terminal-pending");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        finished.store(true, Ordering::SeqCst);
        assert_eq!(
            service.start(valid_config()).unwrap().status,
            RunStatus::Running
        );
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        service.shutdown().unwrap();
    }

    #[test]
    fn prepublication_terminal_window_never_acknowledges_idle_start_or_toggle() {
        for _ in 0..5 {
            let directory = tempfile::tempdir().unwrap();
            let controls = Arc::new(PrePublicationControls::new());
            let service = AppService::new(
                ConfigRepository::new(directory.path()),
                Box::new(FakePermission { granted: true }),
                Box::new(FakeShortcuts::available()),
                Box::new(PrePublicationRuntime::new(Arc::clone(&controls))),
                Arc::new(RecordingEmitter::default()),
            );
            service.save_config(valid_config()).unwrap();
            assert_eq!(
                service.start(valid_config()).unwrap().status,
                RunStatus::Running
            );
            signal(&controls.trigger_terminal);
            wait_until_true(&controls.idle_committed);

            let start = service.start(valid_config());
            let toggle = service.handle_shortcut(ShortcutAction::ToggleRun);

            signal(&controls.publish_terminal);
            wait_until_true(&controls.terminal_published);
            let _ = service.run_snapshot();
            assert_eq!(start.unwrap_err().code, "run-terminal-pending");
            assert_eq!(toggle.unwrap_err().code, "run-terminal-pending");
            assert_eq!(controls.starts.load(Ordering::SeqCst), 1);
            assert_eq!(
                service.start(valid_config()).unwrap().status,
                RunStatus::Running
            );
            assert_eq!(controls.starts.load(Ordering::SeqCst), 2);
            service.shutdown().unwrap();
        }
    }

    #[test]
    fn rapid_toggle_pairs_are_atomic_and_end_idle() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let escape_registered = Arc::new(AtomicBool::new(false));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(FakeShortcuts::with_escape_state(Arc::clone(
                &escape_registered,
            ))),
            Box::new(FakeRuntime::new(Arc::clone(&starts))),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();

        for _ in 0..32 {
            service.enqueue_shortcut(ShortcutAction::ToggleRun).unwrap();
            service.enqueue_shortcut(ShortcutAction::ToggleRun).unwrap();
        }
        let snapshot = service.run_snapshot();

        assert_eq!(snapshot.status, RunStatus::Idle);
        assert_eq!(*starts.lock().unwrap(), 32);
        assert!(!escape_registered.load(Ordering::SeqCst));
        service.shutdown().unwrap();
    }

    #[test]
    fn escape_cleanup_failure_is_visible_blocks_start_and_retries() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let cleanup_failures = Arc::new(AtomicUsize::new(2));
        let shortcuts = FakeShortcuts {
            cleanup_failures: Arc::clone(&cleanup_failures),
            ..FakeShortcuts::available()
        };
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(shortcuts),
            Box::new(FakeRuntime::new(Arc::clone(&starts))),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        service.start(valid_config()).unwrap();
        service.stop().unwrap();

        let failed = service.run_snapshot();
        assert_eq!(failed.status, RunStatus::Failed);
        assert_eq!(failed.error.unwrap().code, "escape-cleanup-failed");
        assert_eq!(
            service.start(valid_config()).unwrap_err().code,
            "escape-cleanup-failed"
        );
        assert_eq!(*starts.lock().unwrap(), 1);

        let restarted = service.start(valid_config()).unwrap();
        assert_eq!(restarted.status, RunStatus::Running);
        assert_eq!(*starts.lock().unwrap(), 2);
        service.shutdown().unwrap();
    }

    #[test]
    fn failed_start_surfaces_escape_cleanup_failure_and_allows_retry() {
        let directory = tempfile::tempdir().unwrap();
        let starts = Arc::new(Mutex::new(0));
        let start_failures = Arc::new(AtomicUsize::new(1));
        let cleanup_failures = Arc::new(AtomicUsize::new(1));
        let shortcuts = FakeShortcuts {
            cleanup_failures,
            ..FakeShortcuts::available()
        };
        let runtime = FakeRuntime::with_controls(
            Arc::clone(&starts),
            start_failures,
            Arc::new(Mutex::new(Vec::new())),
        );
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(shortcuts),
            Box::new(runtime),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();

        assert_eq!(
            service.start(valid_config()).unwrap_err().code,
            "escape-cleanup-failed"
        );
        assert_eq!(
            service.run_snapshot().error.unwrap().code,
            "escape-cleanup-failed"
        );
        assert_eq!(
            service.start(valid_config()).unwrap().status,
            RunStatus::Running
        );
        assert_eq!(*starts.lock().unwrap(), 1);
        service.shutdown().unwrap();
    }

    #[test]
    fn save_and_start_share_one_ordered_configuration_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let replace_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let replace_released = Arc::new((Mutex::new(false), Condvar::new()));
        let starts = Arc::new(Mutex::new(0));
        let started_configs = Arc::new(Mutex::new(Vec::new()));
        let shortcuts = FakeShortcuts {
            replace_gate: Some((Arc::clone(&replace_entered), Arc::clone(&replace_released))),
            ..FakeShortcuts::available()
        };
        let service = Arc::new(AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(shortcuts),
            Box::new(FakeRuntime::with_controls(
                Arc::clone(&starts),
                Arc::new(AtomicUsize::new(0)),
                Arc::clone(&started_configs),
            )),
            Arc::new(RecordingEmitter::default()),
        ));
        let config = AppConfig {
            global_shortcut: "CommandOrControl+Alt+P".to_owned(),
            ..valid_config()
        };

        let saving_service = Arc::clone(&service);
        let saved_config = config.clone();
        let saving = thread::spawn(move || saving_service.save_config(saved_config));
        wait_until_true(&replace_entered);
        let starting_service = Arc::clone(&service);
        let start_config = config.clone();
        let starting = thread::spawn(move || starting_service.start(start_config));
        signal(&replace_released);

        saving.join().unwrap().unwrap();
        assert_eq!(starting.join().unwrap().unwrap().status, RunStatus::Running);
        assert_eq!(started_configs.lock().unwrap().as_slice(), &[config]);
        service.shutdown().unwrap();
    }

    #[test]
    fn shutdown_gates_new_starts_and_drains_an_already_accepted_start() {
        let directory = tempfile::tempdir().unwrap();
        let permission_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let permission_released = Arc::new((Mutex::new(false), Condvar::new()));
        let starts = Arc::new(Mutex::new(0));
        let service = Arc::new(AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(BlockingPermission {
                entered: Arc::clone(&permission_entered),
                released: Arc::clone(&permission_released),
            }),
            Box::new(FakeShortcuts::available()),
            Box::new(FakeRuntime::new(Arc::clone(&starts))),
            Arc::new(RecordingEmitter::default()),
        ));
        service.save_config(valid_config()).unwrap();

        let starting_service = Arc::clone(&service);
        let starting = thread::spawn(move || starting_service.start(valid_config()));
        wait_until_true(&permission_entered);
        let shutdown_service = Arc::clone(&service);
        let shutdown = thread::spawn(move || shutdown_service.shutdown());
        wait_until_lifecycle(&service, ServiceLifecycle::ShuttingDown);

        assert_eq!(
            service.start(valid_config()).unwrap_err().code,
            "service-shutting-down"
        );
        signal(&permission_released);
        assert_eq!(starting.join().unwrap().unwrap().status, RunStatus::Running);
        shutdown.join().unwrap().unwrap();
        assert_eq!(*starts.lock().unwrap(), 1);
    }

    #[test]
    fn shutdown_timeout_preserves_stop_paths_and_retry_then_closes() {
        let directory = tempfile::tempdir().unwrap();
        let escape_registered = Arc::new(AtomicBool::new(false));
        let toggle_registered = Arc::new(AtomicBool::new(true));
        let starts = Arc::new(AtomicUsize::new(0));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(TrackedShortcuts::new(
                Arc::clone(&escape_registered),
                Arc::clone(&toggle_registered),
                None,
            )),
            Box::new(RetryShutdownRuntime::new(
                Arc::clone(&starts),
                Arc::clone(&shutdowns),
                Some(2),
                None,
            )),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        service.start(valid_config()).unwrap();

        assert_eq!(service.shutdown().unwrap_err().code, "wait-timeout");
        assert!(escape_registered.load(Ordering::SeqCst));
        assert!(toggle_registered.load(Ordering::SeqCst));
        service.handle_shortcut(ShortcutAction::StopRun).unwrap();
        service.handle_shortcut(ShortcutAction::ToggleRun).unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            service.start(valid_config()).unwrap_err().code,
            "service-shutting-down"
        );
        assert_eq!(
            service.save_config(valid_config()).unwrap_err().code,
            "service-shutting-down"
        );

        service.shutdown().unwrap();

        assert_eq!(shutdowns.load(Ordering::SeqCst), 2);
        assert!(!escape_registered.load(Ordering::SeqCst));
        assert!(!toggle_registered.load(Ordering::SeqCst));
    }

    #[test]
    fn dropping_after_shutdown_timeout_releases_dispatcher_owned_resources() {
        let directory = tempfile::tempdir().unwrap();
        let runtime_dropped = Arc::new((Mutex::new(false), Condvar::new()));
        let shortcuts_dropped = Arc::new((Mutex::new(false), Condvar::new()));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let service = AppService::new(
            ConfigRepository::new(directory.path()),
            Box::new(FakePermission { granted: true }),
            Box::new(TrackedShortcuts::new(
                Arc::new(AtomicBool::new(false)),
                Arc::new(AtomicBool::new(true)),
                Some(Arc::clone(&shortcuts_dropped)),
            )),
            Box::new(RetryShutdownRuntime::new(
                Arc::new(AtomicUsize::new(0)),
                Arc::clone(&shutdowns),
                None,
                Some(Arc::clone(&runtime_dropped)),
            )),
            Arc::new(RecordingEmitter::default()),
        );
        service.save_config(valid_config()).unwrap();
        service.start(valid_config()).unwrap();
        assert_eq!(service.shutdown().unwrap_err().code, "wait-timeout");

        drop(service);

        wait_until_true(&runtime_dropped);
        wait_until_true(&shortcuts_dropped);
        assert_eq!(shutdowns.load(Ordering::SeqCst), 1);
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
        cleanup_failures: Arc<AtomicUsize>,
        operations: Arc<Mutex<Vec<&'static str>>>,
        replace_gate: Option<(Signal, Signal)>,
    }

    impl FakeShortcuts {
        fn available() -> Self {
            Self {
                active: "CommandOrControl+Shift+K".to_owned(),
                toggle_registered: true,
                escape_available: true,
                escape_registered: Arc::new(AtomicBool::new(false)),
                cleanup_failures: Arc::new(AtomicUsize::new(0)),
                operations: Arc::new(Mutex::new(Vec::new())),
                replace_gate: None,
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

    struct TrackedShortcuts {
        active: String,
        escape_registered: Arc<AtomicBool>,
        toggle_registered: Arc<AtomicBool>,
        dropped: Option<Signal>,
    }

    impl TrackedShortcuts {
        fn new(
            escape_registered: Arc<AtomicBool>,
            toggle_registered: Arc<AtomicBool>,
            dropped: Option<Signal>,
        ) -> Self {
            Self {
                active: valid_config().global_shortcut,
                escape_registered,
                toggle_registered,
                dropped,
            }
        }
    }

    impl Drop for TrackedShortcuts {
        fn drop(&mut self) {
            if let Some(dropped) = &self.dropped {
                signal(dropped);
            }
        }
    }

    impl ShortcutController for TrackedShortcuts {
        fn replace(&mut self, shortcut: &str) -> Result<String, ShortcutError> {
            self.active = shortcut.to_owned();
            self.toggle_registered.store(true, Ordering::SeqCst);
            Ok(shortcut.to_owned())
        }

        fn active(&self) -> Option<&str> {
            Some(&self.active)
        }

        fn toggle_registered(&self) -> bool {
            self.toggle_registered.load(Ordering::SeqCst)
        }

        fn unregister_toggle(&mut self) -> Result<(), ShortcutError> {
            self.toggle_registered.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn register_escape(&mut self) -> Result<(), ShortcutError> {
            self.escape_registered.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn unregister_escape(&mut self) -> Result<(), ShortcutError> {
            self.escape_registered.store(false, Ordering::SeqCst);
            Ok(())
        }

        fn escape_registered(&self) -> bool {
            self.escape_registered.load(Ordering::SeqCst)
        }

        fn unregister_all(&mut self) -> Result<(), ShortcutError> {
            self.escape_registered.store(false, Ordering::SeqCst);
            self.toggle_registered.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    struct RetryShutdownRuntime {
        snapshot: Mutex<RunSnapshot>,
        observer: Mutex<RuntimeObserver>,
        starts: Arc<AtomicUsize>,
        shutdowns: Arc<AtomicUsize>,
        succeed_on: Option<usize>,
        generation: AtomicUsize,
        revision: AtomicUsize,
        dropped: Option<Signal>,
    }

    impl RetryShutdownRuntime {
        fn new(
            starts: Arc<AtomicUsize>,
            shutdowns: Arc<AtomicUsize>,
            succeed_on: Option<usize>,
            dropped: Option<Signal>,
        ) -> Self {
            Self {
                snapshot: Mutex::new(RunSnapshot::idle()),
                observer: Mutex::new(Arc::new(|_, _, _| {})),
                starts,
                shutdowns,
                succeed_on,
                generation: AtomicUsize::new(0),
                revision: AtomicUsize::new(0),
                dropped,
            }
        }

        fn publish(&self, snapshot: RunSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot.clone();
            let generation = self.generation.load(Ordering::SeqCst) as u64;
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            Arc::clone(&self.observer.lock().unwrap())(generation, revision, snapshot);
        }
    }

    impl Drop for RetryShutdownRuntime {
        fn drop(&mut self) {
            if let Some(dropped) = &self.dropped {
                signal(dropped);
            }
        }
    }

    impl RuntimeService for RetryShutdownRuntime {
        fn set_observer(&mut self, observer: RuntimeObserver) {
            *self.observer.lock().unwrap() = observer;
        }

        fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
            if self.snapshot.lock().unwrap().status != RunStatus::Idle {
                return Ok(StartOutcome::BusyRunning);
            }
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            self.revision.store(0, Ordering::SeqCst);
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.publish(RunSnapshot {
                status: RunStatus::Running,
                mode: Some(config.mode),
                ..RunSnapshot::idle()
            });
            Ok(StartOutcome::Started { generation })
        }

        fn stop(&self) -> bool {
            if self.snapshot.lock().unwrap().status != RunStatus::Running {
                return false;
            }
            self.publish(RunSnapshot {
                status: RunStatus::Idle,
                stop_reason: Some(StopReason::Requested),
                ..RunSnapshot::idle()
            });
            true
        }

        fn snapshot(&self) -> RunSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn shutdown(&self, _timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
            let attempt = self.shutdowns.fetch_add(1, Ordering::SeqCst) + 1;
            if self
                .succeed_on
                .is_none_or(|succeed_on| attempt < succeed_on)
            {
                return Err(CommandError::new("wait-timeout"));
            }
            self.stop();
            Ok((
                self.generation.load(Ordering::SeqCst) as u64,
                self.revision.load(Ordering::SeqCst) as u64,
                self.snapshot(),
            ))
        }
    }

    struct PrePublicationControls {
        trigger_terminal: Signal,
        idle_committed: Signal,
        publish_terminal: Signal,
        terminal_published: Signal,
        starts: AtomicUsize,
    }

    impl PrePublicationControls {
        fn new() -> Self {
            Self {
                trigger_terminal: Arc::new((Mutex::new(false), Condvar::new())),
                idle_committed: Arc::new((Mutex::new(false), Condvar::new())),
                publish_terminal: Arc::new((Mutex::new(false), Condvar::new())),
                terminal_published: Arc::new((Mutex::new(false), Condvar::new())),
                starts: AtomicUsize::new(0),
            }
        }
    }

    struct PrePublicationRuntime {
        snapshot: Arc<Mutex<RunSnapshot>>,
        observer: Arc<Mutex<RuntimeObserver>>,
        controls: Arc<PrePublicationControls>,
        terminal_pending: Arc<AtomicBool>,
        generation: Arc<AtomicUsize>,
        revision: Arc<AtomicUsize>,
    }

    impl PrePublicationRuntime {
        fn new(controls: Arc<PrePublicationControls>) -> Self {
            Self {
                snapshot: Arc::new(Mutex::new(RunSnapshot::idle())),
                observer: Arc::new(Mutex::new(Arc::new(|_, _, _| {}))),
                controls,
                terminal_pending: Arc::new(AtomicBool::new(false)),
                generation: Arc::new(AtomicUsize::new(0)),
                revision: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn publish(&self, snapshot: RunSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot.clone();
            let generation = self.generation.load(Ordering::SeqCst) as u64;
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            Arc::clone(&self.observer.lock().unwrap())(generation, revision, snapshot);
        }
    }

    impl RuntimeService for PrePublicationRuntime {
        fn set_observer(&mut self, observer: RuntimeObserver) {
            *self.observer.lock().unwrap() = observer;
        }

        fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
            if self.terminal_pending.load(Ordering::SeqCst) {
                return Ok(StartOutcome::TerminalPending);
            }
            if self.snapshot.lock().unwrap().status != RunStatus::Idle {
                return Ok(StartOutcome::BusyRunning);
            }
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            self.revision.store(0, Ordering::SeqCst);
            self.controls.starts.fetch_add(1, Ordering::SeqCst);
            self.publish(RunSnapshot {
                status: RunStatus::Running,
                mode: Some(config.mode),
                ..RunSnapshot::idle()
            });
            if generation == 1 {
                let snapshot = Arc::clone(&self.snapshot);
                let observer = Arc::clone(&self.observer);
                let controls = Arc::clone(&self.controls);
                let terminal_pending = Arc::clone(&self.terminal_pending);
                let revision = Arc::clone(&self.revision);
                thread::spawn(move || {
                    wait_until_true(&controls.trigger_terminal);
                    terminal_pending.store(true, Ordering::SeqCst);
                    let terminal = RunSnapshot {
                        status: RunStatus::Idle,
                        mode: Some(config.mode),
                        stop_reason: Some(StopReason::DurationComplete),
                        ..RunSnapshot::idle()
                    };
                    *snapshot.lock().unwrap() = terminal.clone();
                    let terminal_revision = revision.fetch_add(1, Ordering::SeqCst) as u64 + 1;
                    signal(&controls.idle_committed);
                    wait_until_true(&controls.publish_terminal);
                    Arc::clone(&observer.lock().unwrap())(generation, terminal_revision, terminal);
                    terminal_pending.store(false, Ordering::SeqCst);
                    signal(&controls.terminal_published);
                });
            }
            Ok(StartOutcome::Started { generation })
        }

        fn stop(&self) -> bool {
            if self.snapshot.lock().unwrap().status != RunStatus::Running {
                return false;
            }
            self.publish(RunSnapshot {
                status: RunStatus::Idle,
                stop_reason: Some(StopReason::Requested),
                ..RunSnapshot::idle()
            });
            true
        }

        fn snapshot(&self) -> RunSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn shutdown(&self, _timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
            self.stop();
            Ok((
                self.generation.load(Ordering::SeqCst) as u64,
                self.revision.load(Ordering::SeqCst) as u64,
                self.snapshot(),
            ))
        }
    }

    struct TerminalPendingRuntime {
        snapshot: Mutex<RunSnapshot>,
        observer: Mutex<RuntimeObserver>,
        finished: Arc<AtomicBool>,
        starts: Arc<AtomicUsize>,
        generation: AtomicUsize,
        revision: AtomicUsize,
    }

    impl TerminalPendingRuntime {
        fn new(finished: Arc<AtomicBool>, starts: Arc<AtomicUsize>) -> Self {
            Self {
                snapshot: Mutex::new(RunSnapshot::idle()),
                observer: Mutex::new(Arc::new(|_, _, _| {})),
                finished,
                starts,
                generation: AtomicUsize::new(0),
                revision: AtomicUsize::new(0),
            }
        }

        fn publish(&self, generation: u64, snapshot: RunSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot.clone();
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            Arc::clone(&self.observer.lock().unwrap())(generation, revision, snapshot);
        }
    }

    impl RuntimeService for TerminalPendingRuntime {
        fn set_observer(&mut self, observer: RuntimeObserver) {
            *self.observer.lock().unwrap() = observer;
        }

        fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
            let current = self.generation.load(Ordering::SeqCst);
            if current == 1 && !self.finished.load(Ordering::SeqCst) {
                return Ok(StartOutcome::TerminalPending);
            }
            if self.snapshot.lock().unwrap().status != RunStatus::Idle {
                return Ok(StartOutcome::BusyRunning);
            }
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            self.revision.store(0, Ordering::SeqCst);
            self.starts.fetch_add(1, Ordering::SeqCst);
            self.publish(
                generation,
                RunSnapshot {
                    status: RunStatus::Running,
                    mode: Some(config.mode),
                    ..RunSnapshot::idle()
                },
            );
            if generation == 1 {
                self.publish(
                    generation,
                    RunSnapshot {
                        status: RunStatus::Idle,
                        mode: Some(config.mode),
                        stop_reason: Some(StopReason::DurationComplete),
                        ..RunSnapshot::idle()
                    },
                );
            }
            Ok(StartOutcome::Started { generation })
        }

        fn stop(&self) -> bool {
            if self.snapshot.lock().unwrap().status != RunStatus::Running {
                return false;
            }
            let generation = self.generation.load(Ordering::SeqCst) as u64;
            self.publish(
                generation,
                RunSnapshot {
                    status: RunStatus::Idle,
                    stop_reason: Some(StopReason::Requested),
                    ..RunSnapshot::idle()
                },
            );
            true
        }

        fn snapshot(&self) -> RunSnapshot {
            self.snapshot.lock().unwrap().clone()
        }

        fn shutdown(&self, _timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
            self.finished.store(true, Ordering::SeqCst);
            self.stop();
            Ok((
                self.generation.load(Ordering::SeqCst) as u64,
                self.revision.load(Ordering::SeqCst) as u64,
                self.snapshot(),
            ))
        }
    }

    struct FakeRuntime {
        snapshot: Mutex<RunSnapshot>,
        observer: Mutex<RuntimeObserver>,
        starts: Arc<Mutex<usize>>,
        start_failures: Arc<AtomicUsize>,
        started_configs: Arc<Mutex<Vec<AppConfig>>>,
        generation: AtomicUsize,
        revision: AtomicUsize,
    }

    impl FakeRuntime {
        fn new(starts: Arc<Mutex<usize>>) -> Self {
            Self {
                snapshot: Mutex::new(RunSnapshot::idle()),
                observer: Mutex::new(Arc::new(|_, _, _| {})),
                starts,
                start_failures: Arc::new(AtomicUsize::new(0)),
                started_configs: Arc::new(Mutex::new(Vec::new())),
                generation: AtomicUsize::new(0),
                revision: AtomicUsize::new(0),
            }
        }

        fn with_controls(
            starts: Arc<Mutex<usize>>,
            start_failures: Arc<AtomicUsize>,
            started_configs: Arc<Mutex<Vec<AppConfig>>>,
        ) -> Self {
            Self {
                snapshot: Mutex::new(RunSnapshot::idle()),
                observer: Mutex::new(Arc::new(|_, _, _| {})),
                starts,
                start_failures,
                started_configs,
                generation: AtomicUsize::new(0),
                revision: AtomicUsize::new(0),
            }
        }

        fn publish(&self, snapshot: RunSnapshot) {
            *self.snapshot.lock().unwrap() = snapshot.clone();
            let observer = Arc::clone(&self.observer.lock().unwrap());
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            observer(
                self.generation.load(Ordering::SeqCst) as u64,
                revision,
                snapshot,
            );
        }
    }

    impl RuntimeService for FakeRuntime {
        fn set_observer(&mut self, observer: RuntimeObserver) {
            *self.observer.lock().unwrap() = observer;
        }

        fn start(&self, config: AppConfig) -> Result<StartOutcome, CommandError> {
            if self.snapshot.lock().unwrap().status != RunStatus::Idle {
                return Ok(StartOutcome::BusyRunning);
            }
            if consume_failure(&self.start_failures) {
                return Err(CommandError::new("start-failed"));
            }
            *self.starts.lock().unwrap() += 1;
            self.started_configs.lock().unwrap().push(config.clone());
            let generation = self.generation.fetch_add(1, Ordering::SeqCst) as u64 + 1;
            self.revision.store(0, Ordering::SeqCst);
            self.publish(RunSnapshot {
                status: RunStatus::Running,
                mode: Some(config.mode),
                remaining_ms: config.stop_after.map(|seconds| u64::from(seconds) * 1_000),
                ..RunSnapshot::idle()
            });
            Ok(StartOutcome::Started { generation })
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

        fn shutdown(&self, _timeout: Duration) -> Result<(u64, u64, RunSnapshot), CommandError> {
            self.stop();
            Ok((
                self.generation.load(Ordering::SeqCst) as u64,
                self.revision.load(Ordering::SeqCst) as u64,
                self.snapshot(),
            ))
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
            if let Some((entered, released)) = &self.replace_gate {
                signal(entered);
                let (lock, ready) = &**released;
                let released = lock.lock().unwrap();
                let _released = ready.wait_while(released, |released| !*released).unwrap();
            }
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
                self.operations.lock().unwrap().push("register-escape");
                self.escape_registered.store(true, Ordering::SeqCst);
                Ok(())
            } else {
                Err(ShortcutError::new("shortcut-conflict"))
            }
        }

        fn unregister_escape(&mut self) -> Result<(), ShortcutError> {
            if consume_failure(&self.cleanup_failures) {
                self.operations
                    .lock()
                    .unwrap()
                    .push("unregister-escape-failed");
                return Err(ShortcutError::new("shortcut-unregister-failed"));
            }
            self.operations.lock().unwrap().push("unregister-escape");
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

    fn consume_failure(failures: &AtomicUsize) -> bool {
        failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
    }

    fn wait_until_true(pair: &Signal) {
        let (lock, ready) = &**pair;
        let entered = lock.lock().unwrap();
        let (entered, result) = ready
            .wait_timeout_while(entered, Duration::from_secs(1), |entered| !*entered)
            .unwrap();
        assert!(!result.timed_out() && *entered);
    }

    fn signal(pair: &Signal) {
        let (lock, ready) = &**pair;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }

    fn wait_until_lifecycle(service: &AppService, expected: ServiceLifecycle) {
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        loop {
            if lock(&service.submission).lifecycle == expected {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "lifecycle did not advance"
            );
            thread::yield_now();
        }
    }
}
