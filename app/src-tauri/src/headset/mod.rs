//! The SteelSeries Arctis Nova 5: EQ (onboard or in kit's APO), mic noise suppression and the default-device keeper.

pub mod audio;
mod chatmix;
pub mod config;
mod device;
pub mod elevate;
pub mod mic_test;
pub mod state;
pub mod surround;

use tauri::{AppHandle, Manager};

use config::{Band, Config};
use state::{Shared, UiState};

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    state::init(app)
}

pub fn on_window_destroyed(app: &AppHandle) {
    mic_test::clear(app);
}

/// Quitting: with surround on nothing would reach the headset without kit,
/// so put the headset back as the Windows default.
pub fn on_exit(app: &AppHandle) {
    let shared = app.state::<Shared>();
    // ChatMix: every app back to full volume.
    let (tx, rx) = std::sync::mpsc::channel();
    if shared.headset_tx.send(device::Cmd::Release(tx)).is_ok() {
        let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
    }
    if shared.core.lock().unwrap().headset.surround {
        audio::restore_headset();
    }
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
pub fn preview_eq(app: AppHandle, bands: Vec<Band>, bass: f32) {
    state::preview_eq(&app, bands, bass);
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

/// Play noise through each virtual surround speaker in turn.
#[tauri::command]
pub fn surround_test(app: AppHandle) {
    let _ = app.state::<Shared>().surround_tx.send(surround::Cmd::Test);
}

/// VB-Audio's page for VB-CABLE, which surround needs.
#[tauri::command]
pub fn open_vbcable_page() {
    use windows::core::{w, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe { ShellExecuteW(None, w!("open"), w!("https://vb-audio.com/Cable/"), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
}

#[tauri::command]
pub fn apo_log() -> String {
    let text = std::fs::read_to_string(sysaudio::apo::log_path()).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}
