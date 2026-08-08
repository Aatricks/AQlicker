use aqlicker_lib::{AppConfig, ConfigRepository, KeyEntry, LogicalKey};
use std::collections::HashSet;

#[test]
fn supported_key_catalog_matches_the_exhaustive_golden_list() {
    let expected: Vec<String> =
        serde_json::from_str(include_str!("fixtures/logical-keys.json")).unwrap();
    let serialized = serde_json::to_string(&LogicalKey::ALL[..]).unwrap();
    let actual: Vec<String> = serde_json::from_str(&serialized).unwrap();

    assert_eq!(
        expected.iter().collect::<HashSet<_>>().len(),
        expected.len()
    );
    assert_eq!(actual.iter().collect::<HashSet<_>>().len(), actual.len());
    assert_eq!(actual, expected);
}

#[test]
fn contract_deserializes_the_v1_fixture_with_camel_case_fields() {
    let fixture = include_str!("fixtures/config-v1.json");
    let config: AppConfig = serde_json::from_str(fixture).unwrap();

    assert_eq!(config.schema_version, 1);
    assert_eq!(
        config.keys,
        vec![
            KeyEntry {
                key: LogicalKey::KeyA,
                weight: 3,
            },
            KeyEntry {
                key: LogicalKey::Digit1,
                weight: 2,
            },
            KeyEntry {
                key: LogicalKey::F12,
                weight: 1,
            },
            KeyEntry {
                key: LogicalKey::ArrowUp,
                weight: 4,
            },
            KeyEntry {
                key: LogicalKey::Space,
                weight: 5,
            },
            KeyEntry {
                key: LogicalKey::Backquote,
                weight: 1,
            },
        ]
    );
    assert_eq!(config.timer.interval_ms, 120);
    assert_eq!(config.natural.naturalness, 65);
    assert_eq!(config.stop_after, Some(3_600));
    assert_eq!(config.global_shortcut, "CommandOrControl+Shift+K");
    assert_eq!(serde_json::to_string(&config).unwrap(), fixture.trim());
}

#[test]
fn rejects_duplicate_keys_and_out_of_range_duration() {
    let mut config = AppConfig::default();
    config.keys = vec![
        KeyEntry::new(LogicalKey::KeyA),
        KeyEntry::new(LogicalKey::KeyA),
    ];
    config.stop_after = Some(86_401);

    let errors = config.validate();

    assert!(
        errors
            .iter()
            .any(|error| error.field == "keys" && error.code == "duplicate")
    );
    assert!(
        errors
            .iter()
            .any(|error| error.field == "stopAfter" && error.code == "range")
    );
}

#[test]
fn corrupt_json_is_preserved_before_defaults_are_loaded() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("config.json"), "{not-json").unwrap();

    let loaded = ConfigRepository::new(dir.path()).load().unwrap();

    assert_eq!(loaded.config, AppConfig::default());
    assert_eq!(loaded.notice.unwrap().code, "corrupt-config-recovered");
    let backups = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
        .count();
    assert_eq!(backups, 1);
}

#[test]
fn load_discards_unrecognized_run_state() {
    let dir = tempfile::tempdir().unwrap();
    let with_run_state = r#"{
        "schemaVersion": 1,
        "keys": [],
        "mode": "timer",
        "timer": { "intervalMs": 100 },
        "natural": { "naturalness": 50, "advanced": null },
        "stopAfter": null,
        "globalShortcut": "CommandOrControl+Shift+K",
        "runState": { "isRunning": true }
    }"#;
    std::fs::write(dir.path().join("config.json"), with_run_state).unwrap();

    let loaded = ConfigRepository::new(dir.path()).load().unwrap();

    assert_eq!(loaded.config, AppConfig::default());
    assert!(loaded.notice.is_none());
    assert!(
        !serde_json::to_string(&loaded.config)
            .unwrap()
            .contains("runState")
    );
}
