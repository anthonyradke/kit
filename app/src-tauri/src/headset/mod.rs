//! The SteelSeries Arctis Nova 5: onboard EQ, mic noise suppression and the default-device keeper.

pub mod audio;
mod config;
mod device;
pub mod elevate;
pub mod state;

use tauri::{AppHandle, Manager};

use config::{Band, Config};
use state::{Shared, UiState};

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    state::init(app)
}

#[tauri::command]
pub fn headset_state(app: AppHandle) -> UiState {
    state::ui_state(&app)
}

#[tauri::command]
pub fn headset_save(app: AppHandle, config: Config) -> UiState {
    state::update_config(&app, config);
    state::ui_state(&app)
}

#[tauri::command]
pub fn activate_preset(app: AppHandle, id: String) -> UiState {
    state::activate(&app, &id);
    state::ui_state(&app)
}

#[tauri::command]
pub fn preview_eq(app: AppHandle, bands: Vec<Band>) {
    state::preview_eq(&app, bands);
}

#[tauri::command]
pub fn use_headset(app: AppHandle) {
    let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::UseHeadset);
}

/// Install or remove the mic noise suppression APO (one UAC prompt).
#[tauri::command(async)]
pub fn set_apo_installed(app: AppHandle, install: bool) -> Result<UiState, String> {
    let result = if install {
        let dll = elevate::apo_dll().ok_or("kit_apo.dll is missing from the install folder.")?;
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
pub fn apo_log() -> String {
    let text = std::fs::read_to_string(sysaudio::apo::log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}
