use std::{
    panic::{self, AssertUnwindSafe},
    sync::{
        Arc, Condvar, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

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

#[derive(Debug, Clone, Copy)]
enum Control {
    Cancel,
}

struct RuntimeState {
    snapshot: RunSnapshot,
    started_at: Option<Instant>,
    deadline: Option<Duration>,
}

struct SharedState {
    state: Mutex<RuntimeState>,
    terminal: Condvar,
}

trait Clock: Send + Sync {
    fn elapsed(&self) -> Duration;
    fn wait_until(&self, target: Duration, receiver: &Receiver<Control>) -> bool;
}

struct RealClock {
    started: Instant,
}

impl RealClock {
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
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
    clock_factory: Arc<dyn Fn() -> Arc<dyn Clock> + Send + Sync>,
    shared: Arc<SharedState>,
    control: Mutex<Option<Sender<Control>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl RunController {
    pub fn new() -> Result<Self, InputFailure> {
        Ok(Self::with_sink(Box::new(EnigoInputSink::new()?)))
    }

    pub fn with_sink(sink: Box<dyn InputSink>) -> Self {
        Self {
            sink: Arc::new(Mutex::new(sink)),
            clock_factory: Arc::new(|| Arc::new(RealClock::new())),
            shared: Arc::new(SharedState {
                state: Mutex::new(RuntimeState {
                    snapshot: RunSnapshot {
                        status: RunStatus::Idle,
                        mode: None,
                        elapsed_ms: 0,
                        remaining_ms: None,
                        successful_presses: 0,
                        stop_reason: None,
                        error: None,
                    },
                    started_at: None,
                    deadline: None,
                }),
                terminal: Condvar::new(),
            }),
            control: Mutex::new(None),
            worker: Mutex::new(None),
        }
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
        controller.clock_factory = Arc::new(move || clock.clone());
        controller
    }

    pub fn start(&self, request: AppConfig) -> Result<bool, StartError> {
        {
            let state = lock(&self.shared.state);
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
        let clock = (self.clock_factory)();
        let (sender, receiver) = mpsc::channel();
        let mut state = lock(&self.shared.state);
        match state.snapshot.status {
            RunStatus::Running | RunStatus::Stopping => return Ok(false),
            RunStatus::Failed => return Err(StartError { code: "run-failed" }),
            RunStatus::Idle => {}
        }
        self.reap_worker();
        state.snapshot = RunSnapshot {
            status: RunStatus::Running,
            mode: Some(mode),
            elapsed_ms: 0,
            remaining_ms: deadline.map(duration_millis),
            successful_presses: 0,
            stop_reason: None,
            error: None,
        };
        state.started_at = Some(Instant::now());
        state.deadline = deadline;
        *lock(&self.control) = Some(sender);
        let spawn = thread::Builder::new()
            .name("aqlicker-input-run".to_owned())
            .spawn(move || worker_main(sink, shared, receiver, schedule, deadline, clock));
        let handle = match spawn {
            Ok(handle) => handle,
            Err(_) => {
                *lock(&self.control) = None;
                state.snapshot.status = RunStatus::Idle;
                state.snapshot.mode = None;
                state.started_at = None;
                return Err(StartError {
                    code: "worker-spawn-failed",
                });
            }
        };
        *lock(&self.worker) = Some(handle);
        drop(state);
        Ok(true)
    }

    pub fn stop(&self) -> bool {
        let should_signal = {
            let mut state = lock(&self.shared.state);
            match state.snapshot.status {
                RunStatus::Running => {
                    state.snapshot.status = RunStatus::Stopping;
                    true
                }
                RunStatus::Idle | RunStatus::Stopping | RunStatus::Failed => false,
            }
        };
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
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.shared.state);
        while matches!(
            state.snapshot.status,
            RunStatus::Running | RunStatus::Stopping
        ) {
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
                && matches!(
                    state.snapshot.status,
                    RunStatus::Running | RunStatus::Stopping
                )
            {
                return Err(StartError {
                    code: "wait-timeout",
                });
            }
        }
        let snapshot = state.snapshot.clone();
        drop(state);
        Ok(snapshot)
    }

    fn reap_worker(&self) {
        if let Some(worker) = lock(&self.worker).take() {
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
        self.reap_worker();
    }
}

enum WorkerExit {
    Idle(StopReason),
    Failed(RunError, StopReason),
}

fn worker_main(
    sink: Arc<Mutex<Box<dyn InputSink>>>,
    shared: Arc<SharedState>,
    receiver: Receiver<Control>,
    mut schedule: Box<dyn PressSchedule>,
    deadline: Option<Duration>,
    clock: Arc<dyn Clock>,
) {
    let mut down_key = None;
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        execute_schedule(
            &sink,
            &shared,
            &receiver,
            schedule.as_mut(),
            deadline,
            clock.as_ref(),
            &mut down_key,
        )
    }));
    let mut exit = match outcome {
        Ok(exit) => exit,
        Err(_) => WorkerExit::Failed(
            RunError {
                code: "worker-panic".to_owned(),
                key: down_key,
                message: "input worker panicked".to_owned(),
            },
            StopReason::WorkerPanic,
        ),
    };

    if matches!(exit, WorkerExit::Idle(_)) {
        let mut state = lock(&shared.state);
        if state.snapshot.status == RunStatus::Running {
            state.snapshot.status = RunStatus::Stopping;
        }
    }
    if let Some(key) = down_key.take() {
        match panic::catch_unwind(AssertUnwindSafe(|| lock(&sink).key_up(key))) {
            Ok(Ok(())) => {}
            Ok(Err(failure)) => exit = input_failure(key, failure),
            Err(_) => {
                exit = WorkerExit::Failed(
                    RunError {
                        code: "worker-panic".to_owned(),
                        key: Some(key),
                        message: "input worker panicked during key cleanup".to_owned(),
                    },
                    StopReason::WorkerPanic,
                );
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
    down_key: &mut Option<LogicalKey>,
) -> WorkerExit {
    loop {
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

        if let Err(failure) = lock(sink).key_down(plan.key) {
            return input_failure(plan.key, failure);
        }
        *down_key = Some(plan.key);
        if clock.wait_until(plan.target_offset.saturating_add(plan.hold_for), receiver) {
            return WorkerExit::Idle(StopReason::Requested);
        }
        if let Err(failure) = lock(sink).key_up(plan.key) {
            return input_failure(plan.key, failure);
        }
        *down_key = None;
        let mut state = lock(&shared.state);
        state.snapshot.successful_presses = state.snapshot.successful_presses.saturating_add(1);
        state.snapshot.elapsed_ms = duration_millis(clock.elapsed());
    }
}

fn input_failure(key: LogicalKey, failure: InputFailure) -> WorkerExit {
    WorkerExit::Failed(
        RunError {
            code: "input-failure".to_owned(),
            key: Some(key),
            message: failure.message,
        },
        StopReason::InputFailure,
    )
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
        WorkerExit::Failed(error, reason) => {
            state.snapshot.status = RunStatus::Failed;
            state.snapshot.stop_reason = Some(reason);
            state.snapshot.error = Some(error);
        }
    }
    shared.terminal.notify_all();
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
            mpsc::{Receiver, TryRecvError},
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
            assert_eq!(snapshot.elapsed_ms, 1_000);
            assert_eq!(snapshot.remaining_ms, Some(0));
        }
    }

    enum FailurePoint {
        Down,
        FirstUp,
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

    fn signal(pair: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, ready) = &**pair;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
}
