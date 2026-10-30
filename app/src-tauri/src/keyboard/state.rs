//! App-wide keyboard state and the operations that change it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::config::{from_hue_sat, Config, Model, Preset, KIT_KEYS, NEXT_PROFILE};
use super::device::{self, Cmd};
use crate::mouse::config::{Action, Macro, PlayMode, Step};
use crate::mouse::{hotkeys, input};
use crate::tray;

pub struct Shared {
    pub core: Mutex<Config>,
    pub status: Mutex<device::Status>,
    /// The keyboard chosen in setup.
    pub model: Mutex<Option<Model>>,
    pub tx: Sender<Cmd>,
    pub config_path: PathBuf,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    pub config: Config,
    pub status: device::Status,
    pub config_path: String,
}

pub fn init(app: &AppHandle, model: Option<Model>) -> tauri::Result<()> {
    let config_path = app.path().app_config_dir()?.join("keyboard.json");
    let mut config = Config::load(&config_path);
    if let Some(m) = model {
        if m != config.model {
            config = Config::for_model(m);
        }
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.manage(Shared {
        core: Mutex::new(config),
        status: Mutex::new(device::Status::default()),
        model: Mutex::new(model),
        tx,
        config_path,
    });
    device::spawn(app.clone(), rx, model);
    apply(app, false);
    sync_hook(app);
    hotkeys::register(app);
    Ok(())
}

pub fn ui_state(app: &AppHandle) -> UiState {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap().clone();
    let status = shared.status.lock().unwrap().clone();
    UiState { config, status, config_path: shared.config_path.display().to_string() }
}

pub fn emit_state(app: &AppHandle) {
    let _ = app.emit("keyboard-state", ui_state(app));
}

pub fn model(app: &AppHandle) -> Option<Model> {
    *app.state::<Shared>().model.lock().unwrap()
}

fn save(shared: &Shared, config: &Config) {
    if let Err(e) = config.save(&shared.config_path) {
        eprintln!("failed to save keyboard config: {e}");
    }
}

/// Send the active lighting to the keyboard.
fn apply(app: &AppHandle, store: bool) {
    let shared = app.state::<Shared>();
    let (preset, idle_off) = {
        let config = shared.core.lock().unwrap();
        (config.active_preset().clone(), config.idle_off)
    };
    let _ = shared.tx.send(Cmd::Apply { preset: Box::new(preset), idle_off, save: store });
}

/// Show `preset` without storing it (slider drags on the NK65).
pub fn preview(app: &AppHandle, preset: Preset) {
    let shared = app.state::<Shared>();
    let idle_off = shared.core.lock().unwrap().idle_off;
    let _ = shared.tx.send(Cmd::Apply { preset: Box::new(preset), idle_off, save: false });
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
    apply(app, true);
    tray::refresh(app);
    emit_state(app);
}

/// Make a key profile the active one; the worker puts it on the board.
pub fn activate_profile(app: &AppHandle, id: &str) {
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        if !config.profiles.iter().any(|p| p.id == id) {
            return;
        }
        config.active_profile = id.to_string();
        save(&shared, &config);
        let _ = shared.tx.send(Cmd::SyncKeymap);
    }
    tray::refresh(app);
    emit_state(app);
}

pub fn next_profile(app: &AppHandle) {
    let next = {
        let shared = app.state::<Shared>();
        let config = shared.core.lock().unwrap();
        let i = config.profiles.iter().position(|p| p.id == config.active_profile).unwrap_or(0);
        config.profiles[(i + 1) % config.profiles.len()].id.clone()
    };
    activate_profile(app, &next);
}

/// Setup picked a different keyboard (or none).
pub fn set_model(app: &AppHandle, model: Option<Model>) {
    {
        let shared = app.state::<Shared>();
        *shared.model.lock().unwrap() = model;
        if let Some(m) = model {
            let mut config = shared.core.lock().unwrap();
            if config.model != m {
                *config = Config::for_model(m);
                save(&shared, &config);
            }
        }
        let _ = shared.tx.send(Cmd::Model(model));
    }
    apply(app, true);
    sync_hook(app);
    sync_macros(app);
    hotkeys::register(app);
    tray::refresh(app);
    emit_state(app);
}

