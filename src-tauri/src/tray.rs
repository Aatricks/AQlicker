//! What the menu bar item should contain for a given state, as plain data.
//!
//! Everything decidable lives here so it can be tested without a menu. The
//! Tauri glue in `lib.rs` only turns a `TrayModel` into menu items and turns a
//! clicked item id back into an action.

use crate::{config::AppConfig, run::RunStatus};

pub const RUN_ITEM_ID: &str = "aqlicker-run";
pub const SHOW_ITEM_ID: &str = "aqlicker-show";
pub const QUIT_ITEM_ID: &str = "aqlicker-quit";
const PRESET_ITEM_PREFIX: &str = "aqlicker-preset:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    pub run_label: &'static str,
    pub presets: Vec<TrayPreset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayPreset {
    pub id: String,
    pub label: String,
    pub active: bool,
    /// A run locks the whole configuration, so switching presets is refused
    /// while one is active. Start/Stop and Quit stay available.
    pub enabled: bool,
}

/// A run holds the whole configuration locked while it is running or stopping.
/// Both the menu bar item and the service read the lock from here, so they
/// cannot drift apart.
pub const fn run_is_active(status: RunStatus) -> bool {
    matches!(status, RunStatus::Running | RunStatus::Stopping)
}

pub fn tray_model(config: Option<&AppConfig>, running: bool) -> TrayModel {
    TrayModel {
        run_label: if running { "Stop" } else { "Start" },
        presets: config.map_or_else(Vec::new, |config| {
            config
                .presets
                .iter()
                .enumerate()
                .map(|(index, preset)| TrayPreset {
                    id: preset.id.clone(),
                    label: match preset.name.trim() {
                        "" => format!("Preset {}", index + 1),
                        name => name.to_owned(),
                    },
                    active: preset.id == config.active_preset_id,
                    enabled: !running,
                })
                .collect()
        }),
    }
}

pub fn preset_item_id(preset_id: &str) -> String {
    format!("{PRESET_ITEM_PREFIX}{preset_id}")
}

pub fn preset_id_from_item(item_id: &str) -> Option<&str> {
    item_id.strip_prefix(PRESET_ITEM_PREFIX)
}

pub const TRAY_ID: &str = "aqlicker-tray";

/// Rebuilds the whole menu on the main thread and returns at once. Menu items
/// must be created on the event loop on macOS, and this is the only place that
/// touches it: the finished model is moved in by value, so nothing on the main
/// thread ever waits on a lock or on the service dispatcher.
pub fn apply(app: &tauri::AppHandle, model: TrayModel) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = handle.tray_by_id(TRAY_ID) else {
            return;
        };
        if let Ok(menu) = build_menu(&handle, &model) {
            let _ = tray.set_menu(Some(menu));
        }
    });
}

pub fn build_menu(
    app: &tauri::AppHandle,
    model: &TrayModel,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};

    let run = MenuItemBuilder::with_id(RUN_ITEM_ID, model.run_label).build(app)?;
    let mut builder = MenuBuilder::new(app).item(&run).separator();
    let presets: Vec<_> = model
        .presets
        .iter()
        .map(|preset| {
            CheckMenuItemBuilder::with_id(preset_item_id(&preset.id), &preset.label)
                .checked(preset.active)
                .enabled(preset.enabled)
                .build(app)
        })
        .collect::<tauri::Result<_>>()?;
    for preset in &presets {
        builder = builder.item(preset);
    }
    builder
        .separator()
        .item(&MenuItemBuilder::with_id(SHOW_ITEM_ID, "Show AQlicker").build(app)?)
        .item(&MenuItemBuilder::with_id(QUIT_ITEM_ID, "Quit AQlicker").build(app)?)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, Preset};

    fn preset(id: &str, name: &str) -> Preset {
        Preset {
            id: id.to_owned(),
            name: name.to_owned(),
            ..Preset::default()
        }
    }

    fn config(active: &str) -> AppConfig {
        AppConfig {
            active_preset_id: active.to_owned(),
            presets: vec![
                preset("a", "Fishing"),
                preset("b", "Grinding"),
                preset("c", "  "),
            ],
            ..AppConfig::default()
        }
    }

    #[test]
    fn menu_marks_the_active_preset_and_offers_start_while_idle() {
        // The active preset is deliberately not the first one, so a hard-coded
        // index cannot pass.
        let model = tray_model(Some(&config("b")), false);

        assert_eq!(model.run_label, "Start");
        assert_eq!(
            model
                .presets
                .iter()
                .map(|entry| (entry.id.as_str(), entry.label.as_str(), entry.active))
                .collect::<Vec<_>>(),
            vec![
                ("a", "Fishing", false),
                ("b", "Grinding", true),
                // A blank name still needs something clickable on the menu.
                ("c", "Preset 3", false),
            ]
        );
        assert!(model.presets.iter().all(|entry| entry.enabled));
    }

    #[test]
    fn an_active_run_locks_the_preset_entries_and_offers_stop() {
        let model = tray_model(Some(&config("b")), true);

        assert_eq!(model.run_label, "Stop");
        assert!(model.presets.iter().all(|entry| !entry.enabled));
        // The lock must not hide which preset is running.
        assert_eq!(
            model
                .presets
                .iter()
                .filter(|entry| entry.active)
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
    }

    #[test]
    fn menu_before_the_configuration_loads_lists_no_preset() {
        let model = tray_model(None, false);

        assert_eq!(model.run_label, "Start");
        assert!(model.presets.is_empty());
    }

    #[test]
    fn only_a_running_or_stopping_run_holds_the_lock() {
        // Exhaustive: a new status must be classified deliberately.
        for (status, active) in [
            (RunStatus::Idle, false),
            (RunStatus::Running, true),
            (RunStatus::Stopping, true),
            (RunStatus::Failed, false),
        ] {
            assert_eq!(run_is_active(status), active, "{status:?}");
        }
    }

    #[test]
    fn preset_item_ids_round_trip_and_reject_the_fixed_items() {
        let id = preset_item_id("preset-grinding");

        assert_eq!(preset_id_from_item(&id), Some("preset-grinding"));
        for fixed in [RUN_ITEM_ID, SHOW_ITEM_ID, QUIT_ITEM_ID] {
            assert_eq!(preset_id_from_item(fixed), None, "{fixed} was accepted");
        }
    }
}
