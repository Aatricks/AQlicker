use std::collections::HashSet;

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::{KeyEntry, LogicalKey, NaturalConfig};

use super::{PressPlan, PressSchedule};

const CHANCE_SCALE: u32 = 10_000;
const MIN_RELEASE_GAP_MS: u64 = 10;
const MAX_PAUSE_MS: u64 = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NaturalSettings {
    min_interval_ms: u64,
    max_interval_ms: u64,
    min_hold_ms: u64,
    max_hold_ms: u64,
    burst_intensity: u8,
    burst_chance_basis_points: u32,
    pause_chance_basis_points: u32,
}

impl NaturalSettings {
    pub fn from_slider(naturalness: u8) -> Self {
        assert!(naturalness <= 100, "naturalness must be in 0..=100");
        Self {
            min_interval_ms: interpolate(140, 55, naturalness),
            max_interval_ms: interpolate(220, 480, naturalness),
            min_hold_ms: interpolate(45, 30, naturalness),
            max_hold_ms: interpolate(75, 120, naturalness),
            burst_intensity: interpolate(8, 100, naturalness) as u8,
            burst_chance_basis_points: interpolate(200, 2_400, naturalness) as u32,
            pause_chance_basis_points: interpolate(100, 1_200, naturalness) as u32,
        }
    }

    pub fn from_config(config: &NaturalConfig) -> Self {
        let mut settings = Self::from_slider(config.naturalness);
        if let Some(advanced) = &config.advanced {
            settings.min_interval_ms = u64::from(advanced.min_interval_ms);
            settings.max_interval_ms = u64::from(advanced.max_interval_ms);
            settings.burst_intensity = advanced.burst_intensity;
            settings.burst_chance_basis_points = u32::from(advanced.burst_intensity) * 24;
            settings.pause_chance_basis_points = u32::from(advanced.pause_chance_percent) * 100;
        }
        settings
    }

    pub const fn min_interval_ms(self) -> u64 {
        self.min_interval_ms
    }

    pub const fn max_interval_ms(self) -> u64 {
        self.max_interval_ms
    }

    pub const fn min_hold_ms(self) -> u64 {
        self.min_hold_ms
    }

    pub const fn max_hold_ms(self) -> u64 {
        self.max_hold_ms
    }

    pub const fn burst_intensity(self) -> u8 {
        self.burst_intensity
    }

    pub const fn burst_chance_basis_points(self) -> u32 {
        self.burst_chance_basis_points
    }

    pub const fn pause_chance_basis_points(self) -> u32 {
        self.pause_chance_basis_points
    }

    pub const fn max_burst_length(self) -> u8 {
        2 + ((3 * self.burst_intensity as u16) / 100) as u8
    }
}

fn interpolate(start: u64, end: u64, naturalness: u8) -> u64 {
    let position = u128::from(naturalness);
    let inverse = 100 - position;
    let numerator = u128::from(start) * inverse + u128::from(end) * position;
    ((numerator + 50) / 100) as u64
}

pub struct NaturalSchedule {
    keys: Vec<KeyEntry>,
    settings: NaturalSettings,
    rng: ChaCha8Rng,
    target_offset_ms: u64,
    previous_normal_ms: Option<u64>,
    last_key: Option<LogicalKey>,
    repeat_count: u8,
    burst_gaps_remaining: u8,
    #[cfg(test)]
    last_timing: Option<TestTiming>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestTiming {
    Normal {
        normal_ms: u64,
        mode_ms: u64,
    },
    Burst {
        normal_ms: u64,
        mode_ms: u64,
        percent: u64,
        starts_length: Option<u8>,
    },
    Pause {
        normal_ms: u64,
        mode_ms: u64,
        factor_hundredths: u64,
    },
}

impl NaturalSchedule {
    pub fn new(keys: &[KeyEntry], settings: NaturalSettings) -> Self {
        let seed = rand::rng().random();
        Self::seeded(keys, settings, seed)
    }

    pub fn seeded(keys: &[KeyEntry], settings: NaturalSettings, seed: u64) -> Self {
        assert!(
            !keys.is_empty(),
            "a natural schedule requires at least one key"
        );
        assert!(
            keys.iter().all(|entry| (1..=10).contains(&entry.weight)),
            "natural schedule weights must be in 1..=10"
        );
        assert_eq!(
            keys.iter()
                .map(|entry| entry.key)
                .collect::<HashSet<_>>()
                .len(),
            keys.len(),
            "natural schedule keys must be unique"
        );
        assert!(
            settings.min_interval_ms <= settings.max_interval_ms,
            "minimum interval must not exceed maximum interval"
        );
        Self {
            keys: keys.to_vec(),
            settings,
            rng: ChaCha8Rng::seed_from_u64(seed),
            target_offset_ms: 0,
            previous_normal_ms: None,
            last_key: None,
            repeat_count: 0,
            burst_gaps_remaining: 0,
            #[cfg(test)]
            last_timing: None,
        }
    }

