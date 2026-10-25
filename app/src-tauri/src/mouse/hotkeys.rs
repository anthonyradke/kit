//! Global keyboard shortcuts for switching profiles.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::state::{self, Shared};

#[derive(Clone, PartialEq)]
enum Target {
    Profile(String),
    Next,
}

static BINDINGS: Mutex<Option<HashMap<u32, Target>>> = Mutex::new(None);
/// What was last registered, so saves that don't touch hotkeys skip the work.
static REGISTERED: Mutex<Option<Vec<(String, Target, String)>>> = Mutex::new(None);
/// While the UI captures a new shortcut, every hotkey is off so pressing one
/// that is already in use is captured instead of switching profiles.
static PAUSED: AtomicBool = AtomicBool::new(false);

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let target = {
                let bindings = BINDINGS.lock().unwrap();
                match bindings.as_ref().and_then(|b| b.get(&shortcut.id())) {
                    Some(Target::Profile(id)) => Some(Some(id.clone())),
                    Some(Target::Next) => Some(None),
                    None => None,
                }
            };
            match target {
                Some(Some(id)) => state::activate(app, &id),
                Some(None) => state::next_profile(app),
                None => {}
            }
        })
        .build()
}

pub fn pause(app: &AppHandle) {
    PAUSED.store(true, Ordering::SeqCst);
    let _ = app.global_shortcut().unregister_all();
    *REGISTERED.lock().unwrap() = None;
}

pub fn resume(app: &AppHandle) {
    PAUSED.store(false, Ordering::SeqCst);
    register(app);
}

/// Re-register every hotkey from the current config, recording failures
/// (bad syntax, already taken by another app) for the UI.
pub fn register(app: &AppHandle) {
    if PAUSED.load(Ordering::SeqCst) {
        return;
    }
    let shared = app.state::<Shared>();
    let wanted: Vec<(String, Target, String)> = {
        let core = shared.core.lock().unwrap();
        let mut v: Vec<_> = core
            .config
            .profiles
            .iter()
            .filter_map(|p| p.hotkey.clone().map(|h| (h, Target::Profile(p.id.clone()), p.name.clone())))
            .collect();
        if let Some(h) = core.config.settings.next_profile_hotkey.clone() {
            v.push((h, Target::Next, "Next profile".into()));
        }
        v
    };
    let mut registered = REGISTERED.lock().unwrap();
    if registered.as_ref() == Some(&wanted) {
        return;
    }
    *registered = Some(wanted.clone());

    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut bindings = HashMap::new();
    let mut errors = Vec::new();
    for (text, target, owner) in wanted {
        let shortcut = match text.parse::<Shortcut>() {
            Ok(s) => s,
            Err(_) => {
                errors.push(format!("{owner}: \"{text}\" is not a valid shortcut"));
                continue;
            }
        };
        if bindings.contains_key(&shortcut.id()) {
            errors.push(format!("{owner}: {text} is already used by another profile"));
            continue;
        }
        match gs.register(shortcut) {
            Ok(()) => {
                bindings.insert(shortcut.id(), target);
            }
            Err(_) => errors.push(format!("{owner}: {text} is taken by another app")),
        }
    }
    *BINDINGS.lock().unwrap() = Some(bindings);
    *shared.hotkey_errors.lock().unwrap() = errors;
}
