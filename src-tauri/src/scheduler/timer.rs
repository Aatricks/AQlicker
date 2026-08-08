use std::time::Duration;

use crate::{KeyEntry, LogicalKey};

use super::{PressPlan, PressSchedule};

pub struct TimerSchedule {
    keys: Vec<LogicalKey>,
    interval: Duration,
    key_index: usize,
    next_at: Duration,
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
            next_at: Duration::ZERO,
        }
    }
}

impl PressSchedule for TimerSchedule {
    fn next_press(&mut self, now: Duration) -> PressPlan {
        // The planned timeline never runs behind reality: whatever held the run
        // up moves it forward instead of being replayed as a catch-up burst.
        self.next_at = self.next_at.max(now);
        let key = self.keys[self.key_index];
        self.key_index = (self.key_index + 1) % self.keys.len();
        let plan = PressPlan {
            key,
            target_offset: self.next_at,
            hold_for: Duration::from_millis(30),
        };
        self.next_at = self.next_at.saturating_add(self.interval);
        plan
    }

    fn record_press(&mut self, at: Duration) {
        // The accumulated plan is what sets the pace, so a press that ran a
        // little late does not push the next one out and the interval never
        // drifts. It is rebased only once the plan has fallen behind the press
        // that really happened -- a pause leaves it stale that way -- and then
        // the next press is a whole interval after that press.
        if self.next_at < at {
            self.next_at = at.saturating_add(self.interval);
        }
    }
}
