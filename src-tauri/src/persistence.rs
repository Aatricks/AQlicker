use std::{
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use atomic_write_file::AtomicWriteFile;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{AppConfig, CURRENT_SCHEMA_VERSION};

const CONFIG_FILE_NAME: &str = "config.json";

#[derive(Debug)]
pub struct ConfigRepository {
    directory: PathBuf,
}

impl ConfigRepository {
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: directory.as_ref().to_path_buf(),
        }
    }

    pub fn load(&self) -> Result<LoadedConfig, ConfigRepositoryError> {
        let path = self.path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(LoadedConfig::defaults());
            }
            Err(error) => return Err(ConfigRepositoryError::Io(error)),
        };

        let document: Value = match serde_json::from_slice(&bytes) {
            Ok(document) => document,
            Err(_) => return self.recover_corrupt(&path),
        };

        let config = match migrate_to_current(document) {
            Ok(config) => config,
            Err(ConfigRepositoryError::InvalidConfig) => return self.recover_corrupt(&path),
            Err(error) => return Err(error),
        };

        if !config.validate().is_empty() {
            return self.recover_corrupt(&path);
        }

        Ok(LoadedConfig {
            config,
            notice: None,
        })
    }

    pub fn save(&self, config: &AppConfig) -> Result<(), ConfigRepositoryError> {
        if !config.validate().is_empty() {
            return Err(ConfigRepositoryError::InvalidConfig);
        }

        fs::create_dir_all(&self.directory)?;
        let bytes = serde_json::to_vec_pretty(config)?;
        let mut file = AtomicWriteFile::open(self.path())?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        file.commit()?;
        Ok(())
    }

    fn path(&self) -> PathBuf {
        self.directory.join(CONFIG_FILE_NAME)
    }

    fn recover_corrupt(&self, original: &Path) -> Result<LoadedConfig, ConfigRepositoryError> {
        let backup = self.corrupt_backup_path();
        fs::rename(original, backup)?;
        Ok(LoadedConfig {
            config: AppConfig::default(),
            notice: Some(RecoveryNotice {
                code: "corrupt-config-recovered".to_owned(),
            }),
        })
    }

    fn corrupt_backup_path(&self) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self.directory
            .join(format!("{CONFIG_FILE_NAME}.corrupt-{timestamp}"))
    }
}

pub fn migrate_to_current(document: Value) -> Result<AppConfig, ConfigRepositoryError> {
    let schema_version = document
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or(ConfigRepositoryError::UnsupportedSchema)?;

    let mut document = document;
    // Migrations are sequential: each arm upgrades one version and falls through
    // to the next. v1 knew nothing about a target application, so it migrates to
    // v2 with the restriction switched off.
    if schema_version == 1 {
        document["schemaVersion"] = Value::from(2);
        document["targetApp"] = Value::Null;
    }

    // v2 knew nothing about per-key cooldowns, so every key migrates to v3 with
    // its cooldown switched off. A `keys` value that is not an array of objects
    // is left untouched and falls through to `InvalidConfig`, which keeps the
    // corrupt-file backup path in charge of it.
    if document.get("schemaVersion").and_then(Value::as_u64) == Some(2) {
        document["schemaVersion"] = Value::from(3);
        if let Some(keys) = document.get_mut("keys").and_then(Value::as_array_mut) {
            for entry in keys {
                if let Some(entry) = entry.as_object_mut() {
                    entry.insert("cooldownMs".to_owned(), Value::from(0));
                }
            }
        }
    }

    let schema_version = document
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or(ConfigRepositoryError::UnsupportedSchema)?;

    if schema_version == u64::from(CURRENT_SCHEMA_VERSION) {
        serde_json::from_value(document).map_err(|_| ConfigRepositoryError::InvalidConfig)
    } else {
        Err(ConfigRepositoryError::UnsupportedSchema)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    pub config: AppConfig,
    pub notice: Option<RecoveryNotice>,
}

impl LoadedConfig {
    fn defaults() -> Self {
        Self {
            config: AppConfig::default(),
            notice: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryNotice {
    pub code: String,
}

#[derive(Debug)]
pub enum ConfigRepositoryError {
    Io(std::io::Error),
    Serialization(serde_json::Error),
    InvalidConfig,
    UnsupportedSchema,
}

impl fmt::Display for ConfigRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "configuration I/O error: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "configuration serialization error: {error}")
            }
            Self::InvalidConfig => formatter.write_str("invalid configuration"),
            Self::UnsupportedSchema => formatter.write_str("unsupported-schema"),
        }
    }
}

impl std::error::Error for ConfigRepositoryError {}

impl From<std::io::Error> for ConfigRepositoryError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ConfigRepositoryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Serialization(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::KeyEntry;
    use crate::key::LogicalKey;

    #[test]
    fn persistence_saves_and_loads_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let repository = ConfigRepository::new(directory.path());
        let config = AppConfig {
            keys: vec![KeyEntry::new(LogicalKey::KeyA)],
            ..AppConfig::default()
        };

        repository.save(&config).unwrap();

        assert_eq!(repository.load().unwrap().config, config);
    }

    #[test]
    fn migration_loads_a_v1_file_with_the_target_application_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, include_str!("../tests/fixtures/config-v1.json")).unwrap();

        let loaded = ConfigRepository::new(directory.path()).load().unwrap();

        assert!(loaded.notice.is_none());
        assert_eq!(loaded.config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.config.target_app, None);
        assert_eq!(loaded.config.natural.naturalness, 65);
        assert_eq!(loaded.config.stop_after, Some(3_600));
        assert!(
            loaded
                .config
                .keys
                .iter()
                .all(|entry| entry.cooldown_ms == 0)
        );
    }

    #[test]
    fn migration_loads_a_v2_file_with_every_cooldown_disabled() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, include_str!("../tests/fixtures/config-v2.json")).unwrap();

        let loaded = ConfigRepository::new(directory.path()).load().unwrap();

        assert!(loaded.notice.is_none());
        assert_eq!(loaded.config.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(loaded.config.keys.len(), 6);
        assert!(
            loaded
                .config
                .keys
                .iter()
                .all(|entry| entry.cooldown_ms == 0)
        );
        assert_eq!(
            loaded.config.target_app.as_ref().map(|app| app.id.as_str()),
            Some("com.apple.TextEdit")
        );
    }

    #[test]
    fn migration_leaves_a_corrupt_v2_file_to_the_backup_path() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, r#"{"schemaVersion":2,"keys":"not-an-array"}"#).unwrap();

        let loaded = ConfigRepository::new(directory.path()).load().unwrap();

        assert_eq!(loaded.notice.unwrap().code, "corrupt-config-recovered");
        assert!(!path.exists());
    }

    #[test]
    fn persistence_rejects_future_schema_without_replacing_the_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(CONFIG_FILE_NAME);
        fs::write(&path, r#"{"schemaVersion":4}"#).unwrap();

        let error = ConfigRepository::new(directory.path()).load().unwrap_err();

        assert!(matches!(error, ConfigRepositoryError::UnsupportedSchema));
        assert!(path.exists());
    }
}
