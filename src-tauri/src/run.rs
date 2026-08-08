use std::{
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::{
    AppConfig, LogicalKey, Mode,
    input::{EnigoInputSink, InputFailure, InputSink},
    scheduler::{
        NaturalSchedule, NaturalSettings, PressSchedule, TimerSchedule, is_before_deadline,
    },
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunStatus {
    Idle,
    Running,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Requested,
    DurationComplete,
    InputFailure,
    WorkerPanic,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunError {
    pub code: String,
    pub key: Option<LogicalKey>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshot {
    pub status: RunStatus,
    pub mode: Option<Mode>,
    pub elapsed_ms: u64,
    pub remaining_ms: Option<u64>,
    pub successful_presses: u64,
    pub stop_reason: Option<StopReason>,
    pub error: Option<RunError>,
}

impl RunSnapshot {
    pub const fn idle() -> Self {
        Self {
            status: RunStatus::Idle,
            mode: None,
            elapsed_ms: 0,
            remaining_ms: None,
            successful_presses: 0,
            stop_reason: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartError {
    pub code: &'static str,
}

impl std::fmt::Display for StartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code)
    }
}

impl std::error::Error for StartError {}

pub type RunObserver = Arc<dyn Fn(RunSnapshot) + Send + Sync>;
pub(crate) type TaggedRunObserver = Arc<dyn Fn(u64, u64, RunSnapshot) + Send + Sync>;

#[derive(Debug, Clone, Copy)]
enum Control {
    Cancel,
}

struct RuntimeState {
    snapshot: RunSnapshot,
    started_at: Option<Instant>,
    deadline: Option<Duration>,
    worker: Option<JoinHandle<()>>,
    worker_complete: bool,
    generation: u64,
    revision: u64,
}

struct SharedState {
    state: Mutex<RuntimeState>,
    terminal: Condvar,
    observer: Mutex<TaggedRunObserver>,
}

trait Clock: Send + Sync {
    fn elapsed(&self) -> Duration;
    fn wait_until(&self, target: Duration, receiver: &Receiver<Control>) -> bool;
}

struct RealClock {
    started: Instant,
}

impl RealClock {
    fn new(started: Instant) -> Self {
        Self { started }
    }
}

impl Clock for RealClock {
    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn wait_until(&self, target: Duration, receiver: &Receiver<Control>) -> bool {
        let remaining = target.saturating_sub(self.elapsed());
        if remaining.is_zero() {
            match receiver.try_recv() {
                Ok(Control::Cancel) | Err(TryRecvError::Disconnected) => true,
                Err(TryRecvError::Empty) => false,
            }
        } else {
            match receiver.recv_timeout(remaining) {
                Ok(Control::Cancel) | Err(RecvTimeoutError::Disconnected) => true,
                Err(RecvTimeoutError::Timeout) => false,
            }
        }
    }
}

pub struct RunController {
    sink: Arc<Mutex<Box<dyn InputSink>>>,
    clock_factory: Arc<dyn Fn(Instant) -> Arc<dyn Clock> + Send + Sync>,
    shared: Arc<SharedState>,
    control: Mutex<Option<Sender<Control>>>,
    #[cfg(test)]
    before_worker_publish: Option<Arc<WorkerPublishGate>>,
    #[cfg(test)]
    before_worker_exit: Option<Arc<WorkerPublishGate>>,
}

#[cfg(test)]
struct WorkerPublishGate {
    armed: AtomicBool,
    entered: Arc<(Mutex<bool>, Condvar)>,
    released: Arc<(Mutex<bool>, Condvar)>,
}

#[cfg(test)]
impl WorkerPublishGate {
    fn new() -> Self {
        Self {
            armed: AtomicBool::new(true),
            entered: Arc::new((Mutex::new(false), Condvar::new())),
            released: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn pause_once(&self) {
        if !self.armed.swap(false, Ordering::SeqCst) {
            return;
        }
        let (entered, ready) = &*self.entered;
        *lock(entered) = true;
        ready.notify_all();
        let (released, ready) = &*self.released;
        let mut released = lock(released);
        while !*released {
            released = ready
                .wait(released)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl RunController {
    pub fn new() -> Result<Self, InputFailure> {
        Ok(Self::with_sink(Box::new(EnigoInputSink::new()?)))
    }

    pub fn with_sink(sink: Box<dyn InputSink>) -> Self {
        Self {
            sink: Arc::new(Mutex::new(sink)),
            clock_factory: Arc::new(|started| Arc::new(RealClock::new(started))),
            shared: Arc::new(SharedState {
                state: Mutex::new(RuntimeState {
                    snapshot: RunSnapshot::idle(),
                    started_at: None,
                    deadline: None,
                    worker: None,
                    worker_complete: true,
                    generation: 0,
                    revision: 0,
                }),
                terminal: Condvar::new(),
                observer: Mutex::new(Arc::new(|_, _, _| {})),
            }),
            control: Mutex::new(None),
            #[cfg(test)]
            before_worker_publish: None,
            #[cfg(test)]
            before_worker_exit: None,
        }
    }

    pub fn set_observer(&mut self, observer: RunObserver) {
        *lock(&self.shared.observer) = Arc::new(move |_, _, snapshot| observer(snapshot));
    }

    pub(crate) fn set_tagged_observer(&mut self, observer: TaggedRunObserver) {
        *lock(&self.shared.observer) = observer;
    }

    pub(crate) fn generation(&self) -> u64 {
        lock(&self.shared.state).generation
    }

    pub(crate) fn revision(&self) -> u64 {
        lock(&self.shared.state).revision
    }

    #[cfg(test)]
    fn for_test(sink: Box<dyn InputSink>) -> Self {
        Self::with_sink(sink)
    }

    #[cfg(test)]
    fn for_test_with_clock<C>(sink: Box<dyn InputSink>, clock: Arc<C>) -> Self
    where
        C: Clock + 'static,
    {
        let mut controller = Self::with_sink(sink);
        controller.clock_factory = Arc::new(move |_started| clock.clone());
        controller
    }

    pub fn start(&self, request: AppConfig) -> Result<bool, StartError> {
        {
            let state = lock(&self.shared.state);
            if !worker_ready(&state) {
                return Ok(false);
            }
            match state.snapshot.status {
                RunStatus::Running | RunStatus::Stopping => return Ok(false),
                RunStatus::Failed => return Err(StartError { code: "run-failed" }),
                RunStatus::Idle => {}
            }
        }
        if !request.validate_for_start().is_empty() {
            return Err(StartError {
                code: "invalid-config",
            });
        }
        let deadline = request
            .stop_after
            .map(|seconds| Duration::from_secs(u64::from(seconds)));
        let mode = request.mode;
        let schedule: Box<dyn PressSchedule> = match request.mode {
            Mode::Timer => Box::new(TimerSchedule::new(
                &request.keys,
                Duration::from_millis(u64::from(request.timer.interval_ms)),
            )),
            Mode::Natural => Box::new(NaturalSchedule::new(
                &request.keys,
                NaturalSettings::from_config(&request.natural),
            )),
        };
        let sink = Arc::clone(&self.sink);
        let shared = Arc::clone(&self.shared);
        #[cfg(test)]
        let before_worker_exit = self.before_worker_exit.clone();
        let (sender, receiver) = mpsc::channel();
        let (launch_sender, launch_receiver) = mpsc::channel();
        let mut state = lock(&self.shared.state);
        if !worker_ready(&state) {
            return Ok(false);
        }
        match state.snapshot.status {
            RunStatus::Running | RunStatus::Stopping => return Ok(false),
            RunStatus::Failed => return Err(StartError { code: "run-failed" }),
            RunStatus::Idle => {}
        }
        if let Some(worker) = state.worker.take() {
            debug_assert!(worker.is_finished());
            let _ = worker.join();
        }
        let run_started = Instant::now();
        let clock = (self.clock_factory)(run_started);
        state.snapshot = RunSnapshot {
            status: RunStatus::Running,
            mode: Some(mode),
            elapsed_ms: 0,
            remaining_ms: deadline.map(duration_millis),
            successful_presses: 0,
            stop_reason: None,
            error: None,
        };
        state.started_at = Some(run_started);
        state.deadline = deadline;
        state.worker_complete = false;
        state.generation = state.generation.saturating_add(1);
        state.revision = 0;
        *lock(&self.control) = Some(sender);
        let running = next_event(&mut state);
        let spawn = thread::Builder::new()
            .name("aqlicker-input-run".to_owned())
            .spawn(move || {
                if launch_receiver.recv().is_ok() {
                    worker_main(sink, shared, receiver, schedule, deadline, clock);
                    #[cfg(test)]
                    if let Some(gate) = before_worker_exit {
                        gate.pause_once();
                    }
                }
            });
        let handle = match spawn {
            Ok(handle) => handle,
            Err(_) => {
                *lock(&self.control) = None;
                state.snapshot.status = RunStatus::Idle;
                state.snapshot.mode = None;
                state.started_at = None;
                state.deadline = None;
                state.worker_complete = true;
                let idle = next_event(&mut state);
                drop(state);
                observe(&self.shared, idle);
                return Err(StartError {
                    code: "worker-spawn-failed",
                });
            }
        };
        #[cfg(test)]
        if let Some(gate) = &self.before_worker_publish {
            gate.pause_once();
        }
        state.worker = Some(handle);
        drop(state);
        observe(&self.shared, running);
        let _ = launch_sender.send(());
        Ok(true)
    }

    pub fn stop(&self) -> bool {
        let (should_signal, event) = {
            let mut state = lock(&self.shared.state);
            match state.snapshot.status {
                RunStatus::Running => {
                    state.snapshot.status = RunStatus::Stopping;
                    (true, Some(next_event(&mut state)))
                }
                RunStatus::Idle | RunStatus::Stopping | RunStatus::Failed => (false, None),
            }
        };
        if let Some(event) = event {
            observe(&self.shared, event);
        }
        if should_signal {
            if let Some(sender) = lock(&self.control).as_ref() {
                let _ = sender.send(Control::Cancel);
            }
        }
        should_signal
    }

    pub fn snapshot(&self) -> RunSnapshot {
        let state = lock(&self.shared.state);
        let mut snapshot = state.snapshot.clone();
        if matches!(snapshot.status, RunStatus::Running | RunStatus::Stopping) {
            let elapsed = state
                .started_at
                .map_or(Duration::ZERO, |start| start.elapsed());
            snapshot.elapsed_ms = duration_millis(elapsed);
            snapshot.remaining_ms = state
                .deadline
                .map(|deadline| duration_millis(deadline.saturating_sub(elapsed)));
        }
        snapshot
    }

    pub fn wait_for_terminal(&self, timeout: Duration) -> Result<RunSnapshot, StartError> {
        self.wait_for_terminal_until(Instant::now() + timeout)
    }

    fn wait_for_terminal_until(&self, deadline: Instant) -> Result<RunSnapshot, StartError> {
        let mut state = lock(&self.shared.state);
        while matches!(
            state.snapshot.status,
            RunStatus::Running | RunStatus::Stopping
        ) || !state.worker_complete
        {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(StartError {
                    code: "wait-timeout",
                });
            }
            let (next, result) = self
                .shared
                .terminal
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
            if result.timed_out()
                && (matches!(
                    state.snapshot.status,
                    RunStatus::Running | RunStatus::Stopping
                ) || !state.worker_complete)
            {
                return Err(StartError {
                    code: "wait-timeout",
                });
            }
        }
        let snapshot = state.snapshot.clone();
        drop(state);
        loop {
            let finished = {
                let state = lock(&self.shared.state);
                state.worker.as_ref().is_none_or(JoinHandle::is_finished)
            };
            if finished {
                break;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(StartError {
                    code: "wait-timeout",
                });
            }
            thread::sleep(remaining.min(Duration::from_millis(1)));
        }
        Ok(snapshot)
    }

    pub fn shutdown(&self, timeout: Duration) -> Result<RunSnapshot, StartError> {
        let deadline = Instant::now() + timeout;
        self.stop();
        let snapshot = self.wait_for_terminal_until(deadline)?;
        self.reap_finished_worker();
        Ok(snapshot)
    }

    fn reap_finished_worker(&self) {
        let worker = {
            let mut state = lock(&self.shared.state);
            if state.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                state.worker.take()
            } else {
                None
            }
        };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for RunController {
    fn drop(&mut self) {
        self.stop();
        if let Some(sender) = lock(&self.control).as_ref() {
            let _ = sender.send(Control::Cancel);
        }
        let worker = lock(&self.shared.state).worker.take();
        if let Some(worker) = worker {
            let deadline = Instant::now() + Duration::from_millis(100);
            while !worker.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            if worker.is_finished() {
                let _ = worker.join();
            }
        }
    }
}

enum WorkerExit {
    Idle(StopReason),
    Failed {
        error: RunError,
        reason: StopReason,
        phase: FailurePhase,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailurePhase {
    Runtime,
    Cleanup,
}

struct PressState {
    down_key: Option<LogicalKey>,
    failure_phase: FailurePhase,
}

fn worker_main(
    sink: Arc<Mutex<Box<dyn InputSink>>>,
    shared: Arc<SharedState>,
    receiver: Receiver<Control>,
    mut schedule: Box<dyn PressSchedule>,
    deadline: Option<Duration>,
    clock: Arc<dyn Clock>,
) {
    let mut press_state = PressState {
        down_key: None,
        failure_phase: FailurePhase::Runtime,
    };
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        execute_schedule(
            &sink,
            &shared,
            &receiver,
            schedule.as_mut(),
            deadline,
            clock.as_ref(),
            &mut press_state,
        )
    }));
    let mut exit = match outcome {
        Ok(exit) => exit,
        Err(_) => WorkerExit::Failed {
            error: RunError {
                code: "worker-panic".to_owned(),
                key: press_state.down_key,
                message: "input worker panicked".to_owned(),
            },
            reason: StopReason::WorkerPanic,
            phase: press_state.failure_phase,
        },
    };

    if matches!(exit, WorkerExit::Idle(_)) {
        let mut state = lock(&shared.state);
        if state.snapshot.status == RunStatus::Running {
            state.snapshot.status = RunStatus::Stopping;
        }
    }
    if let Some(key) = press_state.down_key.take() {
        match panic::catch_unwind(AssertUnwindSafe(|| lock(&sink).key_up(key))) {
            Ok(Ok(())) => {}
            Ok(Err(failure)) => {
                exit = input_failure(key, failure, FailurePhase::Cleanup);
            }
            Err(_) => {
                exit = WorkerExit::Failed {
                    error: RunError {
                        code: "worker-panic".to_owned(),
                        key: Some(key),
                        message: "input worker panicked during key cleanup".to_owned(),
                    },
                    reason: StopReason::WorkerPanic,
                    phase: FailurePhase::Cleanup,
                };
            }
        }
    }
    finish(&shared, exit, clock.elapsed());
}

fn execute_schedule(
    sink: &Arc<Mutex<Box<dyn InputSink>>>,
    shared: &Arc<SharedState>,
    receiver: &Receiver<Control>,
    schedule: &mut dyn PressSchedule,
    deadline: Option<Duration>,
    clock: &dyn Clock,
    press_state: &mut PressState,
) -> WorkerExit {
    loop {
        press_state.failure_phase = FailurePhase::Runtime;
        let plan = schedule.next_press();
        if !is_before_deadline(&plan, deadline) {
            if deadline.is_some_and(|deadline| clock.wait_until(deadline, receiver)) {
                return WorkerExit::Idle(StopReason::Requested);
            }
            return WorkerExit::Idle(StopReason::DurationComplete);
        }
        if clock.wait_until(plan.target_offset, receiver) {
            return WorkerExit::Idle(StopReason::Requested);
        }
        match receiver.try_recv() {
            Ok(Control::Cancel) | Err(TryRecvError::Disconnected) => {
                return WorkerExit::Idle(StopReason::Requested);
            }
            Err(TryRecvError::Empty) => {}
        }
        if deadline.is_some_and(|deadline| clock.elapsed() >= deadline) {
            return WorkerExit::Idle(StopReason::DurationComplete);
        }

        if let Err(failure) = lock(sink).key_down(plan.key) {
            return input_failure(plan.key, failure, FailurePhase::Runtime);
        }
        press_state.down_key = Some(plan.key);
        if clock.wait_until(plan.target_offset.saturating_add(plan.hold_for), receiver) {
            return WorkerExit::Idle(StopReason::Requested);
        }
        press_state.failure_phase = FailurePhase::Cleanup;
        if let Err(failure) = lock(sink).key_up(plan.key) {
            return input_failure(plan.key, failure, FailurePhase::Cleanup);
        }
        press_state.down_key = None;
        press_state.failure_phase = FailurePhase::Runtime;
        let mut state = lock(&shared.state);
        state.snapshot.successful_presses = state.snapshot.successful_presses.saturating_add(1);
        state.snapshot.elapsed_ms = duration_millis(clock.elapsed());
        let event = next_event(&mut state);
        drop(state);
        observe(shared, event);
    }
}

fn input_failure(key: LogicalKey, failure: InputFailure, phase: FailurePhase) -> WorkerExit {
    WorkerExit::Failed {
        error: RunError {
            code: "input-failure".to_owned(),
            key: Some(key),
            message: failure.message,
        },
        reason: StopReason::InputFailure,
        phase,
    }
}

fn finish(shared: &SharedState, exit: WorkerExit, elapsed: Duration) {
    let mut state = lock(&shared.state);
    state.snapshot.elapsed_ms = duration_millis(elapsed);
    state.snapshot.remaining_ms = state
        .deadline
        .map(|deadline| duration_millis(deadline.saturating_sub(elapsed)));
    state.started_at = None;
    match exit {
        WorkerExit::Idle(reason) => {
            state.snapshot.status = RunStatus::Idle;
            state.snapshot.stop_reason = Some(reason);
        }
        WorkerExit::Failed {
            error,
            reason,
            phase,
        } => {
            state.snapshot.status =
                if state.snapshot.status == RunStatus::Stopping && phase == FailurePhase::Runtime {
                    RunStatus::Idle
                } else {
                    RunStatus::Failed
                };
            state.snapshot.stop_reason = Some(reason);
            state.snapshot.error = Some(error);
        }
    }
    let event = next_event(&mut state);
    drop(state);
    observe(shared, event);
    lock(&shared.state).worker_complete = true;
    shared.terminal.notify_all();
}

fn next_event(state: &mut RuntimeState) -> (u64, u64, RunSnapshot) {
    state.revision = state.revision.saturating_add(1);
    (state.generation, state.revision, state.snapshot.clone())
}

fn observe(shared: &SharedState, event: (u64, u64, RunSnapshot)) {
    let observer = Arc::clone(&lock(&shared.observer));
    observer(event.0, event.1, event.2);
}

fn worker_ready(state: &RuntimeState) -> bool {
    state.worker_complete && state.worker.as_ref().is_none_or(JoinHandle::is_finished)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc, Barrier, Condvar, Mutex,
            mpsc::{self, Receiver, TryRecvError},
        },
        thread,
        time::Duration,
    };

    use super::*;
    use crate::{AppConfig, KeyEntry, LogicalKey, Mode};

    #[derive(Clone)]
    struct BlockingSink {
        events: Arc<Mutex<Vec<String>>>,
        down_returned: Arc<(Mutex<bool>, Condvar)>,
        down_entered: Arc<(Mutex<bool>, Condvar)>,
    }

    impl InputSink for BlockingSink {
        fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("down:{key:?}"));
            let (lock, ready) = &*self.down_entered;
            *lock.lock().unwrap() = true;
            ready.notify_all();
            let (lock, ready) = &*self.down_returned;
            let mut returned = lock.lock().unwrap();
            while !*returned {
                returned = ready.wait(returned).unwrap();
            }
            Ok(())
        }

        fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("up:{key:?}"));
            Ok(())
        }
    }

    #[test]
    fn stop_after_key_down_releases_before_idle() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let sink = BlockingSink {
            events: Arc::clone(&events),
            down_returned: Arc::clone(&down_returned),
            down_entered: Arc::clone(&down_entered),
        };
        let controller = RunController::for_test(Box::new(sink));
        controller.start(test_request(Mode::Timer)).unwrap();

        wait_until_true(&down_entered);
        controller.stop();
        signal(&down_returned);
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA", "up:KeyA"]);
        assert_eq!(controller.snapshot().status, RunStatus::Idle);
        assert_eq!(
            controller.snapshot().stop_reason,
            Some(StopReason::Requested)
        );
    }

    #[test]
    fn repeated_start_and_stop_are_idempotent() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let controller = RunController::for_test(Box::new(BlockingSink {
            events,
            down_returned: Arc::clone(&down_returned),
            down_entered: Arc::clone(&down_entered),
        }));

        assert!(controller.start(test_request(Mode::Timer)).unwrap());
        assert!(!controller.start(test_request(Mode::Timer)).unwrap());
        wait_until_true(&down_entered);
        assert!(controller.stop());
        assert!(!controller.stop());
        signal(&down_returned);
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();
        assert!(!controller.stop());
    }

    #[test]
    fn concurrent_starts_create_exactly_one_run() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let controller = Arc::new(RunController::for_test(Box::new(BlockingSink {
            events,
            down_returned: Arc::clone(&down_returned),
            down_entered: Arc::clone(&down_entered),
        })));
        let barrier = Arc::new(Barrier::new(3));
        let starters: Vec<_> = (0..2)
            .map(|_| {
                let controller = Arc::clone(&controller);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    controller.start(test_request(Mode::Natural)).unwrap()
                })
            })
            .collect();
        barrier.wait();
        let starts = starters
            .into_iter()
            .map(|starter| starter.join().unwrap())
            .filter(|started| *started)
            .count();

        assert_eq!(starts, 1);
        wait_until_true(&down_entered);
        controller.stop();
        signal(&down_returned);
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();
    }

    #[test]
    fn shutdown_timeout_includes_terminal_observer_completion() {
        let observer_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let observer_released = Arc::new((Mutex::new(false), Condvar::new()));
        let mut controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(VirtualClock::new()),
        );
        let entered = Arc::clone(&observer_entered);
        let released = Arc::clone(&observer_released);
        controller.set_observer(Arc::new(move |snapshot| {
            if matches!(snapshot.status, RunStatus::Idle | RunStatus::Failed) {
                signal(&entered);
                let (lock, ready) = &*released;
                let released = lock.lock().unwrap();
                let _released = ready.wait_while(released, |released| !*released).unwrap();
            }
        }));
        let controller = Arc::new(controller);
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&observer_entered);

        let (result_sender, result_receiver) = mpsc::channel();
        let shutdown_controller = Arc::clone(&controller);
        let shutdown = thread::spawn(move || {
            let result = shutdown_controller.shutdown(Duration::from_millis(20));
            result_sender.send(result).unwrap();
        });
        let bounded = result_receiver.recv_timeout(Duration::from_millis(200));

        signal(&observer_released);
        let eventual = match bounded {
            Ok(result) => result,
            Err(_) => result_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
        };
        shutdown.join().unwrap();

        assert_eq!(
            eventual.unwrap_err(),
            StartError {
                code: "wait-timeout"
            }
        );
        controller.shutdown(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn shutdown_timeout_includes_actual_worker_thread_exit() {
        let exit_gate = Arc::new(WorkerPublishGate::new());
        let mut controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(VirtualClock::new()),
        );
        controller.before_worker_exit = Some(Arc::clone(&exit_gate));
        let controller = Arc::new(controller);
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&exit_gate.entered);

        let (result_sender, result_receiver) = mpsc::channel();
        let shutdown_controller = Arc::clone(&controller);
        let shutdown = thread::spawn(move || {
            let result = shutdown_controller.shutdown(Duration::from_millis(20));
            result_sender.send(result).unwrap();
        });
        let bounded = result_receiver.recv_timeout(Duration::from_millis(200));

        signal(&exit_gate.released);
        let eventual = match bounded {
            Ok(result) => result,
            Err(_) => result_receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
        };
        shutdown.join().unwrap();

        assert_eq!(eventual.unwrap_err().code, "wait-timeout");
        controller.shutdown(Duration::from_secs(1)).unwrap();
    }

    #[test]
    fn fast_terminal_run_cannot_accept_a_second_start_before_worker_publication() {
        let publish_gate = Arc::new(WorkerPublishGate::new());
        let observer_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let observer_released = Arc::new((Mutex::new(false), Condvar::new()));
        let mut controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(VirtualClock::new()),
        );
        controller.before_worker_publish = Some(Arc::clone(&publish_gate));
        let entered = Arc::clone(&observer_entered);
        let released = Arc::clone(&observer_released);
        controller.set_observer(Arc::new(move |snapshot| {
            if matches!(snapshot.status, RunStatus::Idle | RunStatus::Failed) {
                signal(&entered);
                let (lock, ready) = &*released;
                let released = lock.lock().unwrap();
                let _released = ready.wait_while(released, |released| !*released).unwrap();
            }
        }));
        let controller = Arc::new(controller);

        let first_controller = Arc::clone(&controller);
        let first = thread::spawn(move || first_controller.start(test_request(Mode::Timer)));
        wait_until_true(&publish_gate.entered);
        let terminal_before_publication =
            wait_until_true_for(&observer_entered, Duration::from_millis(200));
        let second_controller = Arc::clone(&controller);
        let second = thread::spawn(move || second_controller.start(test_request(Mode::Timer)));

        signal(&publish_gate.released);
        signal(&observer_released);
        let first_started = first.join().unwrap().unwrap();
        let second_started = second.join().unwrap().unwrap();
        controller.shutdown(Duration::from_secs(1)).unwrap();

        assert!(
            !terminal_before_publication,
            "worker reached a terminal observer before its handle was published"
        );
        assert_eq!(usize::from(first_started) + usize::from(second_started), 1);
    }

    #[derive(Clone)]
    struct VirtualClock {
        elapsed: Arc<Mutex<Duration>>,
        panic_after_waits: Option<usize>,
        waits: Arc<Mutex<usize>>,
    }

    impl VirtualClock {
        fn new() -> Self {
            Self {
                elapsed: Arc::new(Mutex::new(Duration::ZERO)),
                panic_after_waits: None,
                waits: Arc::new(Mutex::new(0)),
            }
        }

        fn panic_after_waits(count: usize) -> Self {
            Self {
                panic_after_waits: Some(count),
                ..Self::new()
            }
        }
    }

    impl Clock for VirtualClock {
        fn elapsed(&self) -> Duration {
            *self.elapsed.lock().unwrap()
        }

        fn wait_until(&self, target: Duration, receiver: &Receiver<Control>) -> bool {
            match receiver.try_recv() {
                Ok(Control::Cancel) | Err(TryRecvError::Disconnected) => return true,
                Err(TryRecvError::Empty) => {}
            }
            let mut waits = self.waits.lock().unwrap();
            *waits += 1;
            if self.panic_after_waits == Some(*waits) {
                panic!("virtual clock panic");
            }
            let mut elapsed = self.elapsed.lock().unwrap();
            *elapsed = (*elapsed).max(target);
            false
        }
    }

    #[test]
    fn observer_receives_running_progress_and_terminal_snapshots() {
        let observed = Arc::new(Mutex::new(Vec::new()));
        let mut controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(VirtualClock::new()),
        );
        let captured = Arc::clone(&observed);
        controller.set_observer(Arc::new(move |snapshot| {
            captured.lock().unwrap().push(snapshot);
        }));

        controller.start(test_request(Mode::Timer)).unwrap();
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        let observed = observed.lock().unwrap();
        assert_eq!(observed.first().unwrap().status, RunStatus::Running);
        assert!(
            observed
                .iter()
                .any(|snapshot| snapshot.successful_presses > 0)
        );
        assert_eq!(observed.last().unwrap().status, RunStatus::Idle);
    }

    struct LateWakeClock {
        elapsed: Mutex<Duration>,
    }

    impl LateWakeClock {
        fn new() -> Self {
            Self {
                elapsed: Mutex::new(Duration::ZERO),
            }
        }
    }

    impl Clock for LateWakeClock {
        fn elapsed(&self) -> Duration {
            *self.elapsed.lock().unwrap()
        }

        fn wait_until(&self, _target: Duration, _receiver: &Receiver<Control>) -> bool {
            *self.elapsed.lock().unwrap() = Duration::from_secs(1);
            false
        }
    }

    #[test]
    fn late_wake_at_deadline_does_not_start_a_press() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::clone(&events),
            }),
            Arc::new(LateWakeClock::new()),
        );

        controller.start(test_request(Mode::Timer)).unwrap();
        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert!(events.lock().unwrap().is_empty());
        assert_eq!(snapshot.stop_reason, Some(StopReason::DurationComplete));
        assert_eq!(snapshot.elapsed_ms, 1_000);
    }

    struct AbsoluteClock {
        now: Arc<Mutex<Duration>>,
        origin: Duration,
    }

    impl Clock for AbsoluteClock {
        fn elapsed(&self) -> Duration {
            self.now.lock().unwrap().saturating_sub(self.origin)
        }

        fn wait_until(&self, target: Duration, receiver: &Receiver<Control>) -> bool {
            match receiver.try_recv() {
                Ok(Control::Cancel) | Err(TryRecvError::Disconnected) => return true,
                Err(TryRecvError::Empty) => {}
            }
            let mut now = self.now.lock().unwrap();
            *now = (*now).max(self.origin.saturating_add(target));
            false
        }
    }

    struct OriginBlockingSink {
        now: Arc<Mutex<Duration>>,
        origin: Arc<Mutex<Option<Duration>>>,
        first_down_elapsed: Arc<Mutex<Option<Duration>>>,
        down_entered: Arc<(Mutex<bool>, Condvar)>,
        down_returned: Arc<(Mutex<bool>, Condvar)>,
    }

    impl InputSink for OriginBlockingSink {
        fn key_down(&mut self, _key: LogicalKey) -> Result<(), InputFailure> {
            let origin = self
                .origin
                .lock()
                .unwrap()
                .expect("clock origin must exist");
            let now = *self.now.lock().unwrap();
            let is_first_down = {
                let mut first_down_elapsed = self.first_down_elapsed.lock().unwrap();
                let is_first_down = first_down_elapsed.is_none();
                if is_first_down {
                    *first_down_elapsed = Some(now.saturating_sub(origin));
                }
                is_first_down
            };
            if is_first_down {
                let (lock, ready) = &*self.down_entered;
                *lock.lock().unwrap() = true;
                ready.notify_all();
                let (lock, ready) = &*self.down_returned;
                let mut returned = lock.lock().unwrap();
                while !*returned {
                    returned = ready.wait(returned).unwrap();
                }
            }
            Ok(())
        }

        fn key_up(&mut self, _key: LogicalKey) -> Result<(), InputFailure> {
            Ok(())
        }
    }

    #[test]
    fn finished_prior_worker_reap_does_not_consume_the_new_run_duration() {
        let now = Arc::new(Mutex::new(Duration::ZERO));
        let origin = Arc::new(Mutex::new(None));
        let first_down_elapsed = Arc::new(Mutex::new(None));
        let clock_run_started = Arc::new(Mutex::new(None));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let mut controller = RunController::for_test(Box::new(OriginBlockingSink {
            now: Arc::clone(&now),
            origin: Arc::clone(&origin),
            first_down_elapsed: Arc::clone(&first_down_elapsed),
            down_entered: Arc::clone(&down_entered),
            down_returned: Arc::clone(&down_returned),
        }));
        let clock_now = Arc::clone(&now);
        let clock_origin = Arc::clone(&origin);
        let recorded_run_started = Arc::clone(&clock_run_started);
        controller.clock_factory = Arc::new(move |run_started| {
            let origin = *clock_now.lock().unwrap();
            *clock_origin.lock().unwrap() = Some(origin);
            *recorded_run_started.lock().unwrap() = Some(run_started);
            Arc::new(AbsoluteClock {
                now: Arc::clone(&clock_now),
                origin,
            })
        });
        let prior_now = Arc::clone(&now);
        lock(&controller.shared.state).worker = Some(thread::spawn(move || {
            *prior_now.lock().unwrap() = Duration::from_millis(500);
        }));
        while !lock(&controller.shared.state)
            .worker
            .as_ref()
            .unwrap()
            .is_finished()
        {
            thread::yield_now();
        }

        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);
        let published = controller.snapshot();
        let published_run_started = lock(&controller.shared.state).started_at;
        let clock_run_started = *clock_run_started.lock().unwrap();
        let first_down_elapsed = first_down_elapsed
            .lock()
            .unwrap()
            .expect("first key-down must be observed");
        signal(&down_returned);
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(first_down_elapsed, Duration::ZERO);
        assert_eq!(published_run_started, clock_run_started);
        assert_eq!(published.status, RunStatus::Running);
        assert!(published.remaining_ms.is_some());
    }

    struct TimedSink {
        clock: Arc<VirtualClock>,
        press_targets: Arc<Mutex<Vec<Duration>>>,
    }

    impl InputSink for TimedSink {
        fn key_down(&mut self, _key: LogicalKey) -> Result<(), InputFailure> {
            self.press_targets
                .lock()
                .unwrap()
                .push(self.clock.elapsed());
            Ok(())
        }

        fn key_up(&mut self, _key: LogicalKey) -> Result<(), InputFailure> {
            Ok(())
        }
    }

    #[test]
    fn deadline_is_shared_by_natural_and_timer_runs() {
        for mode in [Mode::Timer, Mode::Natural] {
            let clock = Arc::new(VirtualClock::new());
            let press_targets = Arc::new(Mutex::new(Vec::new()));
            let controller = RunController::for_test_with_clock(
                Box::new(TimedSink {
                    clock: Arc::clone(&clock),
                    press_targets: Arc::clone(&press_targets),
                }),
                clock,
            );

            controller.start(test_request(mode)).unwrap();
            let snapshot = controller
                .wait_for_terminal(Duration::from_secs(1))
                .unwrap();

            let press_targets = press_targets.lock().unwrap();
            assert!(!press_targets.is_empty(), "{mode:?} emitted no presses");
            assert!(
                press_targets
                    .iter()
                    .all(|target| *target < Duration::from_secs(1)),
                "{mode:?} started a press at or after its deadline: {press_targets:?}"
            );
            assert_eq!(snapshot.stop_reason, Some(StopReason::DurationComplete));
            assert_eq!(snapshot.status, RunStatus::Idle);
            assert!(snapshot.elapsed_ms >= 1_000);
            assert_eq!(snapshot.remaining_ms, Some(0));
        }
    }

    enum FailurePoint {
        Down,
        FirstUp,
    }

    #[derive(Clone, Copy)]
    enum StopRaceOutcome {
        KeyDownFailure,
        CleanupSuccess,
        CleanupFailure,
    }

    struct StopRaceSink {
        outcome: StopRaceOutcome,
        events: Arc<Mutex<Vec<String>>>,
        down_entered: Arc<(Mutex<bool>, Condvar)>,
        down_returned: Arc<(Mutex<bool>, Condvar)>,
    }

    impl InputSink for StopRaceSink {
        fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("down:{key:?}"));
            let (lock, ready) = &*self.down_entered;
            *lock.lock().unwrap() = true;
            ready.notify_all();
            let (lock, ready) = &*self.down_returned;
            let mut returned = lock.lock().unwrap();
            while !*returned {
                returned = ready.wait(returned).unwrap();
            }
            match self.outcome {
                StopRaceOutcome::KeyDownFailure => Err(InputFailure::new("key-down rejected")),
                StopRaceOutcome::CleanupSuccess | StopRaceOutcome::CleanupFailure => Ok(()),
            }
        }

        fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("up:{key:?}"));
            match self.outcome {
                StopRaceOutcome::CleanupFailure => Err(InputFailure::new("cleanup rejected")),
                StopRaceOutcome::KeyDownFailure | StopRaceOutcome::CleanupSuccess => Ok(()),
            }
        }
    }

    struct PanicAfterKeyDownClock {
        waits: Mutex<usize>,
    }

    impl PanicAfterKeyDownClock {
        fn new() -> Self {
            Self {
                waits: Mutex::new(0),
            }
        }
    }

    impl Clock for PanicAfterKeyDownClock {
        fn elapsed(&self) -> Duration {
            Duration::ZERO
        }

        fn wait_until(&self, _target: Duration, _receiver: &Receiver<Control>) -> bool {
            let mut waits = self.waits.lock().unwrap();
            *waits += 1;
            if *waits == 2 {
                panic!("hold-wait panic");
            }
            false
        }
    }

    #[test]
    fn stop_racing_with_key_down_failure_returns_idle_with_visible_error() {
        let (controller, events, down_entered, down_returned) =
            stop_race(StopRaceOutcome::KeyDownFailure);
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);

        assert!(controller.stop());
        signal(&down_returned);
        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Idle);
        assert_eq!(snapshot.stop_reason, Some(StopReason::InputFailure));
        assert_eq!(
            snapshot.error,
            Some(RunError {
                code: "input-failure".to_owned(),
                key: Some(LogicalKey::KeyA),
                message: "key-down rejected".to_owned(),
            })
        );
        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA"]);
    }

    #[test]
    fn stop_racing_with_non_cleanup_panic_returns_idle_with_visible_error() {
        let (controller, events, down_entered, down_returned) = stop_race_with_clock(
            StopRaceOutcome::CleanupSuccess,
            Arc::new(PanicAfterKeyDownClock::new()),
        );
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);

        assert!(controller.stop());
        signal(&down_returned);
        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Idle);
        assert_eq!(snapshot.stop_reason, Some(StopReason::WorkerPanic));
        assert_eq!(snapshot.error.as_ref().unwrap().code, "worker-panic");
        assert_eq!(snapshot.error.as_ref().unwrap().key, Some(LogicalKey::KeyA));
        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA", "up:KeyA"]);
    }

    #[test]
    fn stop_racing_with_cleanup_failure_remains_failed() {
        let (controller, events, down_entered, down_returned) =
            stop_race(StopRaceOutcome::CleanupFailure);
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);

        assert!(controller.stop());
        signal(&down_returned);
        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Failed);
        assert_eq!(snapshot.stop_reason, Some(StopReason::InputFailure));
        assert_eq!(
            snapshot.error,
            Some(RunError {
                code: "input-failure".to_owned(),
                key: Some(LogicalKey::KeyA),
                message: "cleanup rejected".to_owned(),
            })
        );
        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA", "up:KeyA"]);
    }

    #[test]
    fn terminal_revision_dominates_a_delayed_stopping_publication() {
        let (mut controller, _sink_events, down_entered, down_returned) =
            stop_race(StopRaceOutcome::KeyDownFailure);
        let stopping_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let stopping_released = Arc::new((Mutex::new(false), Condvar::new()));
        let terminal_seen = Arc::new((Mutex::new(false), Condvar::new()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let entered = Arc::clone(&stopping_entered);
        let released = Arc::clone(&stopping_released);
        let terminal = Arc::clone(&terminal_seen);
        let events = Arc::clone(&observed);
        controller.set_tagged_observer(Arc::new(move |generation, revision, snapshot| {
            if snapshot.status == RunStatus::Stopping {
                signal(&entered);
                let (lock, ready) = &*released;
                let released = lock.lock().unwrap();
                let _released = ready.wait_while(released, |released| !*released).unwrap();
            }
            events
                .lock()
                .unwrap()
                .push((generation, revision, snapshot.status));
            if matches!(snapshot.status, RunStatus::Idle | RunStatus::Failed) {
                signal(&terminal);
            }
        }));
        let controller = Arc::new(controller);
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);

        let stopping_controller = Arc::clone(&controller);
        let stopping = thread::spawn(move || stopping_controller.stop());
        wait_until_true(&stopping_entered);
        signal(&down_returned);
        wait_until_true(&terminal_seen);
        signal(&stopping_released);
        assert!(stopping.join().unwrap());
        controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        let observed = observed.lock().unwrap();
        let terminal = observed
            .iter()
            .find(|(_, _, status)| matches!(status, RunStatus::Idle | RunStatus::Failed))
            .unwrap();
        let stopping = observed
            .iter()
            .find(|(_, _, status)| *status == RunStatus::Stopping)
            .unwrap();
        assert_eq!(terminal.0, stopping.0);
        assert!(terminal.1 > stopping.1);
        assert!(
            observed.iter().position(|event| event == terminal)
                < observed.iter().position(|event| event == stopping)
        );
    }

    type StopRace = (
        RunController,
        Arc<Mutex<Vec<String>>>,
        Arc<(Mutex<bool>, Condvar)>,
        Arc<(Mutex<bool>, Condvar)>,
    );

    fn stop_race(outcome: StopRaceOutcome) -> StopRace {
        stop_race_with_clock(outcome, Arc::new(VirtualClock::new()))
    }

    fn stop_race_with_clock<C>(outcome: StopRaceOutcome, clock: Arc<C>) -> StopRace
    where
        C: Clock + 'static,
    {
        let events = Arc::new(Mutex::new(Vec::new()));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let controller = RunController::for_test_with_clock(
            Box::new(StopRaceSink {
                outcome,
                events: Arc::clone(&events),
                down_entered: Arc::clone(&down_entered),
                down_returned: Arc::clone(&down_returned),
            }),
            clock,
        );
        (controller, events, down_entered, down_returned)
    }

    struct FailingSink {
        events: Arc<Mutex<Vec<String>>>,
        point: FailurePoint,
        up_attempts: usize,
    }

    impl InputSink for FailingSink {
        fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("down:{key:?}"));
            if matches!(self.point, FailurePoint::Down) {
                Err(InputFailure::new("down rejected"))
            } else {
                Ok(())
            }
        }

        fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("up:{key:?}"));
            self.up_attempts += 1;
            if matches!(self.point, FailurePoint::FirstUp) && self.up_attempts == 1 {
                Err(InputFailure::new("up rejected once"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn input_failure_names_the_logical_key_and_stops_the_run() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let controller = RunController::for_test_with_clock(
            Box::new(FailingSink {
                events: Arc::clone(&events),
                point: FailurePoint::Down,
                up_attempts: 0,
            }),
            Arc::new(VirtualClock::new()),
        );
        controller.start(test_request(Mode::Timer)).unwrap();

        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Failed);
        assert_eq!(snapshot.stop_reason, Some(StopReason::InputFailure));
        assert_eq!(snapshot.error.unwrap().key, Some(LogicalKey::KeyA));
        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA"]);
    }

    #[test]
    fn failed_key_up_is_retried_during_terminal_cleanup() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let controller = RunController::for_test_with_clock(
            Box::new(FailingSink {
                events: Arc::clone(&events),
                point: FailurePoint::FirstUp,
                up_attempts: 0,
            }),
            Arc::new(VirtualClock::new()),
        );
        controller.start(test_request(Mode::Timer)).unwrap();

        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Failed);
        assert_eq!(snapshot.successful_presses, 0);
        assert_eq!(snapshot.error.unwrap().key, Some(LogicalKey::KeyA));
        assert_eq!(
            *events.lock().unwrap(),
            vec!["down:KeyA", "up:KeyA", "up:KeyA"]
        );
    }

    #[test]
    fn worker_panic_releases_a_successful_key_down() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let clock = Arc::new(VirtualClock::panic_after_waits(2));
        let controller = RunController::for_test_with_clock(
            Box::new(TimedRecordingSink {
                events: Arc::clone(&events),
            }),
            clock,
        );
        controller.start(test_request(Mode::Timer)).unwrap();

        let snapshot = controller
            .wait_for_terminal(Duration::from_secs(1))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Failed);
        assert_eq!(snapshot.error.unwrap().code, "worker-panic");
        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA", "up:KeyA"]);
    }

    struct PanickingUpSink {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl InputSink for PanickingUpSink {
        fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("down:{key:?}"));
            Ok(())
        }

        fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("up:{key:?}"));
            panic!("key-up panic");
        }
    }

    #[test]
    fn panic_during_cleanup_still_reaches_failed_terminal_state() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let controller = RunController::for_test_with_clock(
            Box::new(PanickingUpSink {
                events: Arc::clone(&events),
            }),
            Arc::new(VirtualClock::new()),
        );
        controller.start(test_request(Mode::Timer)).unwrap();

        let snapshot = controller
            .wait_for_terminal(Duration::from_millis(100))
            .unwrap();

        assert_eq!(snapshot.status, RunStatus::Failed);
        assert_eq!(snapshot.error.unwrap().code, "worker-panic");
        assert_eq!(
            *events.lock().unwrap(),
            vec!["down:KeyA", "up:KeyA", "up:KeyA"]
        );
    }

    struct TimedRecordingSink {
        events: Arc<Mutex<Vec<String>>>,
    }

    impl InputSink for TimedRecordingSink {
        fn key_down(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("down:{key:?}"));
            Ok(())
        }

        fn key_up(&mut self, key: LogicalKey) -> Result<(), InputFailure> {
            self.events.lock().unwrap().push(format!("up:{key:?}"));
            Ok(())
        }
    }

    #[test]
    fn dropping_a_running_controller_cancels_and_releases_before_returning() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let down_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let down_returned = Arc::new((Mutex::new(false), Condvar::new()));
        let controller = RunController::for_test(Box::new(BlockingSink {
            events: Arc::clone(&events),
            down_returned: Arc::clone(&down_returned),
            down_entered: Arc::clone(&down_entered),
        }));
        controller.start(test_request(Mode::Timer)).unwrap();
        wait_until_true(&down_entered);

        let dropper = thread::spawn(move || drop(controller));
        signal(&down_returned);
        dropper.join().unwrap();

        assert_eq!(*events.lock().unwrap(), vec!["down:KeyA", "up:KeyA"]);
    }

    fn test_request(mode: Mode) -> AppConfig {
        AppConfig {
            keys: vec![KeyEntry::new(LogicalKey::KeyA)],
            mode,
            stop_after: Some(1),
            ..AppConfig::default()
        }
    }

    fn wait_until_true(pair: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, ready) = &**pair;
        let entered = lock.lock().unwrap();
        let (entered, result) = ready
            .wait_timeout_while(entered, Duration::from_secs(1), |entered| !*entered)
            .unwrap();
        assert!(!result.timed_out() && *entered);
    }

    fn wait_until_true_for(pair: &Arc<(Mutex<bool>, Condvar)>, timeout: Duration) -> bool {
        let (lock, ready) = &**pair;
        let entered = lock.lock().unwrap();
        let (entered, result) = ready
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .unwrap();
        !result.timed_out() && *entered
    }

    fn signal(pair: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, ready) = &**pair;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
}
