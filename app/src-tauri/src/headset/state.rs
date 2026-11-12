//! App-wide state and the operations that change it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::{Band, Config, Preset};
use super::{audio, device, mic_test, surround};
use crate::tray;

pub struct Shared {
    pub core: Mutex<Config>,
    pub headset: Mutex<device::Status>,
    pub audio: Mutex<audio::Status>,
    pub surround: Mutex<surround::Status>,
    pub mic_test: mic_test::MicTest,
    pub headset_tx: Sender<device::Cmd>,
    pub audio_tx: Sender<audio::Cmd>,
    pub surround_tx: Sender<surround::Cmd>,
    pub config_path: PathBuf,
    /// Where `apply` last put the EQ: kit's APO (true) or the headset.
    /// Previews follow it, so a drag never plays the EQ in both places.
    pub eq_in_kit: AtomicBool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub config: Config,
    pub headset: device::Status,
    pub audio: audio::Status,
    pub surround: surround::Status,
    pub config_path: String,
    /// The EQ runs in kit's APO right now (chosen and installed).
    pub eq_in_kit: bool,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let config_path = app.path().app_config_dir()?.join("headset.json");
    let config = Config::load(&config_path);
    // Threads are started after `manage` so they can always reach the state.
    let (headset_tx, headset_rx) = std::sync::mpsc::channel();
    let (audio_tx, audio_rx) = std::sync::mpsc::channel();
    let (surround_tx, surround_rx) = std::sync::mpsc::channel();
    app.manage(Shared {
        core: Mutex::new(config),
        headset: Mutex::new(device::Status::default()),
        audio: Mutex::new(audio::Status::default()),
        surround: Mutex::new(surround::Status::default()),
        mic_test: mic_test::MicTest::default(),
        headset_tx,
        audio_tx,
        surround_tx,
        config_path,
        eq_in_kit: AtomicBool::new(false),
    });
    device::spawn(app.clone(), headset_rx);
    audio::spawn(app.clone(), audio_rx);
    surround::spawn(app.clone(), surround_rx);
    apply(app);
    Ok(())
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap().clone();
    let headset = shared.headset.lock().unwrap().clone();
    let audio = shared.audio.lock().unwrap().clone();
    let surround = shared.surround.lock().unwrap().clone();
    let eq_in_kit = shared.eq_in_kit.load(Ordering::Relaxed);
    UiState { config, headset, audio, surround, config_path: shared.config_path.display().to_string(), eq_in_kit }
}

pub fn emit_state(app: &AppHandle) {
    let _ = app.emit("headset-state", ui_state(app));
}

/// Push the desired settings to the headset and kit's APOs.
pub fn apply(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap().clone();
    let preset = config.active_preset();
    // With the EQ in kit, the headset's own EQ stays flat so the two never stack.
    let in_kit = kit_eq_ready(app, &config);
    shared.eq_in_kit.store(in_kit, Ordering::Relaxed);
    let eq = if in_kit { [None; arctis::BANDS] } else { preset.to_eq(config.headset.bass) };
    let _ = shared.headset_tx.send(device::Cmd::Apply { eq, settings: config.headset.clone() });
    let curve = preset.to_curve(config.headset.bass, config.headset.preamp);
    // Fails harmlessly until an APO is installed (the key doesn't exist yet).
    let _ = sysaudio::apo::set_headset_eq(in_kit, &curve.to_string());
    let _ = sysaudio::apo::set_mic_settings(config.mic.denoise, config.mic.strength as u32);
    // Surround on or off moves the Windows defaults and starts or stops the stream.
    let _ = shared.audio_tx.send(audio::Cmd::Refresh);
    let _ = shared.surround_tx.send(surround::Cmd::Refresh);
}

fn save(shared: &Shared, config: &Config) {
    if let Err(e) = config.save(&shared.config_path) {
        log::error!("failed to save headset.json: {e}");
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

pub fn set_surround(app: &AppHandle, on: bool) {
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        config.headset.surround = on;
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
        let menu_changed = tray_entries(&core) != tray_entries(&config)
            || core.mic.denoise != config.mic.denoise
            || core.headset.surround != config.headset.surround;
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

/// Take in a settings file: its config, or what it adds to the one here.
pub fn import(app: &AppHandle, mut config: Config, replace: bool) {
    let active = config.active.clone();
    if !replace {
        let mut merged = app.state::<Shared>().core.lock().unwrap().clone();
        merged.add_from(config);
        config = merged;
    }
    update_config(app, config);
    // `update_config` leaves the active preset alone.
    if replace {
        activate(app, &active);
    }
    emit_state(app);
}

/// Play an EQ without saving it (live preview while dragging): on the
/// headset, or in kit's EQ when that's where the EQ runs.
pub fn preview_eq(app: &AppHandle, bands: Vec<Band>, bass: f32) {
    let shared = app.state::<Shared>();
    let current = shared.core.lock().unwrap().clone();
    let preset = Preset { bands, ..Preset::default() };
    let mut config = Config { presets: vec![preset], ..Config::default() };
    config.headset.bass = bass;
    config.normalize();
    // Where `apply` put the EQ: this runs many times a second while dragging.
    if shared.eq_in_kit.load(Ordering::Relaxed) {
        let curve = config.presets[0].to_curve(config.headset.bass, current.headset.preamp);
        let _ = sysaudio::apo::set_headset_eq(true, &curve.to_string());
    } else {
        let eq = config.presets[0].to_eq(config.headset.bass);
        let _ = shared.headset_tx.send(device::Cmd::PreviewEq(eq));
    }
}

/// The EQ runs in kit: chosen, and kit's EQ is installed on the headset.
/// From the audio thread's last scan once there is one (it applies again
/// whenever that changes); before it, looked up now so the first settings
/// pushed at startup are right.
fn kit_eq_ready(app: &AppHandle, config: &Config) -> bool {
    if !config.headset.eq_in_kit {
        return false;
    }
    let scanned = app.state::<Shared>().audio.lock().unwrap().clone();
    if scanned.scanned {
        scanned.eq_apo_installed
    } else {
        audio::eq_apo_installed()
    }
}

/// What the tray menu shows: the active preset and every preset's name.
fn tray_entries(config: &Config) -> (&str, Vec<(&str, &str)>) {
    (&config.active, config.presets.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect())
}
