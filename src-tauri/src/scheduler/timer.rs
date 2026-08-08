use std::time::Duration;

use crate::{KeyEntry, LogicalKey};

use super::{PressPlan, PressSchedule};

pub struct TimerSchedule {
    keys: Vec<LogicalKey>,
    interval: Duration,
    key_index: usize,
    press_count: u64,
}

impl TimerSchedule {
    pub fn new(keys: &[KeyEntry], interval: Duration) -> Self {
        assert!(
            !keys.is_empty(),
            "a timer schedule requires at least one key"
        );
        Self {
            keys: keys.iter().map(|entry| entry.key).collect(),
            interval,
            key_index: 0,
            press_count: 0,
        }
    }
}

impl PressSchedule for TimerSchedule {
    fn next_press(&mut self) -> PressPlan {
        let count = self.press_count;
        self.press_count = self.press_count.saturating_add(1);
        let key = self.keys[self.key_index];
        self.key_index = (self.key_index + 1) % self.keys.len();
        PressPlan {
            key,
            target_offset: duration_times(self.interval, count),
            hold_for: Duration::from_millis(30),
        }
    }
}

fn duration_times(interval: Duration, count: u64) -> Duration {
    let nanoseconds = interval.as_nanos().saturating_mul(u128::from(count));
    let seconds = nanoseconds / 1_000_000_000;
    if seconds > u128::from(u64::MAX) {
        Duration::MAX
    } else {
        Duration::new(seconds as u64, (nanoseconds % 1_000_000_000) as u32)
    }
}
