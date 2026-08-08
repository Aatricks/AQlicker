use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tauri::Manager;

pub mod commands;
pub mod config;
pub mod input;
pub mod key;
pub mod permission;
pub mod persistence;
pub mod run;
pub mod scheduler;
pub mod shortcuts;

pub use commands::{
    AppService, BootstrapPayload, CommandError, DesktopRuntime, RunEventEmitter,
    ShortcutRegistrationStatus,
};
pub use config::{
    AppConfig, CURRENT_SCHEMA_VERSION, KeyEntry, Mode, NaturalConfig, NaturalOverrides,
    TimerConfig, ValidationError,
};
pub use input::{EnigoInputSink, InputFailure, InputSink};
pub use key::LogicalKey;
pub use permission::{PermissionProvider, PermissionStatus};
pub use persistence::{
    ConfigRepository, ConfigRepositoryError, LoadedConfig, RecoveryNotice, migrate_to_current,
};
pub use run::{
    RunController, RunError, RunObserver, RunSnapshot, RunStatus, StartError, StopReason,
};
pub use shortcuts::{ShortcutAction, ShortcutError};

pub fn run() {
    let shutdown_started = Arc::new(AtomicBool::new(false));
    let shutdown_complete = Arc::new(AtomicBool::new(false));
    let close_started = Arc::clone(&shutdown_started);
    let close_complete = Arc::clone(&shutdown_complete);
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let app_handle = app.handle().clone();
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
