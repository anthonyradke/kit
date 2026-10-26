//! The keyboard: an NK65 (VIA firmware: keymap, onboard macros, lighting) or
//! a Logitech PRO X TKL LIGHTSPEED (HID++: lighting, battery; keys handled by
//! kit's input hook).

mod anim;
pub mod config;
mod device;
pub mod state;

use tauri::AppHandle;

use config::{Config, Preset};
use state::UiState;

pub use state::init;

#[tauri::command]
pub fn keyboard_state(app: AppHandle) -> UiState {
    state::ui_state(&app)
}

#[tauri::command]
pub fn keyboard_save(app: AppHandle, config: Config) -> UiState {
    state::update_config(&app, config);
    state::ui_state(&app)
}

#[tauri::command]
pub fn activate_lighting(app: AppHandle, id: String) -> UiState {
    state::activate(&app, &id);
    state::ui_state(&app)
}

#[tauri::command]
pub fn preview_lighting(app: AppHandle, preset: Preset) {
    state::preview(&app, preset);
}

/// Write one keycode to the NK65 (it is stored on the board right away).
#[tauri::command]
pub fn keyboard_set_key(app: AppHandle, layer: u8, row: u8, col: u8, code: u16) {
    state::set_key(&app, layer, row, col, code);
}

/// NK65: put the keymap and macros from before a firmware change back, or forget them.
#[tauri::command]
pub fn keyboard_restore(app: AppHandle, apply: bool) {
    state::send(&app, device::Cmd::Restore(apply));
}

/// NK65: restart into the bootloader so new firmware can be flashed.
#[tauri::command]
pub fn keyboard_bootloader(app: AppHandle) {
    state::send(&app, device::Cmd::Bootloader);
}

/// Open kit's latest GitHub release (where the NK65 firmware files are) in the browser.
#[tauri::command]
pub fn keyboard_firmware_page() {
    use windows::core::w;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        ShellExecuteW(None, w!("open"), w!("https://github.com/anthonyradke/kit/releases/latest"), None, None, SW_SHOWNORMAL);
    }
}
