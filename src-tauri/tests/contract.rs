use aqlicker_lib::{
    AppConfig, ConfigRepository, DEFAULT_PRESET_ID, DEFAULT_PRESET_NAME, KeyEntry, LogicalKey,
    Mode, Preset, TargetApp, migrate_to_current,
};
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
fn contract_deserializes_the_v5_fixture_with_camel_case_fields() {
    let fixture = include_str!("fixtures/config-v5.json");
    let config: AppConfig = serde_json::from_str(fixture).unwrap();

    assert_eq!(config.schema_version, 5);
    assert_eq!(config.global_shortcut, "CommandOrControl+Shift+K");
    assert_eq!(
        config.preset_cycle_shortcut.as_deref(),
        Some("CommandOrControl+Shift+P")
    );
    assert_eq!(config.active_preset_id, "preset-grinding");
    assert_eq!(
        config.presets,
        vec![
            Preset {
                id: "default".to_owned(),
                name: "Default".to_owned(),
                keys: vec![
                    KeyEntry {
                        key: LogicalKey::KeyA,
                        weight: 3,
                        cooldown_ms: 0,
                    },
                    KeyEntry {
                        key: LogicalKey::Digit1,
                        weight: 2,
                        cooldown_ms: 250,
                    },
                ],
                ..Preset::default()
            },
            Preset {
                id: "preset-grinding".to_owned(),
                name: "Grinding".to_owned(),
                keys: vec![
                    KeyEntry {
                        key: LogicalKey::F12,
                        weight: 1,
                        cooldown_ms: 60_000,
                    },
                    KeyEntry {
                        key: LogicalKey::ArrowUp,
                        weight: 4,
                        cooldown_ms: 0,
                    },
                    KeyEntry {
                        key: LogicalKey::Space,
                        weight: 5,
                        cooldown_ms: 1_500,
                    },
                    KeyEntry {
                        key: LogicalKey::Backquote,
                        weight: 1,
                        cooldown_ms: 0,
                    },
                ],
                mode: Mode::Natural,
                timer: aqlicker_lib::TimerConfig { interval_ms: 120 },
                natural: aqlicker_lib::NaturalConfig {
                    naturalness: 65,
                    advanced: Some(aqlicker_lib::NaturalOverrides {
                        min_interval_ms: 80,
                        max_interval_ms: 400,
                        burst_intensity: 35,
                        pause_chance_percent: 10,
                    }),
                },
                stop_after: Some(3_600),
                target_app: Some(TargetApp {
                    id: "com.apple.TextEdit".to_owned(),
                    name: "TextEdit".to_owned(),
                }),
            },
        ]
    );
    assert_eq!(config.active_preset().unwrap().name, "Grinding");
    assert!(config.validate().is_empty());
    assert_eq!(serde_json::to_string(&config).unwrap(), fixture.trim());
}

#[test]
fn contract_migrates_every_earlier_fixture_to_one_default_preset() {
    for (fixture, expected_cooldowns) in [
        (
            include_str!("fixtures/config-v1.json"),
            vec![0, 0, 0, 0, 0, 0],
        ),
        (
            include_str!("fixtures/config-v2.json"),
            vec![0, 0, 0, 0, 0, 0],
        ),
        (
            include_str!("fixtures/config-v3.json"),
            vec![0, 250, 60_000, 0, 1_500, 0],
        ),
    ] {
        let document = serde_json::from_str(fixture).unwrap();

        let config = migrate_to_current(document).unwrap();

        assert_eq!(config.schema_version, 5);
        assert_eq!(config.global_shortcut, "CommandOrControl+Shift+K");
        // Upgrading never assigns a cycle shortcut: it would steal a hotkey the
        // user may already be using in another application.
        assert_eq!(config.preset_cycle_shortcut, None);
        assert_eq!(config.active_preset_id, DEFAULT_PRESET_ID);
        assert_eq!(config.presets.len(), 1);
        let preset = config.active_preset().unwrap();
        assert_eq!(preset.id, DEFAULT_PRESET_ID);
        assert_eq!(preset.name, DEFAULT_PRESET_NAME);
        assert_eq!(preset.mode, Mode::Natural);
        assert_eq!(preset.timer.interval_ms, 120);
        assert_eq!(preset.natural.naturalness, 65);
        assert_eq!(preset.stop_after, Some(3_600));
        assert_eq!(
            preset
                .keys
                .iter()
                .map(|entry| entry.cooldown_ms)
                .collect::<Vec<_>>(),
            expected_cooldowns
        );
        assert!(config.validate().is_empty());
    }

    // v1 predates the target application, so only v2 and v3 carry one.
    let target_of = |fixture: &str| {
        migrate_to_current(serde_json::from_str(fixture).unwrap())
            .unwrap()
            .presets
            .swap_remove(0)
            .target_app
    };
    assert_eq!(target_of(include_str!("fixtures/config-v1.json")), None);
    for fixture in [
        include_str!("fixtures/config-v2.json"),
        include_str!("fixtures/config-v3.json"),
    ] {
        assert_eq!(
            target_of(fixture),
            Some(TargetApp {
                id: "com.apple.TextEdit".to_owned(),
                name: "TextEdit".to_owned(),
            })
        );
    }
}

#[test]
fn contract_migrates_a_v4_file_keeping_its_presets_and_no_cycle_shortcut() {
    let document = serde_json::from_str(include_str!("fixtures/config-v4.json")).unwrap();

    let config = migrate_to_current(document).unwrap();

    assert_eq!(config.schema_version, 5);
    assert_eq!(config.preset_cycle_shortcut, None);
    assert_eq!(config.global_shortcut, "CommandOrControl+Shift+K");
    assert_eq!(config.active_preset_id, "preset-grinding");
    assert_eq!(
        config
            .presets
            .iter()
            .map(|preset| preset.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Default", "Grinding"]
    );
    assert!(config.validate().is_empty());
}

#[test]
fn rejects_duplicate_keys_and_out_of_range_duration_naming_the_preset() {
    let config = AppConfig {
        active_preset_id: "second".to_owned(),
        presets: vec![
            Preset::default(),
            Preset {
                id: "second".to_owned(),
                name: "Second".to_owned(),
                keys: vec![
                    KeyEntry::new(LogicalKey::KeyA),
                    KeyEntry::new(LogicalKey::KeyA),
                ],
                stop_after: Some(86_401),
                ..Preset::default()
            },
        ],
        ..AppConfig::default()
    };

    let errors = config.validate();

    assert!(
        errors
            .iter()
            .any(|error| error.field == "presets[1].keys" && error.code == "duplicate")
    );
    assert!(
        errors
            .iter()
            .any(|error| error.field == "presets[1].stopAfter" && error.code == "range")
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
        "schemaVersion": 3,
        "keys": [],
        "mode": "timer",
        "timer": { "intervalMs": 100 },
        "natural": { "naturalness": 50, "advanced": null },
        "stopAfter": null,
        "globalShortcut": "CommandOrControl+Shift+K",
        "runState": { "isRunning": true }
    }"#;
    // A v3 body of nothing but defaults must migrate to exactly the default v4
    // document, which pins the migrated preset's id and name.
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
