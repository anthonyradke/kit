//! Export and import: every device's settings in one JSON file, to carry them
//! to a new Windows install or share them. Importing either replaces a device's
//! settings or adds what the file has that this PC doesn't. `app.json` stays
//! out of it: which devices this PC has isn't something to copy.

use std::path::{Path, PathBuf};

use rfd::FileDialog;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::{headset, keyboard, mouse, setup};

/// Bump when an older kit could no longer read the file.
const FORMAT: u64 = 1;
const NOT_A_BACKUP: &str = "That isn't a kit settings file.";
/// What's here is kept as before-import-<time>.json, in kit's config folder,
/// before an import. The newest few are kept.
const BEFORE_IMPORT: &str = "before-import";
const KEEP_BEFORE_IMPORT: usize = 5;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bundle {
    kit_settings: u64,
    version: String,
    mouse: Option<mouse::config::Config>,
    keyboard: Option<keyboard::config::Config>,
    headset: Option<headset::config::Config>,
}

impl Bundle {
    fn parse(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| NOT_A_BACKUP)?;
        let format = value.get("kitSettings").and_then(Value::as_u64).ok_or(NOT_A_BACKUP)?;
        if format > FORMAT {
            return Err("This file is from a newer kit. Update kit, then import it again.".into());
        }
        let mut bundle: Bundle = serde_json::from_value(value).map_err(|_| NOT_A_BACKUP)?;
        if let Some(c) = &mut bundle.mouse {
            c.normalize();
        }
        if let Some(c) = &mut bundle.keyboard {
            c.normalize();
        }
        if let Some(c) = &mut bundle.headset {
            c.normalize();
            c.upgrade_presets();
        }
        Ok(bundle)
    }

    fn read(path: &Path) -> Result<Self, String> {
        Self::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }

    fn write(&self, path: &Path) -> Result<(), String> {
        crate::store::save(path, self).map_err(|e| e.to_string())
    }

    /// The settings kit is running with now.
    fn current(app: &AppHandle) -> Self {
        let mouse = app.state::<mouse::state::Shared>().core.lock().unwrap().config.clone();
        let keyboard = app.state::<keyboard::state::Shared>().core.lock().unwrap().clone();
        let headset = app.state::<headset::state::Shared>().core.lock().unwrap().clone();
        Bundle {
            kit_settings: FORMAT,
            version: app.package_info().version.to_string(),
            mouse: Some(mouse),
            keyboard: Some(keyboard),
            headset: Some(headset),
        }
    }
}

/// Show a file dialog for kit settings files over kit's window.
fn dialog(app: &AppHandle, show: impl FnOnce(FileDialog) -> Option<PathBuf> + Send + 'static) -> Option<PathBuf> {
    file_dialog(app, ("kit settings", "json"), show)
}

/// Show a file dialog over kit's window, for files of one kind (a name and
/// an extension). It gets its own thread because it blocks until closed and
/// sets up COM its own way.
pub fn file_dialog(app: &AppHandle, kind: (&'static str, &'static str), show: impl FnOnce(FileDialog) -> Option<PathBuf> + Send + 'static) -> Option<PathBuf> {
    let window = app.get_webview_window("main");
    let thread = std::thread::spawn(move || {
        let mut dialog = FileDialog::new().add_filter(kind.0, &[kind.1]);
        if let Some(window) = &window {
            dialog = dialog.set_parent(window);
        }
        show(dialog)
    });
    thread.join().ok().flatten()
}

/// Write every setting to a file the user picks. Its path, or nothing if they cancelled.
#[tauri::command(async)]
pub fn settings_export(app: AppHandle, file_name: String) -> Result<Option<String>, String> {
    let Some(path) = dialog(&app, move |d| d.set_file_name(file_name).save_file()) else { return Ok(None) };
    Bundle::current(&app).write(&path)?;
    Ok(Some(path.display().to_string()))
}

