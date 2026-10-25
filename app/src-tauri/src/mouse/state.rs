//! App-wide state and the operations that change it.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::{Action, Config};
use super::{device, exec, hotkeys, input};
use crate::tray;

pub struct Shared {
    pub core: Mutex<Core>,
    pub status: Mutex<device::Status>,
    pub device_tx: Sender<device::Cmd>,
    pub exec_tx: Sender<exec::Msg>,
    pub config_path: PathBuf,
    pub hotkey_errors: Mutex<Vec<String>>,
}

pub struct Core {
    pub config: Config,
    /// DPI stage currently in use (changed by DPI cycle buttons).
    pub dpi_stage: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub config: Config,
    pub dpi_stage: usize,
    pub device: device::Status,
    pub hotkey_errors: Vec<String>,
    pub config_path: String,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let config_path = app.path().app_config_dir()?.join("mouse.json");
    let config = Config::load(&config_path);
    let dpi_stage = config.active_profile().dpi_index;
    // Threads are started after `manage` so they can always reach the state.
    let (device_tx, device_rx) = std::sync::mpsc::channel();
    let (exec_tx, exec_rx) = std::sync::mpsc::channel();
    app.manage(Shared {
        core: Mutex::new(Core { config, dpi_stage }),
        status: Mutex::new(device::Status::default()),
        device_tx,
        exec_tx: exec_tx.clone(),
        config_path,
        hotkey_errors: Mutex::new(vec![]),
    });
    device::spawn(app.clone(), device_rx);
    exec::spawn(app.clone(), exec_rx);
    input::start(exec_tx);
    apply_active(app);
    hotkeys::register(app);
    Ok(())
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let shared = app.state::<Shared>();
    let (config, dpi_stage) = {
        let core = shared.core.lock().unwrap();
        (core.config.clone(), core.dpi_stage)
    };
    let device = shared.status.lock().unwrap().clone();
    let hotkey_errors = shared.hotkey_errors.lock().unwrap().clone();
    UiState { config, dpi_stage, device, hotkey_errors, config_path: shared.config_path.display().to_string() }
}

pub fn emit_state(app: &AppHandle) {
    let _ = app.emit("mouse-state", ui_state(app));
}

/// Push the active profile's settings to the mouse and the input hook.
fn apply_active(app: &AppHandle) {
    let shared = app.state::<Shared>();
    let core = shared.core.lock().unwrap();
    let p = core.config.active_profile();
    let dpi = p.dpi_stages[core.dpi_stage.min(p.dpi_stages.len() - 1)];
    let _ = shared.device_tx.send(device::Cmd::Apply { dpi, rate: p.report_rate });
    input::set_intercept(p.back != Action::Default, p.forward != Action::Default);
}

fn save(shared: &Shared, config: &Config) {
    if let Err(e) = config.save(&shared.config_path) {
        eprintln!("failed to save config: {e}");
    }
}

pub fn activate(app: &AppHandle, id: &str) {
    {
        let shared = app.state::<Shared>();
        let mut core = shared.core.lock().unwrap();
        let Some(index) = core.config.profile(id).map(|p| p.dpi_index) else { return };
        core.config.active = id.to_string();
        core.dpi_stage = index;
        save(&shared, &core.config);
        let _ = shared.exec_tx.send(exec::Msg::CancelAll);
    }
    apply_active(app);
    tray::refresh(app);
    emit_state(app);
}

pub fn next_profile(app: &AppHandle) {
    let next = {
        let shared = app.state::<Shared>();
        let core = shared.core.lock().unwrap();
        let profiles = &core.config.profiles;
        let i = profiles.iter().position(|p| p.id == core.config.active).unwrap_or(0);
        profiles[(i + 1) % profiles.len()].id.clone()
    };
    activate(app, &next);
}

pub fn cycle_dpi(app: &AppHandle) {
    {
        let shared = app.state::<Shared>();
        let mut core = shared.core.lock().unwrap();
        let len = core.config.active_profile().dpi_stages.len();
        core.dpi_stage = (core.dpi_stage + 1) % len;
    }
    apply_active(app);
    emit_state(app);
}

/// Replace the whole config (the UI saves this way) and apply what changed.
pub fn update_config(app: &AppHandle, mut config: Config) {
    let (menu_changed, macros_changed) = {
        let shared = app.state::<Shared>();
        let mut core = shared.core.lock().unwrap();
        // Only `activate` switches profiles. A save sent just before a hotkey
        // or side button switched must not switch back.
        config.active = core.config.active.clone();
        config.normalize();
        let menu_changed = tray_entries(&core.config) != tray_entries(&config);
        let old = core.config.active_profile().clone();
        let new = config.active_profile();
        if old.id != new.id || old.dpi_stages != new.dpi_stages || old.dpi_index != new.dpi_index {
            core.dpi_stage = new.dpi_index;
        }
        let macros_changed = core.config.macros != config.macros;
        if macros_changed {
            let _ = shared.exec_tx.send(exec::Msg::CancelAll);
        }
        core.config = config;
        save(&shared, &core.config);
        (menu_changed, macros_changed)
    };
    apply_active(app);
    hotkeys::register(app);
    if menu_changed {
        tray::refresh(app);
    }
    // Keyboards bind and store these macros too.
    if macros_changed {
        crate::keyboard::state::macros_changed(app);
    }
}

/// What the tray menu shows: the active profile and every profile's name.
fn tray_entries(config: &Config) -> (&str, Vec<(&str, &str)>) {
    (&config.active, config.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect())
}