/// Replace the whole config (the UI saves this way) and apply what changed.
pub fn update_config(app: &AppHandle, mut config: Config) {
    let exists = macro_exists(app);
    let (lighting_changed, menu_changed) = {
        let shared = app.state::<Shared>();
        let mut core = shared.core.lock().unwrap();
        // Only `activate` switches presets, so a save racing a tray click can't undo it.
        config.active = core.active.clone();
        config.active_profile = core.active_profile.clone();
        config.on_board = core.on_board.clone();
        config.keymap_protocol = core.keymap_protocol;
        config.model = core.model;
        config.adopted = core.adopted;
        // Keymaps change through `set_keys`; this copy may predate a key write.
        for p in &mut config.profiles {
            if let Some(old) = core.profiles.iter().find(|q| q.id == p.id) {
                p.keymap = old.keymap.clone();
            }
        }
        config.normalize();
        config.drop_missing_macros(|id| exists.iter().any(|m| m == id));
        if core.active_key_profile().keymap != config.active_key_profile().keymap {
            let _ = shared.tx.send(Cmd::SyncKeymap);
        }
        let lighting_changed = core.active_preset() != config.active_preset() || core.idle_off != config.idle_off;
        let menu_changed = tray_entries(&core) != tray_entries(&config);
        *core = config;
        save(&shared, &core);
        (lighting_changed, menu_changed)
    };
    if lighting_changed {
        apply(app, true);
    }
    sync_hook(app);
    sync_macros(app);
    hotkeys::register(app);
    if menu_changed {
        tray::refresh(app);
    }
}

/// The first time kit sees an NK65, keep the lighting it already has (set in
/// VIA) as a preset and make it the active one. Returns true if it did.
pub fn adopt_board_lighting(app: &AppHandle, kind: via::Lighting, values: &BTreeMap<u8, [u8; 2]>) -> bool {
    use via::Value;
    let get = |v: Value| values.get(&(v as u8)).copied();
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        if config.adopted {
            return false;
        }
        config.adopted = true;
        let (Some(b), Some(e), Some(sp), Some(c1)) = (get(Value::Brightness), get(Value::Effect), get(Value::Speed), get(Value::Color1)) else {
            save(&shared, &config);
            return false;
        };
        let speed_max = if kind == via::Lighting::Wilba { 3 } else { 255 };
        let mut p = Preset {
            name: "From the keyboard".into(),
            effect: e[0] as u16,
            brightness: ((b[0] as u16 * 100 + 127) / 255) as u8,
            speed: ((sp[0] as u16 * 100 + speed_max / 2) / speed_max).min(100) as u8,
            color1: from_hue_sat(c1),
            color2: get(Value::Color2).map(from_hue_sat).unwrap_or_else(|| from_hue_sat(c1)),
            ..Preset::default()
        };
        let slots = [(Value::CapsOn, Value::CapsColor), (Value::Layer1On, Value::Layer1Color), (Value::Layer2On, Value::Layer2Color), (Value::Layer3On, Value::Layer3Color)];
        for (i, (on, color)) in slots.into_iter().enumerate() {
            if get(on).is_some_and(|v| v[0] != 255) {
                p.indicators[i] = get(color).map(from_hue_sat);
            }
        }
        config.active = p.id.clone();
        config.presets.insert(0, p);
        save(&shared, &config);
    }
    apply(app, false);
    tray::refresh(app);
    emit_state(app);
    true
}

pub fn send(app: &AppHandle, cmd: Cmd) {
    let _ = app.state::<Shared>().tx.send(cmd);
}

/// Change keys of a profile, by keymap index. The active one goes on the board too.
pub fn set_keys(app: &AppHandle, profile: &str, keys: &[(usize, u16)]) {
    let shared = app.state::<Shared>();
    let mut config = shared.core.lock().unwrap();
    let Some(p) = config.profiles.iter_mut().find(|p| p.id == profile) else { return };
    for &(i, code) in keys {
        if let Some(k) = p.keymap.get_mut(i) {
            *k = code;
        }
    }
    save(&shared, &config);
    if config.active_profile == profile {
        let _ = shared.tx.send(Cmd::SyncKeymap);
    }
}

/// Take in the keymap the worker just read from the board.
pub fn board_read(app: &AppHandle, keymap: &[u16], protocol: u16) {
    let changed = {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        let before = config.clone();
        config.board_read(keymap, protocol);
        let changed = *config != before;
        if changed {
            save(&shared, &config);
        }
        changed
    };
    if changed {
        emit_state(app);
    }
}

/// The profile the board should hold: its id and keymap.
pub fn active_keymap(app: &AppHandle) -> (String, Vec<u16>) {
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap();
    let p = config.active_key_profile();
    (p.id.clone(), p.keymap.clone())
}

/// Record which profile is all on the board ("" while it's being written).
pub fn set_on_board(app: &AppHandle, id: &str) {
    let shared = app.state::<Shared>();
    let mut config = shared.core.lock().unwrap();
    if config.on_board != id {
        config.on_board = id.to_string();
        save(&shared, &config);
    }
}

/// The hotkeys of the NK65's profiles, as (shortcut, profile id or None for next, name).
pub fn hotkeys(app: &AppHandle) -> Vec<(String, Option<String>, String)> {
    let Some(shared) = app.try_state::<Shared>() else { return vec![] };
    if *shared.model.lock().unwrap() != Some(Model::Nk65) {
        return vec![];
    }
    let config = shared.core.lock().unwrap();
    let mut v: Vec<_> =
        config.profiles.iter().filter_map(|p| p.hotkey.clone().map(|h| (h, Some(p.id.clone()), format!("{} (keyboard)", p.name)))).collect();
    if let Some(h) = config.next_profile_hotkey.clone() {
        v.push((h, None, "Next keyboard profile".into()));
    }
    v
}

