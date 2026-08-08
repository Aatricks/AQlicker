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
    fn next_press(&mut self) -> PressPlan;

    /// Reports that the press just planned was emitted, `at` this many
    /// milliseconds into the run. Schedules with a wall-clock constraint start
    /// it here; the timer schedule has none.
    fn record_press(&mut self, _at: Duration) {}
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
        assert_eq!(timer.next_press(), PressPlan::new(LogicalKey::KeyA, 0, 30));
        assert_eq!(
            timer.next_press(),
            PressPlan::new(LogicalKey::Space, 100, 30)
        );
        assert_eq!(
            timer.next_press(),
            PressPlan::new(LogicalKey::KeyA, 200, 30)
        );
    }

    #[test]
    fn natural_is_seeded_weighted_and_repeat_bounded() {
        let keys = entries(&[(LogicalKey::KeyA, 1), (LogicalKey::KeyB, 3)]);
        let mut a = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 7);
        let mut b = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 7);
        let left: Vec<_> = (0..1_000).map(|_| a.next_press()).collect();
        let right: Vec<_> = (0..1_000).map(|_| b.next_press()).collect();
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
        let plans: Vec<(LogicalKey, u64, u64)> = (0..20)
            .map(|_| {
                let plan = press(&mut schedule);
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

    /// Drives the schedule the way the worker does: plan a press, then report
    /// that it was emitted at the instant it was planned for.
    fn press(schedule: &mut NaturalSchedule) -> PressPlan {
        let plan = schedule.next_press();
        schedule.record_press(plan.target_offset);
        plan
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
        let plans: Vec<_> = (0..50).map(|_| press(&mut schedule)).collect();

        for window in plans.windows(2) {
            let gap = window[1].target_offset - window[0].target_offset;
            assert!(
                gap >= Duration::from_millis(1_000),
                "gap {gap:?} is shorter than the cooldown"
            );
        }
        assert!(
            plans
                .windows(2)
                .any(|window| window[1].target_offset - window[0].target_offset
                    == Duration::from_millis(1_000)),
            "the cooldown must set the pace, not merely bound it"
        );
    }

    #[test]
    fn every_key_cooling_waits_for_the_earliest_expiry() {
        let keys = cooling_entries(&[(LogicalKey::KeyA, 1, 1_000), (LogicalKey::KeyB, 1, 5_000)]);
        let mut schedule = NaturalSchedule::seeded(&keys, fixed_interval_settings(), 3);
        let plans: Vec<_> = (0..9)
            .map(|_| {
                let plan = press(&mut schedule);
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
        let mut counts = [0_u64; 3];
        for _ in 0..100_000 {
            let index = match press(&mut schedule).key {
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
        let plans: Vec<_> = (0..1_000).map(|_| schedule.next_press()).collect();
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
        let timer_at_zero = timer.next_press();
        let timer_at_deadline = timer.next_press();
        assert!(is_before_deadline(
            &timer_at_zero,
            Some(Duration::from_millis(100))
        ));
        assert!(!is_before_deadline(
            &timer_at_deadline,
            Some(Duration::from_millis(100))
        ));

        let mut natural = NaturalSchedule::seeded(&keys, NaturalSettings::from_slider(50), 19);
        let natural_at_zero = natural.next_press();
        let natural_later = natural.next_press();
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
        let plans: Vec<_> = (0..=100_000).map(|_| schedule.next_press()).collect();
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
        let mut counts = [0_u64; 3];
        for _ in 0..100_000 {
            let index = match press(&mut schedule).key {
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
