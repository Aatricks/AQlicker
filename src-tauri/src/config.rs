use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::key::LogicalKey;

pub const CURRENT_SCHEMA_VERSION: u32 = 2;
const MIN_INTERVAL_MS: u32 = 40;
const MAX_TIMER_INTERVAL_MS: u32 = 60_000;
const MAX_NATURAL_INTERVAL_MS: u32 = 5_000;
const MAX_PAUSE_CHANCE_PERCENT: u8 = 25;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub keys: Vec<KeyEntry>,
    pub mode: Mode,
    pub timer: TimerConfig,
    pub natural: NaturalConfig,
    pub stop_after: Option<u32>,
    pub global_shortcut: String,
    /// Optional application the run is restricted to. `None` keeps the run
    /// unrestricted, which is the behaviour every schema-v1 file migrates to.
    pub target_app: Option<TargetApp>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            keys: Vec::new(),
            mode: Mode::Timer,
            timer: TimerConfig { interval_ms: 100 },
            natural: NaturalConfig {
                naturalness: 50,
                advanced: None,
            },
            stop_after: None,
            global_shortcut: "CommandOrControl+Shift+K".to_owned(),
            target_app: None,
        }
    }
}

impl AppConfig {
    /// Validates values that may be stored in the durable configuration file.
    pub fn validate(&self) -> Vec<ValidationError> {
        let mut errors = Vec::new();

        if self.schema_version != CURRENT_SCHEMA_VERSION {
            errors.push(ValidationError::new("schemaVersion", "unsupported-schema"));
        }

        let mut seen = HashSet::new();
        for entry in &self.keys {
            if !seen.insert(entry.key) {
                errors.push(ValidationError::new("keys", "duplicate"));
                break;
            }
        }

        for (index, entry) in self.keys.iter().enumerate() {
            if !(1..=10).contains(&entry.weight) {
                errors.push(ValidationError::new(
                    format!("keys[{index}].weight"),
                    "range",
                ));
            }
        }

        if !(MIN_INTERVAL_MS..=MAX_TIMER_INTERVAL_MS).contains(&self.timer.interval_ms) {
            errors.push(ValidationError::new("timer.intervalMs", "range"));
        }

        if self.natural.naturalness > 100 {
            errors.push(ValidationError::new("natural.naturalness", "range"));
        }

        if let Some(advanced) = &self.natural.advanced {
            if !(MIN_INTERVAL_MS..=MAX_NATURAL_INTERVAL_MS).contains(&advanced.min_interval_ms) {
                errors.push(ValidationError::new(
                    "natural.advanced.minIntervalMs",
                    "range",
                ));
            }
            if !(MIN_INTERVAL_MS..=MAX_NATURAL_INTERVAL_MS).contains(&advanced.max_interval_ms) {
                errors.push(ValidationError::new(
                    "natural.advanced.maxIntervalMs",
                    "range",
                ));
            }
            if advanced.min_interval_ms > advanced.max_interval_ms {
                errors.push(ValidationError::new("natural.advanced", "ordering"));
            }
            if advanced.burst_intensity > 100 {
                errors.push(ValidationError::new(
                    "natural.advanced.burstIntensity",
                    "range",
                ));
            }
            if advanced.pause_chance_percent > MAX_PAUSE_CHANCE_PERCENT {
                errors.push(ValidationError::new(
                    "natural.advanced.pauseChancePercent",
                    "range",
                ));
            }
        }

        if self
            .stop_after
            .is_some_and(|seconds| !(1..=86_400).contains(&seconds))
        {
            errors.push(ValidationError::new("stopAfter", "range"));
        }

        if self
            .target_app
            .as_ref()
            .is_some_and(|target| target.id.trim().is_empty())
        {
            errors.push(ValidationError::new("targetApp.id", "required"));
        }

        errors
    }

    /// Adds the start-only requirement that at least one key has been selected.
    pub fn validate_for_start(&self) -> Vec<ValidationError> {
        let mut errors = self.validate();
        if self.keys.is_empty() {
            errors.push(ValidationError::new("keys", "required"));
        }
        errors
    }
}

/// A user-visible application, identified by the stable platform identifier
/// (bundle identifier on macOS, executable name on Windows) and shown by name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TargetApp {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyEntry {
    pub key: LogicalKey,
    pub weight: u8,
}

impl KeyEntry {
    pub const fn new(key: LogicalKey) -> Self {
        Self { key, weight: 1 }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Timer,
    Natural,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TimerConfig {
    pub interval_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NaturalConfig {
    pub naturalness: u8,
    pub advanced: Option<NaturalOverrides>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NaturalOverrides {
    pub min_interval_ms: u32,
    pub max_interval_ms: u32,
    pub burst_intensity: u8,
    pub pause_chance_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub code: String,
}

impl ValidationError {
    pub fn new(field: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            code: code.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_validation_reports_each_invalid_field() {
        let config = AppConfig {
            keys: vec![
                KeyEntry {
                    key: LogicalKey::KeyA,
                    weight: 0,
                },
                KeyEntry::new(LogicalKey::KeyA),
            ],
            timer: TimerConfig { interval_ms: 39 },
            natural: NaturalConfig {
                naturalness: 101,
                advanced: Some(NaturalOverrides {
                    min_interval_ms: 5_001,
                    max_interval_ms: 39,
                    burst_intensity: 101,
                    pause_chance_percent: 101,
                }),
            },
            stop_after: Some(0),
            ..AppConfig::default()
        };

        let errors = config.validate();
        assert!(
            errors
                .iter()
                .any(|error| error.field == "keys" && error.code == "duplicate")
        );
        assert!(
            errors
                .iter()
                .any(|error| error.field == "keys[0].weight" && error.code == "range")
        );
        assert!(
            errors
                .iter()
                .any(|error| error.field == "timer.intervalMs" && error.code == "range")
        );
        assert!(
            errors
                .iter()
                .any(|error| error.field == "natural.advanced" && error.code == "ordering")
        );
        assert!(
            errors
                .iter()
                .any(|error| error.field == "stopAfter" && error.code == "range")
        );
    }

    #[test]
    fn config_validation_requires_a_key_only_when_starting() {
        let config = AppConfig::default();
        assert!(config.validate().is_empty());
        assert!(
            config
                .validate_for_start()
                .iter()
                .any(|error| error.field == "keys" && error.code == "required")
        );
    }

    #[test]
    fn config_validation_caps_pause_chance_at_twenty_five_percent() {
        let config_with_pause_chance = |pause_chance_percent| AppConfig {
            natural: NaturalConfig {
                naturalness: 50,
                advanced: Some(NaturalOverrides {
                    min_interval_ms: 100,
                    max_interval_ms: 500,
                    burst_intensity: 50,
                    pause_chance_percent,
                }),
            },
            ..AppConfig::default()
        };

        assert!(config_with_pause_chance(25).validate().is_empty());
        for pause_chance_percent in [26, 100] {
            assert!(
                config_with_pause_chance(pause_chance_percent)
                    .validate()
                    .iter()
                    .any(|error| error.field == "natural.advanced.pauseChancePercent"
                        && error.code == "range")
            );
        }
    }
}