    fn choose_key(&mut self) -> LogicalKey {
        let excluded = (self.keys.len() >= 2 && self.repeat_count >= 3)
            .then_some(self.last_key)
            .flatten();
        let total_weight: u32 = self
            .keys
            .iter()
            .filter(|entry| Some(entry.key) != excluded)
            .map(|entry| u32::from(entry.weight))
            .sum();
        let mut draw = self.rng.random_range(0..total_weight);
        let key = self
            .keys
            .iter()
            .filter(|entry| Some(entry.key) != excluded)
            .find_map(|entry| {
                let weight = u32::from(entry.weight);
                if draw < weight {
                    Some(entry.key)
                } else {
                    draw -= weight;
                    None
                }
            })
            .expect("positive validated weights leave at least one candidate");

        if self.last_key == Some(key) {
            self.repeat_count = self.repeat_count.saturating_add(1);
        } else {
            self.last_key = Some(key);
            self.repeat_count = 1;
        }
        key
    }

    fn sample_normal_interval(&mut self) -> (u64, u64) {
        let min = self.settings.min_interval_ms;
        let max = self.settings.max_interval_ms;
        let mode = self
            .previous_normal_ms
            .unwrap_or_else(|| min + (max - min) / 2)
            .clamp(min, max);
        let sampled = sample_discrete_triangular(&mut self.rng, min, max, mode);
        self.previous_normal_ms = Some(sampled);
        (sampled, mode)
    }

    fn chance(&mut self, basis_points: u32) -> bool {
        basis_points != 0 && self.rng.random_range(0..CHANCE_SCALE) < basis_points
    }

    fn next_interval(&mut self) -> u64 {
        let (normal, mode) = self.sample_normal_interval();

        if self.burst_gaps_remaining > 0 {
            self.burst_gaps_remaining -= 1;
            return self.burst_interval(normal, mode, None);
        }

        if self.chance(self.settings.pause_chance_basis_points) {
            let factor = self.rng.random_range(180_u64..=350);
            #[cfg(test)]
            {
                self.last_timing = Some(TestTiming::Pause {
                    normal_ms: normal,
                    mode_ms: mode,
                    factor_hundredths: factor,
                });
            }
            return rounded_ratio(normal, factor, 100).min(MAX_PAUSE_MS);
        }

        if self.settings.burst_intensity != 0
            && self.chance(self.settings.burst_chance_basis_points)
        {
            let length = self.rng.random_range(2..=self.settings.max_burst_length());
            self.burst_gaps_remaining = length - 1;
            return self.burst_interval(normal, mode, Some(length));
        }

        #[cfg(test)]
        {
            self.last_timing = Some(TestTiming::Normal {
                normal_ms: normal,
                mode_ms: mode,
            });
        }
        normal
    }

