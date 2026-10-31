//! System tray icon: switch mouse and keyboard profiles, keyboard lighting and
//! headset presets, toggle noise suppression, without opening the window. Only
//! the devices picked in setup are listed. A device running low on battery
//! marks the icon instead of showing a popup.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::{badge, headset, keyboard, mouse, setup, update};

const TRAY_ID: &str = "main";
/// The same level the window turns a battery red at.
const LOW_BATTERY: u8 = 15;
/// A mark stays until here, so a reading that wobbles around 15% doesn't flicker the icon.
const LOW_BATTERY_CLEARS: u8 = 20;
const PROFILE_PREFIX: &str = "profile:";
const PRESET_PREFIX: &str = "preset:";
const LIGHTING_PREFIX: &str = "light:";
const KEY_PROFILE_PREFIX: &str = "keys:";

/// The icon has the low-battery mark.
static MARKED: AtomicBool = AtomicBool::new(false);

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
                } else if let Some(profile) = id.strip_prefix(KEY_PROFILE_PREFIX) {
                    keyboard::state::activate_profile(app, profile);
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::show_window(tray.app_handle());
            }
        })
        .build(app)?;
    refresh_status(app);
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
    let (lights, active_light, key_profiles, active_keys) = {
        let shared = app.state::<keyboard::state::Shared>();
        let config = shared.core.lock().unwrap();
        let lights: Vec<(String, String)> = config.presets.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        let key_profiles: Vec<(String, String)> = config.profiles.iter().map(|p| (p.id.clone(), p.name.clone())).collect();
        (lights, config.active.clone(), key_profiles, config.active_profile.clone())
    };
    let devices = setup::get(app);
    // Before setup, show everything rather than an empty menu.
    let all = !devices.setup_done;
    let nk65 = setup::keyboard_model(app) == Some(keyboard::config::Model::Nk65);

    if all || devices.mouse.is_some() {
        menu.append(&MenuItem::with_id(app, "h-mouse", "Mouse profile", false, None::<&str>)?)?;
        for (id, name) in profiles {
            let checked = id == active_profile;
            menu.append(&CheckMenuItem::with_id(app, format!("{PROFILE_PREFIX}{id}"), name, true, checked, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    if nk65 {
        menu.append(&MenuItem::with_id(app, "h-keys", "Keyboard profile", false, None::<&str>)?)?;
        for (id, name) in key_profiles {
            let checked = id == active_keys;
            menu.append(&CheckMenuItem::with_id(app, format!("{KEY_PROFILE_PREFIX}{id}"), name, true, checked, None::<&str>)?)?;
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
    refresh_status(app);
}

/// Bring the tooltip and the low-battery mark up to date.
pub fn refresh_status(app: &AppHandle) {
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
    let mouse = m.status.lock().unwrap().clone();
    let kb = k.status.lock().unwrap().clone();
    let headset = h.headset.lock().unwrap().clone();

    // A sleeping mouse or keyboard keeps its last reading; only a headset that's off has none.
    let limit = if MARKED.load(Ordering::Relaxed) { LOW_BATTERY_CLEARS } else { LOW_BATTERY };
    let mut low = vec![];
    let mut check = |picked: bool, name: &str, battery: Option<u8>, charging: bool| {
        if let Some(b) = battery.filter(|&b| picked && b <= limit && !charging) {
            low.push(format!("{name} {b}%"));
        }
    };
    check(all || devices.mouse.is_some(), "mouse", mouse.battery, mouse.charging);
    check(devices.keyboard.is_some(), "keyboard", kb.battery, kb.charging);
    check(all || devices.headset.is_some(), "headset", headset.battery, headset.charging);

    let mut text = if low.is_empty() { String::from("kit") } else { format!("Low battery: {}", low.join(", ")) };

    if all || devices.mouse.is_some() {
        let profile = m.core.lock().unwrap().config.active_profile().name.clone();
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
        let (keys, light) = {
            let config = k.core.lock().unwrap();
            (config.active_key_profile().name.clone(), config.active_preset().name.clone())
        };
        if setup::keyboard_model(app) == Some(keyboard::config::Model::Nk65) {
            text.push_str(&format!("\nKeyboard · {keys} · {light}"));
        } else {
            text.push_str(&format!("\nKeyboard · {light}"));
        }
        match (kb.connected, kb.battery) {
            (true, Some(b)) => text.push_str(&format!(" · {b}%")),
            (true, None) => {}
            (false, _) => text.push_str(" · not connected"),
        }
    }

    if all || devices.headset.is_some() {
        let preset = h.core.lock().unwrap().active_preset().name.clone();
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

    let mark = !low.is_empty();
    if MARKED.swap(mark, Ordering::Relaxed) != mark {
        if let Some(icon) = app.default_window_icon() {
            let (w, h) = (icon.width(), icon.height());
            let image = if mark { Image::new_owned(badge::low_battery(icon.rgba(), w, h), w, h) } else { icon.clone() };
            let _ = tray.set_icon(Some(image));
        }
    }
}
