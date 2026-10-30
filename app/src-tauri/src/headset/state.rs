//! App-wide state and the operations that change it.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::{Band, Config, Preset};
use super::{audio, device, mic_test};
use crate::tray;

pub struct Shared {
    pub core: Mutex<Config>,
    pub headset: Mutex<device::Status>,
    pub audio: Mutex<audio::Status>,
    pub mic_test: mic_test::MicTest,
    pub headset_tx: Sender<device::Cmd>,
    pub audio_tx: Sender<audio::Cmd>,
    pub config_path: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub config: Config,
    pub headset: device::Status,
    pub audio: audio::Status,
    pub config_path: String,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let config_path = app.path().app_config_dir()?.join("headset.json");
    let config = Config::load(&config_path);
    // Threads are started after `manage` so they can always reach the state.
    let (headset_tx, headset_rx) = std::sync::mpsc::channel();
    let (audio_tx, audio_rx) = std::sync::mpsc::channel();
    app.manage(Shared {
        core: Mutex::new(config),
        headset: Mutex::new(device::Status::default()),
        audio: Mutex::new(audio::Status::default()),
        mic_test: mic_test::MicTest::default(),
        headset_tx,
        audio_tx,
        config_path,
    });
    device::spawn(app.clone(), headset_rx);
    audio::spawn(app.clone(), audio_rx);
    apply(app);
    Ok(())
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap().clone();
    let headset = shared.headset.lock().unwrap().clone();
    let audio = shared.audio.lock().unwrap().clone();
    UiState { config, headset, audio, config_path: shared.config_path.display().to_string() }
}

pub fn emit_state(app: &AppHandle) {
    let _ = app.emit("headset-state", ui_state(app));
}

/// Push the desired settings to the headset and the mic APO.
fn apply(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap().clone();
    let eq = config.active_preset().to_eq(config.headset.bass);
    let _ = shared.headset_tx.send(device::Cmd::Apply { eq, settings: config.headset.clone() });
    // Fails harmlessly until the APO is installed (the key doesn't exist yet).
    let _ = sysaudio::apo::set_mic_settings(config.mic.denoise, config.mic.strength as u32);
}

fn save(shared: &Shared, config: &Config) {
    if let Err(e) = config.save(&shared.config_path) {
        eprintln!("failed to save config: {e}");
    }
}

pub fn activate(app: &AppHandle, id: &str) {
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        if !config.presets.iter().any(|p| p.id == id) {
            return;
        }
        config.active = id.to_string();
        save(&shared, &config);
    }
    apply(app);
    tray::refresh(app);
    emit_state(app);
}

pub fn set_denoise(app: &AppHandle, on: bool) {
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        config.mic.denoise = on;
        save(&shared, &config);
    }
    apply(app);
    tray::refresh(app);
    emit_state(app);
}

/// Replace the whole config (the UI saves this way) and apply what changed.
pub fn update_config(app: &AppHandle, mut config: Config) {
    let (menu_changed, keep_default_on) = {
        let shared = app.state::<Shared>();
        let mut core = shared.core.lock().unwrap();
        // Only `activate` switches presets, so a save racing a tray click can't undo it.
        config.active = core.active.clone();
        config.seeds = core.seeds;
        config.normalize();
        let menu_changed = tray_entries(&core) != tray_entries(&config) || core.mic.denoise != config.mic.denoise;
        let keep_default_on = config.keep_default && !core.keep_default;
        *core = config;
        save(&shared, &core);
        (menu_changed, keep_default_on)
    };
    apply(app);
    if keep_default_on {
        let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::UseHeadset);
    }
    if menu_changed {
        tray::refresh(app);
    }
}

/// Send an EQ to the headset without saving it (live preview while dragging).
pub fn preview_eq(app: &AppHandle, bands: Vec<Band>, bass: f32) {
    let preset = Preset { bands, ..Preset::default() };
    let mut config = Config { presets: vec![preset], ..Config::default() };
    config.headset.bass = bass;
    config.normalize();
    let eq = config.presets[0].to_eq(config.headset.bass);
    let _ = app.state::<Shared>().headset_tx.send(device::Cmd::PreviewEq(eq));
}

/// What the tray menu shows: the active preset and every preset's name.
fn tray_entries(config: &Config) -> (&str, Vec<(&str, &str)>) {
    (&config.active, config.presets.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect())
}