    fn burst_interval(&mut self, normal: u64, _mode: u64, _starts_length: Option<u8>) -> u64 {
        let percent = self.rng.random_range(55_u64..=80);
        #[cfg(test)]
        {
            self.last_timing = Some(TestTiming::Burst {
                normal_ms: normal,
                mode_ms: _mode,
                percent,
                starts_length: _starts_length,
            });
        }
        rounded_ratio(normal, percent, 100).max(self.settings.min_interval_ms)
    }
}

impl PressSchedule for NaturalSchedule {
    fn next_press(&mut self) -> PressPlan {
        let key = self.choose_key();
        let interval_ms = self.next_interval();
        let sampled_hold = self
            .rng
            .random_range(self.settings.min_hold_ms..=self.settings.max_hold_ms);
        let hold_ms = sampled_hold.min(interval_ms.saturating_sub(MIN_RELEASE_GAP_MS));
        let plan = PressPlan::new(key, self.target_offset_ms, hold_ms);
        self.target_offset_ms = self.target_offset_ms.saturating_add(interval_ms);
        plan
    }
}

fn rounded_ratio(value: u64, numerator: u64, denominator: u64) -> u64 {
    let product = u128::from(value) * u128::from(numerator);
    ((product + u128::from(denominator / 2)) / u128::from(denominator)) as u64
}

fn sample_discrete_triangular(rng: &mut ChaCha8Rng, min: u64, max: u64, mode: u64) -> u64 {
    if min == max {
        return min;
    }

    let left_count = u128::from(mode - min + 1);
    let right_count = u128::from(max - mode + 1);
    let left_total = right_count * left_count * (left_count + 1) / 2;
    let right_total = left_count * (right_count - 1) * right_count / 2;
    let draw = u128::from(rng.random_range(0..(left_total + right_total) as u64));

    if draw < left_total {
        let rank = first_rank(left_count, |rank| {
            right_count * rank * (rank + 1) / 2 > draw
        });
        min + rank as u64 - 1
    } else {
        let right_draw = draw - left_total;
        let rank = first_rank(right_count - 1, |rank| {
            left_count * (rank * right_count - rank * (rank + 1) / 2) > right_draw
        });
        mode + rank as u64
    }
}

fn first_rank(mut high: u128, predicate: impl Fn(u128) -> bool) -> u128 {
    let mut low = 1;
    while low < high {
        let middle = low + (high - low) / 2;
        if predicate(middle) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

#[cfg(test)]
mod tests {
    use crate::{KeyEntry, LogicalKey, NaturalOverrides};

    use super::*;

    #[test]
    fn timing_samples_preserve_normal_burst_and_pause_state_contracts() {
        let disabled_bursts = NaturalSettings::from_config(&NaturalConfig {
            naturalness: 0,
            advanced: Some(NaturalOverrides {
                min_interval_ms: 40,
                max_interval_ms: 5_000,
                burst_intensity: 0,
                pause_chance_percent: 25,
            }),
        });
        for (settings, seed, expect_bursts) in [
            (NaturalSettings::from_slider(0), 101, true),
            (NaturalSettings::from_slider(50), 102, true),
            (NaturalSettings::from_slider(100), 103, true),
            (disabled_bursts, 104, false),
        ] {
            assert_timing_contract(settings, seed, expect_bursts);
        }
    }

    fn assert_timing_contract(settings: NaturalSettings, seed: u64, expect_bursts: bool) {
        let keys = [KeyEntry::new(LogicalKey::KeyA)];
        let mut schedule = NaturalSchedule::seeded(&keys, settings, seed);
        let mut previous_normal = None;
        let mut expected_burst_gaps = 0_u8;
        let mut burst_starts = 0_u64;
        let mut pauses = 0_u64;

        for _ in 0..100_000 {
            let expected_mode = previous_normal.unwrap_or_else(|| {
                settings.min_interval_ms + (settings.max_interval_ms - settings.min_interval_ms) / 2
            });
            let interval = schedule.next_interval();
            let timing = schedule.last_timing.expect("test timing must be recorded");
            let (normal, mode) = match timing {
                TestTiming::Normal { normal_ms, mode_ms } => {
                    assert_eq!(expected_burst_gaps, 0);
                    assert_eq!(interval, normal_ms);
                    (normal_ms, mode_ms)
                }
                TestTiming::Burst {
                    normal_ms,
                    mode_ms,
                    percent,
                    starts_length,
                } => {
                    assert!((55..=80).contains(&percent));
                    assert_eq!(
                        interval,
                        rounded_ratio(normal_ms, percent, 100).max(settings.min_interval_ms)
                    );
                    if let Some(length) = starts_length {
                        assert_eq!(expected_burst_gaps, 0);
                        assert!((2..=settings.max_burst_length()).contains(&length));
                        expected_burst_gaps = length - 1;
                        burst_starts += 1;
                    } else {
                        assert!(expected_burst_gaps > 0);
                        expected_burst_gaps -= 1;
                    }
                    (normal_ms, mode_ms)
                }
                TestTiming::Pause {
                    normal_ms,
                    mode_ms,
                    factor_hundredths,
                } => {
                    assert_eq!(expected_burst_gaps, 0);
                    assert!((180..=350).contains(&factor_hundredths));
                    assert_eq!(
                        interval,
                        rounded_ratio(normal_ms, factor_hundredths, 100).min(MAX_PAUSE_MS)
                    );
                    pauses += 1;
                    (normal_ms, mode_ms)
                }
            };

            assert_eq!(mode, expected_mode);
            assert!((settings.min_interval_ms..=settings.max_interval_ms).contains(&normal));
            assert_eq!(schedule.burst_gaps_remaining, expected_burst_gaps);
            previous_normal = Some(normal);
        }

        assert!(pauses > 0, "seed {seed} did not exercise a pause");
        if expect_bursts {
            assert!(burst_starts > 0, "seed {seed} did not exercise a burst");
        } else {
            assert_eq!(burst_starts, 0);
        }
    }
}
