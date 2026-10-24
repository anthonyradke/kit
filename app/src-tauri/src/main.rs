#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod elevate;
mod headset;
mod state;
mod tray;

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use config::{Band, Config};
use state::{Shared, UiState};

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
        .title("headroom")
        .inner_size(1120.0, 760.0)
        .min_inner_size(900.0, 620.0)
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
fn activate_preset(app: AppHandle, id: String) -> UiState {
    state::activate(&app, &id);
    state::ui_state(&app)
}

#[tauri::command]
fn preview_eq(app: AppHandle, bands: Vec<Band>) {
    state::preview_eq(&app, bands);
}

#[tauri::command]
fn use_headset(app: AppHandle) {
    let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::UseHeadset);
}

/// Install or remove the mic noise suppression APO (one UAC prompt).
#[tauri::command(async)]
fn set_apo_installed(app: AppHandle, install: bool) -> Result<UiState, String> {
    let result = if install {
        let dll = elevate::apo_dll().ok_or("headroom_apo.dll is missing from the install folder.")?;
        elevate::run_elevated(&[elevate::INSTALL_ARG, &dll.to_string_lossy()])
    } else {
        elevate::run_elevated(&[elevate::UNINSTALL_ARG])
    };
    if result.is_ok() {
        // The settings key exists now; give the APO our current settings.
        let mic = app.state::<Shared>().core.lock().unwrap().mic.clone();
        let _ = sysaudio::apo::set_mic_settings(mic.denoise, mic.strength as u32);
    }
    let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::Refresh);
    result.map(|()| state::ui_state(&app))
}

#[tauri::command]
fn apo_log() -> String {
    let text = std::fs::read_to_string(sysaudio::apo::log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
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
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = elevate::helper_main(&args) {
        std::process::exit(code);
    }
    let start_hidden = args.iter().any(|a| a == HIDDEN_ARG);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
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
            activate_preset,
            preview_eq,
            use_headset,
            set_apo_installed,
            apo_log,
            get_autostart,
            set_autostart,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build headroom")
        .run(|_app, event| {
            // Keep running in the tray when the last window closes; only Quit exits.
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}
