#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod device;
mod exec;
mod hotkeys;
mod input;
mod state;
mod tray;

use std::time::Duration;

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use config::{Config, KeyRef, Step};
use state::UiState;

const HIDDEN_ARG: &str = "--minimized";

/// Show the settings window, creating it if it was closed. Closing destroys
/// the webview so only the small tray process stays resident.
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("lightswitch")
        .inner_size(1180.0, 780.0)
        .min_inner_size(940.0, 640.0)
        // Frameless: the UI draws its own title bar (drag region + minimize/close).
        .decorations(false)
        .maximizable(false)
        .shadow(true)
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::window::Color(8, 9, 12, 255))
        .build();
}

#[tauri::command]
fn get_state(app: AppHandle) -> UiState {
    state::ui_state(&app)
}

#[tauri::command]
fn save_config(app: AppHandle, config: Config) -> UiState {
    state::update_config(&app, config);
    state::ui_state(&app)
}

#[tauri::command]
fn activate_profile(app: AppHandle, id: String) -> UiState {
    state::activate(&app, &id);
    state::ui_state(&app)
}

#[tauri::command]
fn test_macro(app: AppHandle, id: String) {
    let _ = app.state::<state::Shared>().exec_tx.send(exec::Msg::Test { id });
}

#[tauri::command]
fn start_recording() {
    input::start_recording();
}

#[tauri::command]
fn stop_recording() -> Vec<Step> {
    input::stop_recording()
}

/// Wait for the user to press a key combination. Runs off the main thread.
#[tauri::command(async)]
fn capture_keys() -> Option<Vec<KeyRef>> {
    let rx = input::capture_combo();
    let keys = rx.recv_timeout(Duration::from_secs(15)).ok();
    if keys.is_none() {
        input::cancel_capture();
    }
    keys
}

/// Key events from the settings window while recording or capturing.
#[tauri::command]
fn ui_key(code: String, down: bool) {
    input::ui_key(&code, down);
}

#[tauri::command]
fn cancel_capture() {
    input::cancel_capture();
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(false))
}

fn main() {
    let start_hidden = std::env::args().any(|a| a == HIDDEN_ARG);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
        .plugin(hotkeys::plugin())
        .setup(move |app| {
            let handle = app.handle();
            state::init(handle)?;
            tray::create(handle)?;
            if !start_hidden {
                show_window(handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_config,
            activate_profile,
            test_macro,
            start_recording,
            stop_recording,
            capture_keys,
            ui_key,
            cancel_capture,
            get_autostart,
            set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build lightswitch")
        .run(|_app, event| {
            // Keep running in the tray when the last window closes; only Quit exits.
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}
