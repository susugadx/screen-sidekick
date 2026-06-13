#![forbid(unsafe_code)]

pub mod bridge;

#[cfg(feature = "tauri-app")]
use screen_sidekick_sidekick_daemon::{DaemonRuntime, DaemonStatus};
#[cfg(feature = "tauri-app")]
use tauri::Manager;

#[cfg(feature = "tauri-app")]
#[derive(Debug, Clone)]
pub struct AppState {
    daemon_status: DaemonStatus,
}

#[cfg(feature = "tauri-app")]
impl AppState {
    #[must_use]
    pub fn new(daemon_status: DaemonStatus) -> Self {
        Self { daemon_status }
    }

    #[must_use]
    pub fn daemon_status(&self) -> DaemonStatus {
        self.daemon_status.clone()
    }
}

#[cfg(feature = "tauri-app")]
#[tauri::command]
fn get_daemon_status(state: tauri::State<'_, AppState>) -> DaemonStatus {
    state.daemon_status()
}

#[cfg(feature = "tauri-app")]
#[tauri::command]
fn get_bridge_status(state: tauri::State<'_, AppState>) -> DaemonStatus {
    state.daemon_status()
}

#[cfg(feature = "tauri-app")]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_daemon_status,
            get_bridge_status
        ])
        .setup(|app| {
            let (daemon_runtime, daemon_status) = DaemonRuntime::start()?;
            app.manage(AppState::new(daemon_status));
            app.manage(daemon_runtime);
            Ok(())
        })
        .run(tauri::generate_context!())
}

#[cfg(not(feature = "tauri-app"))]
pub fn run() -> Result<(), &'static str> {
    Err("screen-sidekick desktop was built without the tauri-app feature")
}
