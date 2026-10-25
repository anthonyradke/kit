#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod headset;
mod keyboard;
mod mouse;
mod setup;
mod tray;
mod update;

use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const HIDDEN_ARG: &str = "--minimized";

/// Show the settings window, creating it if it was closed. Closing destroys
/// the webview so only the small tray process stays resident.
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("kit")
        .inner_size(1240.0, 800.0)
        .min_inner_size(1000.0, 660.0)
        // Frameless: the UI draws its own title bar (drag region + minimize/close).
        .decorations(false)
        .maximizable(false)
        .shadow(true)
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::window::Color(14, 14, 13, 255))
        .build();
}

/// kit replaced two apps, lightswitch (mouse) and headroom (headset). Carry
/// their settings over the first time it runs.
fn migrate_configs(app: &AppHandle) {
    let (Ok(roaming), Ok(dir)) = (app.path().config_dir(), app.path().app_config_dir()) else { return };
    for (old_app, new_file) in [("com.anthonyradke.lightswitch", "mouse.json"), ("com.anthonyradke.headroom", "headset.json")] {
        let old = roaming.join(old_app).join("config.json");
        let new = dir.join(new_file);
        if !new.exists() && old.exists() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::copy(&old, &new);
        }
    }
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(false))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(code) = headset::elevate::helper_main(&args) {
        std::process::exit(code);
    }
    let start_hidden = args.iter().any(|a| a == HIDDEN_ARG);
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![HIDDEN_ARG])))
        .plugin(mouse::hotkeys_plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let handle = app.handle();
            migrate_configs(handle);
            setup::init(handle)?;
            mouse::init(handle)?;
            headset::init(handle)?;
            keyboard::init(handle, setup::keyboard_model(handle))?;
            update::init(handle);
            tray::create(handle)?;
            if !start_hidden {
                show_window(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Destroyed = event {
                mouse::on_window_destroyed(window.app_handle());
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_autostart,
            set_autostart,
            setup::app_config,
            setup::app_save,
            setup::detect_devices,
            setup::vendor_apps,
            mouse::mouse_state,
            mouse::mouse_save,
            mouse::activate_profile,
            mouse::test_macro,
            mouse::start_recording,
            mouse::stop_recording,
            mouse::capture_keys,
            mouse::ui_key,
            mouse::cancel_capture,
            mouse::pause_hotkeys,
            mouse::resume_hotkeys,
            headset::headset_state,
            headset::headset_save,
            headset::activate_preset,
            headset::preview_eq,
            headset::use_headset,
            headset::set_apo_installed,
            headset::apo_log,
            keyboard::keyboard_state,
            keyboard::keyboard_save,
            keyboard::activate_lighting,
            keyboard::preview_lighting,
            keyboard::keyboard_set_key,
            update::update_state,
            update::check_update,
            update::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build kit")
        .run(|app, event| match event {
            // Keep running in the tray when the last window closes; only Quit exits.
            RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
            // Quit or an update restart: give a Logitech keyboard its own lighting back.
            RunEvent::Exit => keyboard::state::release(app),
            _ => {}
        });
}
