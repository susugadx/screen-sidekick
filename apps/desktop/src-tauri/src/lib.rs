#![forbid(unsafe_code)]

pub mod bridge;

#[cfg(feature = "tauri-app")]
use bridge::{BridgeRuntime, BridgeStatus};
#[cfg(feature = "tauri-app")]
use tauri::Manager;

#[cfg(feature = "tauri-app")]
#[derive(Debug, Clone)]
pub struct AppState {
    bridge_status: BridgeStatus,
}

#[cfg(feature = "tauri-app")]
impl AppState {
    #[must_use]
    pub fn new(bridge_status: BridgeStatus) -> Self {
        Self { bridge_status }
    }

    #[must_use]
    pub fn bridge_status(&self) -> BridgeStatus {
        self.bridge_status.clone()
    }
}

#[cfg(feature = "tauri-app")]
#[tauri::command]
fn get_bridge_status(state: tauri::State<'_, AppState>) -> BridgeStatus {
    state.bridge_status()
}

#[cfg(feature = "tauri-app")]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_bridge_status])
        .setup(|app| {
            let (bridge_runtime, bridge_status) = BridgeRuntime::start()?;
            app.manage(AppState::new(bridge_status));
            app.manage(bridge_runtime);
            Ok(())
        })
        .run(tauri::generate_context!())
}

#[cfg(not(feature = "tauri-app"))]
pub fn run() -> Result<(), &'static str> {
    Err("screen-sidekick desktop was built without the tauri-app feature")
}