/// Mouse macros were edited or deleted: drop stale bindings, rewrite onboard copies.
pub fn macros_changed(app: &AppHandle) {
    let exists = macro_exists(app);
    {
        let shared = app.state::<Shared>();
        let mut config = shared.core.lock().unwrap();
        let before = config.clone();
        config.drop_missing_macros(|id| exists.iter().any(|m| m == id));
        if *config != before {
            save(&shared, &config);
        }
    }
    sync_hook(app);
    sync_macros(app);
    emit_state(app);
}

fn macro_exists(app: &AppHandle) -> Vec<String> {
    let shared = app.state::<crate::mouse::state::Shared>();
    let core = shared.core.lock().unwrap();
    core.config.macros.iter().map(|m| m.id.clone()).collect()
}

/// Tell the input hook which physical keys kit handles.
fn sync_hook(app: &AppHandle) {
    let keys: Vec<(u16, bool)> = {
        let shared = app.state::<Shared>();
        let config = shared.core.lock().unwrap();
        let model = *shared.model.lock().unwrap();
        match model {
            Some(Model::ProXTkl) => config.bindings.iter().filter_map(|b| via::usage::to_scan(b.usage)).collect(),
            Some(Model::Nk65) => (0..KIT_KEYS)
                .filter(|&i| config.kit_keys[i].is_some() || config.switch_keys[i].is_some())
                .filter_map(|i| via::usage::to_scan(via::usage::F13 + i as u8))
                .collect(),
            None => vec![],
        }
    };
    input::set_key_bindings(&keys);
}

/// What a key the hook caught should do.
pub fn key_action(app: &AppHandle, scan: u16, ext: bool) -> Option<Action> {
    let usage = via::usage::from_scan(scan, ext)?;
    let shared = app.state::<Shared>();
    let config = shared.core.lock().unwrap();
    let model = *shared.model.lock().unwrap();
    match model {
        Some(Model::ProXTkl) => config.bindings.iter().find(|b| b.usage == usage).map(|b| b.action.clone()),
        Some(Model::Nk65) => {
            let i = usage.checked_sub(via::usage::F13)? as usize;
            if let Some(target) = config.switch_keys.get(i)? {
                return Some(if target == NEXT_PROFILE { Action::NextKeyProfile } else { Action::KeyProfile { id: target.clone() } });
            }
            let id = config.kit_keys.get(i)?.clone()?;
            Some(Action::Macro { id })
        }
        None => None,
    }
}

/// Write the macros kit keeps in the NK65's onboard slots.
fn sync_macros(app: &AppHandle) {
    let shared = app.state::<Shared>();
    if *shared.model.lock().unwrap() != Some(Model::Nk65) {
        return;
    }
    let slots = shared.core.lock().unwrap().onboard.clone();
    if slots.is_empty() {
        return;
    }
    let macros: Vec<Macro> = {
        let mouse = app.state::<crate::mouse::state::Shared>();
        let core = mouse.core.lock().unwrap();
        core.config.macros.clone()
    };
    let owned: BTreeMap<u8, Vec<u8>> = slots
        .into_iter()
        .filter_map(|(slot, id)| macros.iter().find(|m| m.id == id).map(|m| (slot, encode_onboard(m))))
        .collect();
    let _ = shared.tx.send(Cmd::Macros(owned));
}

/// A kit macro as an onboard macro: keys and waits only (the board can't
/// click), played `repeat` times per press.
pub fn encode_onboard(m: &Macro) -> Vec<u8> {
    use via::macros::Action as A;
    let once: Vec<A> = m
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::Key { key, down } => {
                let usage = via::usage::from_scan(key.scan, key.ext)? as u16;
                Some(if *down { A::Down(usage) } else { A::Up(usage) })
            }
            Step::Delay { ms } => Some(A::Delay(*ms)),
            Step::Mouse { .. } => None,
        })
        .collect();
    let times = if m.mode == PlayMode::Once { m.repeat.clamp(1, 50) } else { 1 };
    let mut all = Vec::new();
    for _ in 0..times {
        all.extend(once.iter().cloned());
    }
    via::macros::encode(&all)
}

/// Hand the PRO X TKL's lights back before kit exits.
pub fn release(app: &AppHandle) {
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    if app.state::<Shared>().tx.send(Cmd::Release(done_tx)).is_ok() {
        let _ = done_rx.recv_timeout(Duration::from_secs(2));
    }
}

/// What the tray menu shows: the active preset and profile, and every one's name.
fn tray_entries(config: &Config) -> (&str, &str, Vec<(&str, &str)>) {
    let presets = config.presets.iter().map(|p| (p.id.as_str(), p.name.as_str()));
    let profiles = config.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str()));
    (&config.active, &config.active_profile, presets.chain(profiles).collect())
}
