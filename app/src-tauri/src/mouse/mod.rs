//! The Logitech PRO X Superlight: profiles, DPI, report rate, side buttons and macros.

pub mod config;
mod device;
pub mod exec;
mod hotkeys;
pub mod input;
pub mod state;

use std::time::Duration;

use tauri::{AppHandle, Manager};

use config::{Config, KeyRef, Step};
use state::UiState;

pub use hotkeys::plugin as hotkeys_plugin;

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    state::init(app)
}

/// A closed window can't stop what it started: a recording or key capture
/// would keep side buttons unhandled, a shortcut capture would leave hotkeys off.
pub fn on_window_destroyed(app: &AppHandle) {
    input::reset();
    hotkeys::resume(app);
}

#[tauri::command]
pub fn mouse_state(app: AppHandle) -> UiState {
    state::ui_state(&app)
}

#[tauri::command]
pub fn mouse_save(app: AppHandle, config: Config) -> UiState {
    state::update_config(&app, config);
    state::ui_state(&app)
}

#[tauri::command]
pub fn activate_profile(app: AppHandle, id: String) -> UiState {
    state::activate(&app, &id);
    state::ui_state(&app)
}

#[tauri::command]
pub fn test_macro(app: AppHandle, id: String) {
    let _ = app.state::<state::Shared>().exec_tx.send(exec::Msg::Test { id });
}

#[tauri::command]
pub fn start_recording() {
    input::start_recording();
}

#[tauri::command]
pub fn stop_recording() -> Vec<Step> {
    input::stop_recording()
}

/// Wait for the user to press a key combination. Runs off the main thread.
#[tauri::command(async)]
pub fn capture_keys() -> Option<Vec<KeyRef>> {
    let rx = input::capture_combo();
    let keys = rx.recv_timeout(Duration::from_secs(15)).ok();
    if keys.is_none() {
        input::cancel_capture();
    }
    keys
}

/// Key events from the settings window while recording or capturing.
#[tauri::command]
pub fn ui_key(code: String, down: bool) {
    input::ui_key(&code, down);
}

#[tauri::command]
pub fn cancel_capture() {
    input::cancel_capture();
}

/// Turn global hotkeys off while the UI captures a new one.
#[tauri::command]
pub fn pause_hotkeys(app: AppHandle) {
    hotkeys::pause(&app);
}

#[tauri::command]
pub fn resume_hotkeys(app: AppHandle) {
    hotkeys::resume(&app);
}
