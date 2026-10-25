//! System tray icon: switch mouse profiles, keyboard lighting and headset
//! presets, toggle noise suppression, without opening the window. Only the
//! devices picked in setup are listed.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::{headset, keyboard, mouse, setup, update};

const TRAY_ID: &str = "main";
const PROFILE_PREFIX: &str = "profile:";
const PRESET_PREFIX: &str = "preset:";
const LIGHTING_PREFIX: &str = "light:";

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("kit")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::show_window(app),
            "quit" => app.exit(0),
            "update" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = update::install(&app).await;
                });
            }
            "denoise" => {
                let on = app.state::<headset::state::Shared>().core.lock().unwrap().mic.denoise;
                headset::state::set_denoise(app, !on);
            }
            "default" => {
                let _ = app.state::<headset::state::Shared>().audio_tx.send(headset::audio::Cmd::UseHeadset);
            }
            id => {
                if let Some(profile) = id.strip_prefix(PROFILE_PREFIX) {
                    mouse::state::activate(app, profile);
                } else if let Some(preset) = id.strip_prefix(PRESET_PREFIX) {
                    headset::state::activate(app, preset);
                } else if let Some(preset) = id.strip_prefix(LIGHTING_PREFIX) {
                    keyboard::state::activate(app, preset);
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
    let (profiles, active_profile) = {
        let shared = app.state::<mouse::state::Shared>();
        let core = shared.core.lock().unwrap();
        let profiles: Vec<(String, String)> =
            core.config.profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        (profiles, core.config.active.clone())
    };
    let (presets, active_preset, denoise) = {
        let shared = app.state::<headset::state::Shared>();
        let config = shared.core.lock().unwrap();
        let presets: Vec<(String, String)> = config.presets.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        (presets, config.active.clone(), config.mic.denoise)
    };
    let (lights, active_light) = {
        let shared = app.state::<keyboard::state::Shared>();
        let config = shared.core.lock().unwrap();
        let lights: Vec<(String, String)> = config.presets.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        (lights, config.active.clone())
    };
    let devices = setup::get(app);
    // Before setup, show everything rather than an empty menu.
    let all = !devices.setup_done;

    if all || devices.mouse.is_some() {
        menu.append(&MenuItem::with_id(app, "h-mouse", "Mouse profile", false, None::<&str>)?)?;
        for (id, name) in profiles {
            let checked = id == active_profile;
            menu.append(&CheckMenuItem::with_id(app, format!("{PROFILE_PREFIX}{id}"), name, true, checked, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if devices.keyboard.is_some() {
        menu.append(&MenuItem::with_id(app, "h-keyboard", "Keyboard lighting", false, None::<&str>)?)?;
        for (id, name) in lights {
            let checked = id == active_light;
            menu.append(&CheckMenuItem::with_id(app, format!("{LIGHTING_PREFIX}{id}"), name, true, checked, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if all || devices.headset.is_some() {
        menu.append(&MenuItem::with_id(app, "h-headset", "Headset EQ", false, None::<&str>)?)?;
        for (id, name) in presets {
            let checked = id == active_preset;
            menu.append(&CheckMenuItem::with_id(app, format!("{PRESET_PREFIX}{id}"), name, true, checked, None::<&str>)?)?;
        }
        menu.append(&CheckMenuItem::with_id(app, "denoise", "Mic noise suppression", true, denoise, None::<&str>)?)?;
        menu.append(&MenuItem::with_id(app, "default", "Use headset as default device", true, None::<&str>)?)?;
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if let Some(version) = app.try_state::<update::Shared>().and_then(|_| update::status(app).available) {
        menu.append(&MenuItem::with_id(app, "update", format!("Update to kit {version}"), true, None::<&str>)?)?;
    }
    menu.append(&MenuItem::with_id(app, "open", "Open kit", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

/// Rebuild the menu after profiles, presets or noise suppression change.
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
    // Every device's worker calls this; any may run before the others' state exists.
    let (Some(m), Some(h), Some(k), Some(_)) = (
        app.try_state::<mouse::state::Shared>(),
        app.try_state::<headset::state::Shared>(),
        app.try_state::<keyboard::state::Shared>(),
        app.try_state::<setup::Shared>(),
    ) else {
        return;
    };
    let devices = setup::get(app);
    let all = !devices.setup_done;
    let mut text = String::from("kit");

    if all || devices.mouse.is_some() {
        let profile = m.core.lock().unwrap().config.active_profile().name.clone();
        let mouse = m.status.lock().unwrap().clone();
        text.push_str(&format!("\nMouse · {profile}"));
        if mouse.connected {
            if let Some(dpi) = mouse.dpi {
                text.push_str(&format!(" · {dpi} DPI"));
            }
            if let Some(b) = mouse.battery {
                text.push_str(&format!(" · {b}%"));
            }
        } else {
            text.push_str(" · not connected");
        }
    }

    if devices.keyboard.is_some() {
        let light = k.core.lock().unwrap().active_preset().name.clone();
        let kb = k.status.lock().unwrap().clone();
        text.push_str(&format!("\nKeyboard · {light}"));
        match (kb.connected, kb.battery) {
            (true, Some(b)) => text.push_str(&format!(" · {b}%")),
            (true, None) => {}
            (false, _) => text.push_str(" · not connected"),
        }
    }

    if all || devices.headset.is_some() {
        let preset = h.core.lock().unwrap().active_preset().name.clone();
        let headset = h.headset.lock().unwrap().clone();
        text.push_str(&format!("\nHeadset · {preset}"));
        if headset.connected {
            if let Some(b) = headset.battery {
                text.push_str(&format!(" · {b}%"));
            }
        } else if headset.present {
            text.push_str(" · off");
        } else {
            text.push_str(" · not found");
        }
    }
    let _ = tray.set_tooltip(Some(text));
}
