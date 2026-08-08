mod natural;
mod timer;

use std::time::Duration;

use crate::LogicalKey;

pub use natural::{NaturalSchedule, NaturalSettings};
pub use timer::TimerSchedule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PressPlan {
    pub key: LogicalKey,
    pub target_offset: Duration,
    pub hold_for: Duration,
}

impl PressPlan {
    pub fn new(key: LogicalKey, target_ms: u64, hold_ms: u64) -> Self {
        Self {
            key,
            target_offset: Duration::from_millis(target_ms),
            hold_for: Duration::from_millis(hold_ms),
        }
    }
}

pub trait PressSchedule: Send {
    /// Plans the next press. `now` is how long the run has been going, and is
    /// the single clock everything here is measured against: the returned
    /// `target_offset`, the automatic-stop deadline, and the emission instants
    /// reported to [`PressSchedule::record_press`].
    fn next_press(&mut self, now: Duration) -> PressPlan;

    /// Reports that the press just planned was emitted, `at` this far into the
    /// run. Emissions are always at least a little late, so this is what
    /// anchors the plan to reality: the next press stays a full interval after
    /// the one that really happened, and a cooldown starts from it.
    fn record_press(&mut self, at: Duration);
}

pub fn is_before_deadline(plan: &PressPlan, deadline: Option<Duration>) -> bool {
    deadline.is_none_or(|limit| plan.target_offset < limit)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{KeyEntry, LogicalKey, NaturalConfig, NaturalOverrides};

    use super::*;

    fn entries(entries: &[(LogicalKey, u8)]) -> Vec<KeyEntry> {
        entries
            .iter()
            .map(|&(key, weight)| KeyEntry {
                key,
                weight,
                cooldown_ms: 0,
            })
            .collect()
    }

    #[test]
    fn timer_wraps_without_offset_drift() {
        let keys = entries(&[(LogicalKey::KeyA, 1), (LogicalKey::Space, 1)]);
        let mut timer = TimerSchedule::new(&keys, Duration::from_millis(100));
        // Driven the way the worker does it: plan at the instant the run has
        // reached, then report the press emitted that much later. A real
        // emission is never exactly on time, so no latency here is zero.
        let mut press = |timer: &mut TimerSchedule, now: u64, latency: u64| {
            let plan = timer.next_press(Duration::from_millis(now));
            timer.record_press(plan.target_offset + Duration::from_millis(latency));
            plan
        };
        // A couple of milliseconds late every time: the pace stays exact
        // instead of charging each press the lateness of the one before it.
        assert_eq!(
            press(&mut timer, 0, 2),
            PressPlan::new(LogicalKey::KeyA, 0, 30)
        );
        assert_eq!(
            press(&mut timer, 32, 2),
            PressPlan::new(LogicalKey::Space, 100, 30)
        );
        assert_eq!(
            press(&mut timer, 132, 2),
            PressPlan::new(LogicalKey::KeyA, 200, 30)
        );
        // Planned for 300 ms but emitted at 1,200 ms, nine intervals late, as a
        // focus pause leaves it. The next press is a whole interval after that
        // press: not the stale 400 ms the accumulator was holding, and not a
        // catch-up burst.
        assert_eq!(
            press(&mut timer, 232, 900),
            PressPlan::new(LogicalKey::Space, 300, 30)
        );
        assert_eq!(
            press(&mut timer, 1_230, 2),
            PressPlan::new(LogicalKey::KeyA, 1_300, 30)
        );
    }

    #[test]
    fn natural_is_seeded_weighted_and_repeat_bounded() {
        let keys = entries(&[(LogicalKey::KeyA, 1), (LogicalKey::KeyB, 3)]);
        let mut a = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 7);
        let mut b = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 7);
        let left: Vec<_> = (0..1_000).map(|_| a.next_press(Duration::ZERO)).collect();
        let right: Vec<_> = (0..1_000).map(|_| b.next_press(Duration::ZERO)).collect();
        assert_eq!(left, right);
        assert!(
            !left
                .windows(4)
                .any(|window| window.iter().all(|plan| plan.key == window[0].key))
        );
    }

    /// Golden record of a seeded natural run. Any change that shifts a random
    /// draw or an offset shows up here, so it pins "no cooldown behaves exactly
    /// as it did before cooldowns existed".
    #[test]
    fn natural_without_cooldowns_matches_its_recorded_golden_run() {
        let keys = entries(&[
            (LogicalKey::KeyA, 1),
            (LogicalKey::KeyB, 3),
            (LogicalKey::KeyC, 6),
        ]);
        let mut schedule = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 0x601D);
        let mut emitter = Emitter::new(&[0]);
        let plans: Vec<(LogicalKey, u64, u64)> = (0..20)
            .map(|_| {
                let plan = emitter.press(&mut schedule).0;
                (
                    plan.key,
                    plan.target_offset.as_millis() as u64,
                    plan.hold_for.as_millis() as u64,
                )
            })
            .collect();

        assert_eq!(
            plans,
            [
                (LogicalKey::KeyC, 0, 97),
                (LogicalKey::KeyC, 142, 52),
                (LogicalKey::KeyC, 264, 66),
                (LogicalKey::KeyB, 373, 89),
                (LogicalKey::KeyC, 709, 74),
                (LogicalKey::KeyB, 903, 47),
                (LogicalKey::KeyC, 1_056, 49),
                (LogicalKey::KeyC, 1_204, 59),
                (LogicalKey::KeyC, 1_351, 82),
                (LogicalKey::KeyB, 1_587, 91),
                (LogicalKey::KeyB, 1_755, 89),
                (LogicalKey::KeyA, 1_946, 61),
                (LogicalKey::KeyB, 2_091, 38),
                (LogicalKey::KeyC, 2_331, 42),
                (LogicalKey::KeyC, 3_015, 60),
                (LogicalKey::KeyC, 3_258, 73),
                (LogicalKey::KeyB, 3_475, 40),
                (LogicalKey::KeyA, 3_637, 65),
                (LogicalKey::KeyB, 3_964, 50),
                (LogicalKey::KeyC, 4_284, 90),
            ]
        );
    }

    /// Stands in for the worker loop: plans at the instant the run has reached,
    /// then reports the press emitted some latency after the instant it was
    /// planned for. Real emissions are always at least a little late and the
    /// lateness varies, so a fixed zero latency describes a run that cannot
    /// happen -- the cooldown tests cycle real ones.
    struct Emitter {
        now: Duration,
        latencies: Vec<Duration>,
        presses: usize,
    }

    impl Emitter {
        fn new(latencies: &[u64]) -> Self {
            Self {
                now: Duration::ZERO,
                latencies: latencies
                    .iter()
                    .map(|&ms| Duration::from_millis(ms))
                    .collect(),
                presses: 0,
            }
        }

        /// Returns the plan and the instant the press was actually emitted.
        fn press(&mut self, schedule: &mut NaturalSchedule) -> (PressPlan, Duration) {
            let plan = schedule.next_press(self.now);
            let latency = self.latencies[self.presses % self.latencies.len()];
            self.presses += 1;
            let at = plan.target_offset.saturating_add(latency);
            schedule.record_press(at);
            // The worker plans the next press after releasing this one.
            self.now = at.saturating_add(plan.hold_for);
            (plan, at)
        }
    }

    fn cooling_entries(entries: &[(LogicalKey, u8, u32)]) -> Vec<KeyEntry> {
        entries
            .iter()
            .map(|&(key, weight, cooldown_ms)| KeyEntry {
                key,
                weight,
                cooldown_ms,
            })
            .collect()
    }

    /// Fixed 100 ms intervals with no bursts and no pauses, so a cooldown test
    /// reads the schedule's timing decisions instead of its sampling.
    fn fixed_interval_settings() -> NaturalSettings {
        NaturalSettings::from_config(&NaturalConfig {
            naturalness: 50,
            advanced: Some(NaturalOverrides {
                min_interval_ms: 100,
                max_interval_ms: 100,
                burst_intensity: 0,
                pause_chance_percent: 0,
            }),
        })
    }

    #[test]
    fn a_cooling_single_key_paces_presses_to_its_cooldown() {
        let keys = cooling_entries(&[(LogicalKey::KeyA, 1, 1_000)]);
        let mut schedule = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 5);
        // Varying emission latency, the way a real run behaves. The cooldown is
        // measured from the press, so it must hold between actual emissions
        // however early or late any one of them lands.
        let mut emitter = Emitter::new(&[0, 37, 5, 420, 12]);
        let emissions: Vec<_> = (0..50).map(|_| emitter.press(&mut schedule).1).collect();

        for window in emissions.windows(2) {
            let gap = window[1] - window[0];
            assert!(
                gap >= Duration::from_millis(1_000),
                "gap {gap:?} is shorter than the cooldown: {emissions:?}"
            );
        }
        assert!(
            emissions
                .windows(2)
                .any(|window| window[1] - window[0] < Duration::from_millis(1_200)),
            "the cooldown must set the pace, not merely bound it: {emissions:?}"
        );
    }

    #[test]
    fn every_key_cooling_waits_for_the_earliest_expiry() {
        let keys = cooling_entries(&[(LogicalKey::KeyA, 1, 1_000), (LogicalKey::KeyB, 1, 5_000)]);
        let mut schedule = NaturalSchedule::seeded(&keys, fixed_interval_settings(), 3);
        let mut emitter = Emitter::new(&[0]);
        let plans: Vec<_> = (0..9)
            .map(|_| {
                let plan = emitter.press(&mut schedule).0;
                (plan.key, plan.target_offset.as_millis() as u64)
            })
            .collect();

        assert_eq!(
            plans,
            [
                (LogicalKey::KeyA, 0),
                (LogicalKey::KeyB, 100),
                // Both keys are cooling from here on, so every press waits for
                // the earliest expiry and takes the key that expired.
                (LogicalKey::KeyA, 1_000),
                (LogicalKey::KeyA, 2_000),
                (LogicalKey::KeyA, 3_000),
                (LogicalKey::KeyA, 4_000),
                (LogicalKey::KeyA, 5_000),
                (LogicalKey::KeyB, 5_100),
                (LogicalKey::KeyA, 6_000),
            ]
        );
    }

    /// The natural twin of the timer resume case, and it has nothing to do with
    /// cooldowns: a press held up past its own interval must still leave a full
    /// interval before the next one, rather than the leftover of a plan made
    /// before the hold-up.
    #[test]
    fn a_press_delayed_past_its_interval_still_leads_the_next_by_a_full_interval() {
        let keys = cooling_entries(&[(LogicalKey::KeyA, 1, 0)]);
        let mut schedule = NaturalSchedule::seeded(&keys, fixed_interval_settings(), 7);
        // The third press is emitted 2 s late, as a focus pause would leave it.
        let mut emitter = Emitter::new(&[0, 0, 2_000, 0, 0, 0]);
        let emissions: Vec<_> = (0..6)
            .map(|_| emitter.press(&mut schedule).1.as_millis() as u64)
            .collect();

        assert_eq!(emissions, [0, 100, 2_200, 2_300, 2_400, 2_500]);
    }

    /// A cooling key must drop out of the weighted draw without distorting the
    /// keys that remain: their shares have to match the repeat-capped reference
    /// for their own weights alone.
    #[test]
    fn a_cooling_key_renormalizes_the_weights_of_the_keys_that_remain() {
        let keys = cooling_entries(&[
            (LogicalKey::KeyA, 1, 0),
            (LogicalKey::KeyB, 3, 0),
            (LogicalKey::KeyC, 6, 60_000),
        ]);
        let expected = repeat_capped_reference(&[1, 3]);
        let mut schedule = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 0xC001);
        let mut emitter = Emitter::new(&[0]);
        let mut counts = [0_u64; 3];
        for _ in 0..100_000 {
            let index = match emitter.press(&mut schedule).0.key {
                LogicalKey::KeyA => 0,
                LogicalKey::KeyB => 1,
                LogicalKey::KeyC => 2,
                key => panic!("unexpected key {key:?}"),
            };
            counts[index] += 1;
        }

        // The 60 s cooldown lets KeyC back in roughly once a minute of schedule
        // time. That is well under 1% of the presses, so the KeyA-to-KeyB chain
        // it interrupts stays inside the 1 percentage point tolerance.
        assert!(
            counts[2] * 100 < counts.iter().sum::<u64>(),
            "KeyC took {} of {} presses",
            counts[2],
            counts.iter().sum::<u64>()
        );
        let uncooled = (counts[0] + counts[1]) as f64;
        for index in 0..2 {
            let observed = counts[index] as f64 / uncooled;
            assert!(
                (observed - expected[index]).abs() <= 0.01,
                "key {index}: observed {observed:.5}, expected {0:.5}",
                expected[index]
            );
        }
    }

    #[test]
    fn natural_single_key_remains_stable_past_the_repeat_counter_range() {
        let keys = entries(&[(LogicalKey::KeyA, 1)]);
        let mut schedule = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 11);
        let plans: Vec<_> = (0..1_000)
            .map(|_| schedule.next_press(Duration::ZERO))
            .collect();
        assert!(plans.iter().all(|plan| plan.key == LogicalKey::KeyA));
    }

    #[test]
    #[should_panic(expected = "natural schedule keys must be unique")]
    fn natural_rejects_duplicate_keys_at_construction() {
        let keys = entries(&[(LogicalKey::KeyA, 1), (LogicalKey::KeyA, 1)]);
        let _schedule = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 11);
    }

    #[test]
    fn natural_settings_interpolate_with_exact_integer_rounding() {
        let calm = NaturalSettings::from_slider(0);
        assert_eq!(
            (
                calm.min_interval_ms(),
                calm.max_interval_ms(),
                calm.min_hold_ms(),
                calm.max_hold_ms(),
                calm.burst_chance_basis_points(),
                calm.pause_chance_basis_points(),
                calm.burst_intensity(),
                calm.max_burst_length(),
            ),
            (140, 220, 45, 75, 200, 100, 8, 2)
        );

        let middle = NaturalSettings::from_slider(50);
        assert_eq!(
            (
                middle.min_interval_ms(),
                middle.max_interval_ms(),
                middle.min_hold_ms(),
                middle.max_hold_ms(),
                middle.burst_chance_basis_points(),
                middle.pause_chance_basis_points(),
                middle.burst_intensity(),
                middle.max_burst_length(),
            ),
            (98, 350, 38, 98, 1_300, 650, 54, 3)
        );

        let erratic = NaturalSettings::from_slider(100);
        assert_eq!(
            (
                erratic.min_interval_ms(),
                erratic.max_interval_ms(),
                erratic.min_hold_ms(),
                erratic.max_hold_ms(),
                erratic.burst_chance_basis_points(),
                erratic.pause_chance_basis_points(),
                erratic.burst_intensity(),
                erratic.max_burst_length(),
            ),
            (55, 480, 30, 120, 2_400, 1_200, 100, 5)
        );
    }

    #[test]
    fn advanced_settings_apply_bounds_and_disable_zero_intensity_bursts() {
        let disabled = NaturalSettings::from_config(&NaturalConfig {
            naturalness: 0,
            advanced: Some(NaturalOverrides {
                min_interval_ms: 40,
                max_interval_ms: 5_000,
                burst_intensity: 0,
                pause_chance_percent: 25,
            }),
        });
        assert_eq!(disabled.min_interval_ms(), 40);
        assert_eq!(disabled.max_interval_ms(), 5_000);
        assert_eq!(disabled.burst_chance_basis_points(), 0);
        assert_eq!(disabled.pause_chance_basis_points(), 2_500);

        let lowest_enabled = NaturalSettings::from_config(&NaturalConfig {
            naturalness: 100,
            advanced: Some(NaturalOverrides {
                min_interval_ms: 40,
                max_interval_ms: 40,
                burst_intensity: 1,
                pause_chance_percent: 0,
            }),
        });
        assert_eq!(lowest_enabled.burst_chance_basis_points(), 24);
        assert_eq!(lowest_enabled.max_burst_length(), 2);

        for (intensity, expected_length) in [(33, 2), (34, 3), (66, 3), (67, 4), (99, 4), (100, 5)]
        {
            let settings = NaturalSettings::from_config(&NaturalConfig {
                naturalness: 50,
                advanced: Some(NaturalOverrides {
                    min_interval_ms: 40,
                    max_interval_ms: 5_000,
                    burst_intensity: intensity,
                    pause_chance_percent: 0,
                }),
            });
            assert_eq!(settings.max_burst_length(), expected_length);
            assert_eq!(
                settings.burst_chance_basis_points(),
                u32::from(intensity) * 24
            );
        }
    }

    #[test]
    fn timer_and_natural_deadlines_are_exclusive() {
        let keys = entries(&[(LogicalKey::KeyA, 1)]);
        let mut timer = TimerSchedule::new(&keys, Duration::from_millis(100));
        let timer_at_zero = timer.next_press(Duration::ZERO);
        let timer_at_deadline = timer.next_press(Duration::ZERO);
        assert!(is_before_deadline(
            &timer_at_zero,
            Some(Duration::from_millis(100))
        ));
        assert!(!is_before_deadline(
            &timer_at_deadline,
            Some(Duration::from_millis(100))
        ));

        let mut natural = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 19);
        let natural_at_zero = natural.next_press(Duration::ZERO);
        let natural_later = natural.next_press(Duration::ZERO);
        assert!(!is_before_deadline(&natural_at_zero, Some(Duration::ZERO)));
        assert!(!is_before_deadline(
            &natural_later,
            Some(natural_later.target_offset)
        ));
        assert!(is_before_deadline(&natural_later, None));
    }

    #[test]
    fn natural_samples_stay_inside_slider_and_advanced_bounds() {
        let advanced = NaturalSettings::from_config(&NaturalConfig {
            naturalness: 100,
            advanced: Some(NaturalOverrides {
                min_interval_ms: 40,
                max_interval_ms: 5_000,
                burst_intensity: 100,
                pause_chance_percent: 25,
            }),
        });
        for (settings, seed) in [
            (NaturalSettings::from_slider(0), 10),
            (NaturalSettings::from_slider(50), 20),
            (NaturalSettings::from_slider(100), 30),
            (advanced, 40),
        ] {
            assert_sample_bounds(settings, seed);
        }
    }

    fn assert_sample_bounds(settings: NaturalSettings, seed: u64) {
        let keys = entries(&[(LogicalKey::KeyA, 1), (LogicalKey::KeyB, 1)]);
        let mut schedule = NaturalSchedule::seeded(&keys, settings, seed);
        let plans: Vec<_> = (0..=100_000)
            .map(|_| schedule.next_press(Duration::ZERO))
            .collect();
        let minimum_hold = settings
            .min_hold_ms()
            .min(settings.min_interval_ms().saturating_sub(10));

        for window in plans.windows(2).take(100_000) {
            let plan = &window[0];
            let interval = (window[1].target_offset - plan.target_offset).as_millis() as u64;
            let hold = plan.hold_for.as_millis() as u64;
            assert!(
                (settings.min_interval_ms()..=5_000).contains(&interval),
                "seed {seed} generated interval {interval} ms"
            );
            assert!(
                (minimum_hold..=settings.max_hold_ms()).contains(&hold),
                "seed {seed} generated hold {hold} ms"
            );
            assert!(
                hold + 10 <= interval,
                "seed {seed} left less than the 10 ms release gap"
            );
        }
    }

    #[test]
    fn weighted_sample_matches_independent_repeat_capped_reference() {
        let weights = [1_u8, 3, 6];
        let keys = entries(&[
            (LogicalKey::KeyA, weights[0]),
            (LogicalKey::KeyB, weights[1]),
            (LogicalKey::KeyC, weights[2]),
        ]);
        let expected = repeat_capped_reference(&weights);
        let mut schedule =
            NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 0xA11CE);
        let mut emitter = Emitter::new(&[0]);
        let mut counts = [0_u64; 3];
        for _ in 0..100_000 {
            let index = match emitter.press(&mut schedule).0.key {
                LogicalKey::KeyA => 0,
                LogicalKey::KeyB => 1,
                LogicalKey::KeyC => 2,
                key => panic!("unexpected key {key:?}"),
            };
            counts[index] += 1;
        }

        for index in 0..weights.len() {
            let observed = counts[index] as f64 / 100_000.0;
            assert!(
                (observed - expected[index]).abs() <= 0.01,
                "key {index}: observed {observed:.5}, expected {0:.5}",
                expected[index]
            );
        }
    }

    fn repeat_capped_reference(weights: &[u8]) -> Vec<f64> {
        let key_count = weights.len();
        let total_weight: u32 = weights.iter().map(|&weight| u32::from(weight)).sum();
        let mut states = vec![[0.0_f64; 3]; key_count];
        for (index, &weight) in weights.iter().enumerate() {
            states[index][0] = f64::from(weight) / f64::from(total_weight);
        }

        for _ in 0..10_000 {
            let mut next = vec![[0.0_f64; 3]; key_count];
            for (last, repeats) in states.iter().enumerate() {
                for (repeat_index, &mass) in repeats.iter().enumerate() {
                    let eligible_weight = if repeat_index == 2 {
                        total_weight - u32::from(weights[last])
                    } else {
                        total_weight
                    };
                    for selected in 0..key_count {
                        if repeat_index == 2 && selected == last {
                            continue;
                        }
                        let probability = f64::from(weights[selected]) / f64::from(eligible_weight);
                        let selected_repeat = if selected == last {
                            repeat_index + 1
                        } else {
                            0
                        };
                        next[selected][selected_repeat] += mass * probability;
                    }
                }
            }
            states = next;
        }

        states.iter().map(|repeats| repeats.iter().sum()).collect()
    }
}
