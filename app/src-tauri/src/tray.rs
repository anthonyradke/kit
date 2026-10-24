//! System tray icon: switch presets and toggle noise suppression without
//! opening the window.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::audio;
use crate::state::{self, Shared};

const TRAY_ID: &str = "main";
const PRESET_PREFIX: &str = "preset:";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("headroom")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_window(app),
            "quit" => app.exit(0),
            "denoise" => {
                let on = app.state::<Shared>().core.lock().unwrap().mic.denoise;
                state::set_denoise(app, !on);
            }
            "default" => {
                let _ = app.state::<Shared>().audio_tx.send(audio::Cmd::UseHeadset);
            }
            id => {
                if let Some(preset) = id.strip_prefix(PRESET_PREFIX) {
                    state::activate(app, preset);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_window(tray.app_handle());
            }
        })
        .build(app)?;
    refresh_tooltip(app);
    Ok(())
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    let (presets, active, denoise) = {
        let shared = app.state::<Shared>();
        let config = shared.core.lock().unwrap();
        let presets: Vec<(String, String)> = config.presets.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        (presets, config.active.clone(), config.mic.denoise)
    };
    for (id, name) in presets {
        let item = CheckMenuItem::with_id(app, format!("{PRESET_PREFIX}{id}"), name, true, id == active, None::<&str>)?;
        menu.append(&item)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&CheckMenuItem::with_id(app, "denoise", "Mic noise suppression", true, denoise, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "default", "Use headset as default device", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "open", "Open headroom", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

/// Rebuild the menu after presets, the active preset or noise suppression change.
pub fn refresh(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    refresh_tooltip(app);
}

pub fn refresh_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let shared = app.state::<Shared>();
    let preset = shared.core.lock().unwrap().active_preset().name.clone();
    let status = shared.headset.lock().unwrap().clone();
    let mut text = format!("headroom · {preset}");
    if status.connected {
        if let Some(b) = status.battery {
            text.push_str(&format!("\nHeadset {b}%{}", if status.charging { " (charging)" } else { "" }));
        }
    } else if status.present {
        text.push_str("\nHeadset off");
    } else {
        text.push_str("\nBase station not found");
    }
    let _ = tray.set_tooltip(Some(text));
}
