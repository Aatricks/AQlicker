pub mod config;
pub mod input;
pub mod key;
pub mod persistence;
pub mod run;
pub mod scheduler;

pub use config::{
    AppConfig, CURRENT_SCHEMA_VERSION, KeyEntry, Mode, NaturalConfig, NaturalOverrides,
    TimerConfig, ValidationError,
};
pub use input::{EnigoInputSink, InputFailure, InputSink};
pub use key::LogicalKey;
pub use persistence::{
    ConfigRepository, ConfigRepositoryError, LoadedConfig, RecoveryNotice, migrate_to_current,
};
pub use run::{RunController, RunError, RunSnapshot, RunStatus, StartError, StopReason};

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .run(tauri::generate_context!())
        .expect("error while running AQlicker application");
}
