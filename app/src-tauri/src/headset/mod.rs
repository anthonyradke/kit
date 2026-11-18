//! The SteelSeries Arctis Nova 5: the EQ (in kit's APO, the headset's own kept flat), mic noise suppression and the default-device keeper.

pub mod audio;
pub mod config;
mod device;
pub mod elevate;
pub mod mic_test;
pub mod state;

use tauri::{AppHandle, Manager};

use config::{Band, Config, Tone};
use state::{Shared, UiState};

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    state::init(app)
}

pub fn on_window_destroyed(app: &AppHandle) {
    mic_test::clear(app);
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
pub fn preview_eq(app: AppHandle, bands: Vec<Band>, tone: Tone) {
    state::preview_eq(&app, bands, tone);
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
    result.map(|_| state::ui_state(&app))
}

#[derive(serde::Serialize)]
pub struct Repaired {
    state: UiState,
    /// What was fixed, one line each; empty when nothing needed it.
    done: Vec<String>,
}

/// Put kit's audio effects back in working order (one UAC prompt); see
/// `sysaudio::apo::repair`.
#[tauri::command(async)]
pub fn repair_audio_effects(app: AppHandle) -> Result<Repaired, String> {
    let dll = elevate::apo_dll().map(|d| d.to_string_lossy().into_owned());
    let mut args = vec![elevate::REPAIR_ARG];
    if let Some(dll) = &dll {
        args.push(dll);
    }
    let said = elevate::run_elevated(&args)?;
    log::info!("repaired audio effects: {}", if said.is_empty() { "nothing to fix" } else { &said });
    // The settings key may be new again: send the EQ and mic settings.
    state::apply(&app);
    let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::Refresh);
    Ok(Repaired { state: state::ui_state(&app), done: said.lines().map(str::to_owned).collect() })
}

/// Install or remove kit's EQ on the headset output (one UAC prompt).
#[tauri::command(async)]
pub fn set_eq_apo_installed(app: AppHandle, install: bool) -> Result<UiState, String> {
    let result = if install {
        let dll = elevate::apo_dll().ok_or("kit_apo.dll is missing from the install folder.")?;
        elevate::run_elevated(&[elevate::INSTALL_EQ_ARG, &dll.to_string_lossy()])
    } else {
        elevate::run_elevated(&[elevate::UNINSTALL_EQ_ARG])
    };
    // Installed: the EQ moves into kit and the headset's goes flat. Removed:
    // the preset goes back onto the headset.
    state::apply(&app);
    let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::Refresh);
    result.map(|_| state::ui_state(&app))
}

/// Record a mic sample, play it back or listen live; `Idle` stops. Progress
/// comes back in `mic-test` events.
#[tauri::command(async)]
pub fn set_mic_test(app: AppHandle, mode: mic_test::Mode) {
    mic_test::set(&app, mode);
}

#[tauri::command]
pub fn apo_log() -> String {
    let text = std::fs::read_to_string(sysaudio::apo::log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}
