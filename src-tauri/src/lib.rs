use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::Manager;

pub mod commands;
pub mod config;
pub mod focus;
pub mod input;
pub mod key;
pub mod permission;
pub mod persistence;
pub mod run;
pub mod scheduler;
pub mod shortcuts;
pub mod tray;

pub use commands::{
    AppService, BootstrapPayload, CommandError, DesktopRuntime, RunEventEmitter,
    ShortcutRegistrationStatus,
};
pub use config::{
    AppConfig, CURRENT_SCHEMA_VERSION, DEFAULT_PRESET_ID, DEFAULT_PRESET_NAME, KeyEntry,
    MAX_PRESET_NAME_LENGTH, Mode, NaturalConfig, NaturalOverrides, Preset, TargetApp, TimerConfig,
    ValidationError,
};
pub use focus::{FocusProbe, RunningApp};
pub use input::{EnigoInputSink, InputFailure, InputSink};
pub use key::LogicalKey;
pub use permission::{PermissionProvider, PermissionStatus};
pub use persistence::{
    ConfigRepository, ConfigRepositoryError, LoadedConfig, RecoveryNotice, migrate_to_current,
};
pub use run::{
    RunController, RunError, RunObserver, RunSnapshot, RunStatus, StartError, StartOutcome,
    StopReason,
};
pub use shortcuts::{ShortcutAction, ShortcutError};

pub fn run() {
    let shutdown_started = Arc::new(AtomicBool::new(false));
    let shutdown_complete = Arc::new(AtomicBool::new(false));
    let close_started = Arc::clone(&shutdown_started);
    let close_complete = Arc::clone(&shutdown_complete);
    let tray_started = Arc::clone(&shutdown_started);
    let tray_complete = Arc::clone(&shutdown_complete);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            build_tray(app.handle(), &tray_started, &tray_complete)?;
            let shortcut_app = app_handle.clone();
            let registry = shortcuts::TauriShortcutRegistry::new(
                app_handle.clone(),
                Arc::new(move |action| {
                    if let Some(service) = shortcut_app.try_state::<AppService>() {
                        let _ = service.enqueue_shortcut(action);
                    }
                }),
            );
            let service = AppService::new(
                ConfigRepository::new(app.path().app_config_dir()?),
                permission::system_permission_provider(),
                Box::new(shortcuts::ShortcutManager::without_toggle(registry)),
                Box::new(commands::DesktopRuntime::default()),
                Arc::new(commands::TauriRunEventEmitter::new(app_handle)),
            );
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::save_config,
            commands::start_run,
            commands::stop_run,
            commands::request_access,
            commands::permission_status,
            commands::set_shortcut,
            commands::set_cycle_shortcut,
            commands::list_apps,
        ])
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !close_complete.load(Ordering::SeqCst) {
                    api.prevent_close();
                    request_shutdown(
                        window.app_handle().clone(),
                        Arc::clone(&close_started),
                        Arc::clone(&close_complete),
                    );
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building AQlicker application");

    app.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if !shutdown_complete.load(Ordering::SeqCst) {
                api.prevent_exit();
                request_shutdown(
                    app.clone(),
                    Arc::clone(&shutdown_started),
                    Arc::clone(&shutdown_complete),
                );
            }
        }
    });
}

/// Every handler here runs on the main thread, which the service dispatcher
/// needs in order to register global shortcuts. So each one only enqueues work
/// or spawns a thread and returns: none of them ever waits for a reply.
fn build_tray(
    app: &tauri::AppHandle,
    started: &Arc<AtomicBool>,
    complete: &Arc<AtomicBool>,
) -> tauri::Result<()> {
    let started = Arc::clone(started);
    let complete = Arc::clone(complete);
    let menu = tray::build_menu(app, &tray::tray_model(None, false))?;
    tauri::tray::TrayIconBuilder::with_id(tray::TRAY_ID)
        // Alpha-only, so macOS tints it for a light or a dark menu bar.
        .icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/tray-template.png"
        ))?)
        .icon_as_template(true)
        .tooltip("AQlicker")
        .menu(&menu)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            if id == tray::QUIT_ITEM_ID {
                request_shutdown(app.clone(), Arc::clone(&started), Arc::clone(&complete));
                return;
            }
            if id == tray::SHOW_ITEM_ID {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.unminimize();
                    let _ = window.set_focus();
                }
                return;
            }
            let Some(service) = app.try_state::<AppService>() else {
                return;
            };
            if id == tray::RUN_ITEM_ID {
                let _ = service.enqueue_shortcut(ShortcutAction::ToggleRun);
            } else if let Some(preset_id) = tray::preset_id_from_item(id) {
                let _ = service.enqueue_select_preset(preset_id.to_owned());
            }
        })
        .build(app)?;
    Ok(())
}

fn request_shutdown(app: tauri::AppHandle, started: Arc<AtomicBool>, complete: Arc<AtomicBool>) {
    if started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || {
        let result = app
            .try_state::<AppService>()
            .map_or(Ok(()), |service| service.shutdown());
        if result.is_ok() {
            complete.store(true, Ordering::SeqCst);
            app.exit(0);
        } else {
            started.store(false, Ordering::SeqCst);
        }
    });
}
