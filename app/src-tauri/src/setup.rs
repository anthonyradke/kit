//! Which devices this PC has and their colors, chosen in the first-run setup
//! and changeable in Settings. Devices that aren't picked stay out of the
//! window and the tray.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::keyboard::config::Model;
use crate::{headset, keyboard, mouse, tray};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub setup_done: bool,
    pub mouse: Option<Product>,
    pub keyboard: Option<KeyboardProduct>,
    pub headset: Option<Product>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Product {
    /// "white" or "black": picks the product photos.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct KeyboardProduct {
    pub model: Model,
    pub color: String,
}

impl Default for Product {
    fn default() -> Self {
        Product { color: "white".into() }
    }
}

impl Default for KeyboardProduct {
    fn default() -> Self {
        KeyboardProduct { model: Model::Nk65, color: "white".into() }
    }
}

pub struct Shared {
    config: Mutex<AppConfig>,
    path: PathBuf,
}

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let dir = app.path().app_config_dir()?;
    let path = dir.join("app.json");
    let config = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default(),
        // Before setup existed kit was the white mouse and headset; offer those.
        Err(_) => AppConfig {
            setup_done: false,
            mouse: dir.join("mouse.json").exists().then(Product::default),
            headset: dir.join("headset.json").exists().then(Product::default),
            keyboard: None,
        },
    };
    app.manage(Shared { config: Mutex::new(normalized(config)), path });
    Ok(())
}

fn normalized(mut c: AppConfig) -> AppConfig {
    let fix = |color: &mut String| {
        if color != "black" {
            *color = "white".into();
        }
    };
    if let Some(p) = c.mouse.as_mut() {
        fix(&mut p.color);
    }
    if let Some(p) = c.headset.as_mut() {
        fix(&mut p.color);
    }
    if let Some(k) = c.keyboard.as_mut() {
        // Each keyboard comes in the color of its photo.
        k.color = match k.model {
            Model::Nk65 => "white".into(),
            Model::ProXTkl => "black".into(),
        };
    }
    c
}

pub fn get(app: &AppHandle) -> AppConfig {
    app.state::<Shared>().config.lock().unwrap().clone()
}

pub fn keyboard_model(app: &AppHandle) -> Option<Model> {
    get(app).keyboard.map(|k| k.model)
}

#[tauri::command]
pub fn app_config(app: AppHandle) -> AppConfig {
    get(&app)
}

#[tauri::command]
pub fn app_save(app: AppHandle, config: AppConfig) -> Result<AppConfig, String> {
    let config = normalized(config);
    let old = {
        let shared = app.state::<Shared>();
        let mut current = shared.config.lock().unwrap();
        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        if let Some(dir) = shared.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&shared.path, text).map_err(|e| e.to_string())?;
        std::mem::replace(&mut *current, config.clone())
    };
    let model = config.keyboard.as_ref().map(|k| k.model);
    if old.keyboard.as_ref().map(|k| k.model) != model {
        keyboard::state::set_model(&app, model);
    }
    tray::refresh(&app);
    Ok(config)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// A Logitech receiver (or cabled mouse) answers.
    pub mouse: bool,
    pub headset: bool,
    pub keyboard: Option<Model>,
}

/// What is plugged in right now, for the setup screen. Probing a Logitech
/// keyboard can take a few seconds, so this runs off the main thread.
#[tauri::command(async)]
pub fn detect_devices(app: AppHandle) -> Detected {
    let m = app.state::<mouse::state::Shared>().status.lock().unwrap().clone();
    let h = app.state::<headset::state::Shared>().headset.lock().unwrap().clone();
    let k = app.state::<keyboard::state::Shared>().status.lock().unwrap().clone();
    let keyboard = if via::find().is_some() {
        Some(Model::Nk65)
    } else if k.present && keyboard::state::model(&app) == Some(Model::ProXTkl) {
        Some(Model::ProXTkl)
    } else {
        hidpp::Device::open(hidpp::Kind::Keyboard).ok().map(|_| Model::ProXTkl)
    };
    Detected { mouse: m.present, headset: h.present, keyboard }
}

/// Vendor apps that fight kit for the devices, if any are running.
#[tauri::command]
pub fn vendor_apps() -> Vec<&'static str> {
    let names = process_names();
    let mut out = vec![];
    if names.iter().any(|n| n.starts_with("lghub")) {
        out.push("Logitech G HUB");
    }
    if names.iter().any(|n| n.starts_with("steelseriesgg") || n.starts_with("steelseriesengine") || n.starts_with("steelseriessonar")) {
        out.push("SteelSeries GG");
    }
    out
}

/// Lowercase executable names of every running process.
fn process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            out.push(String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase());
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}