/// Let the user pick a file and say what's in it, or nothing if they cancelled.
#[tauri::command(async)]
pub fn settings_open(app: AppHandle) -> Result<Option<Value>, String> {
    let Some(path) = dialog(&app, |d| d.pick_file()) else { return Ok(None) };
    let bundle = Bundle::read(&path)?;
    let model = setup::keyboard_model(&app);
    Ok(Some(json!({
        "path": path.display().to_string(),
        "version": bundle.version,
        "mouse": bundle.mouse.map(|c| json!({ "profiles": c.profiles.len(), "macros": c.macros.len() })),
        "keyboard": bundle.keyboard.map(|c| json!({
            "model": c.model,
            "fits": Some(c.model) == model,
            "profiles": c.profiles.len(),
            "presets": c.presets.len(),
        })),
        "headset": bundle.headset.map(|c| json!({ "presets": c.presets.len() })),
    })))
}

/// Bring in the picked parts of the file at `path`. The mouse goes first:
/// keyboards bind its macros.
#[tauri::command(async)]
pub fn settings_import(app: AppHandle, path: String, mouse: bool, keyboard: bool, headset: bool, replace: bool) -> Result<(), String> {
    let bundle = Bundle::read(Path::new(&path))?;
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    keep_current(&app, &dir)?;
    if let Some(config) = bundle.mouse.filter(|_| mouse) {
        mouse::state::import(&app, config, replace);
    }
    // Lighting effects and keymaps only mean something on the keyboard they were made for.
    if let Some(config) = bundle.keyboard.filter(|c| keyboard && Some(c.model) == setup::keyboard_model(&app)) {
        keyboard::state::import(&app, config, replace);
    }
    if let Some(config) = bundle.headset.filter(|_| headset) {
        headset::state::import(&app, config, replace);
    }
    Ok(())
}

/// Save the settings kit runs with now as a new before-import file, and drop
/// the oldest ones past the few kept.
fn keep_current(app: &AppHandle, dir: &Path) -> Result<(), String> {
    let name = format!("{BEFORE_IMPORT}-{}.json", crate::logging::file_stamp());
    Bundle::current(app).write(&dir.join(name))?;
    let mut old: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(BEFORE_IMPORT) && n.ends_with(".json")))
        .collect();
    // The stamp sorts by time; the plain before-import.json of older kits sorts first.
    old.sort();
    let extra = old.len().saturating_sub(KEEP_BEFORE_IMPORT);
    for path in &old[..extra] {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

/// Settings files kit couldn't use when it started, for the window to mention.
#[tauri::command]
pub fn settings_problems() -> Vec<String> {
    crate::store::problems()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_file_reads_back_the_same() {
        let mut mouse = mouse::config::Config::default();
        let mut keyboard = keyboard::config::Config::default();
        let mut headset = headset::config::Config::default();
        mouse.normalize();
        keyboard.normalize();
        headset.normalize();
        headset.upgrade_presets();
        let bundle = Bundle { kit_settings: FORMAT, version: "1.2.3".into(), mouse: Some(mouse), keyboard: Some(keyboard), headset: Some(headset) };
        assert_eq!(Bundle::parse(&serde_json::to_string(&bundle).unwrap()).unwrap(), bundle);
    }

    #[test]
    fn a_file_can_hold_only_some_devices() {
        let bundle = Bundle::parse(r#"{"kitSettings": 1, "version": "0.7.0", "headset": {}}"#).unwrap();
        assert!(bundle.mouse.is_none() && bundle.keyboard.is_none());
        assert!(!bundle.headset.unwrap().presets.is_empty());
    }

    #[test]
    fn other_files_are_refused() {
        for text in ["", "not json", "[]", r#"{"profiles": []}"#, r#"{"kitSettings": "1", "version": "0.7.0"}"#, r#"{"kitSettings": 1}"#] {
            assert_eq!(Bundle::parse(text).unwrap_err(), NOT_A_BACKUP, "{text}");
        }
    }

    #[test]
    fn newer_files_ask_for_an_update() {
        let text = format!(r#"{{"kitSettings": {}, "version": "9.0.0"}}"#, FORMAT + 1);
        assert!(Bundle::parse(&text).unwrap_err().contains("newer kit"));
    }

    #[test]
    fn a_byte_order_mark_is_fine() {
        assert!(Bundle::parse("\u{feff}{\"kitSettings\": 1, \"version\": \"0.7.0\"}").is_ok());
    }
}
