//! Notices when nobody is at the PC: the session locked, every display off, or
//! Windows going to sleep. A hidden window on its own thread takes the messages
//! because telling the keyboard waits for it, which the input hooks can't afford.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::AppHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Power::{RegisterPowerSettingNotification, POWERBROADCAST_SETTING};
use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows_sys::Win32::System::SystemServices::GUID_CONSOLE_DISPLAY_STATE;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::keyboard;

/// `GUID_CONSOLE_DISPLAY_STATE`'s value for "off" (1 is on, 2 is dimmed).
const DISPLAY_OFF_VALUE: u8 = 0;

static APP: OnceLock<AppHandle> = OnceLock::new();
static LOCKED: AtomicBool = AtomicBool::new(false);
static DISPLAY_OFF: AtomicBool = AtomicBool::new(false);
static SUSPENDED: AtomicBool = AtomicBool::new(false);
static AWAY: AtomicBool = AtomicBool::new(false);

pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    std::thread::Builder::new()
        .name("session".into())
        .spawn(|| unsafe { session_thread() })
        .expect("spawn session thread");
}

unsafe fn session_thread() {
    let module = GetModuleHandleW(std::ptr::null());
    let class: Vec<u16> = "kit-session\0".encode_utf16().collect();
    let mut wc: WNDCLASSW = std::mem::zeroed();
    wc.lpfnWndProc = Some(window_proc);
    wc.hInstance = module;
    wc.lpszClassName = class.as_ptr();
    RegisterClassW(&wc);
    // Top-level: message-only windows don't get these broadcasts.
    let hwnd = CreateWindowExW(
        WS_EX_TOOLWINDOW,
        class.as_ptr(),
        std::ptr::null(),
        WS_POPUP,
        0,
        0,
        0,
        0,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        module,
        std::ptr::null(),
    );
    if hwnd.is_null() {
        log::warn!("creating the window failed; the keyboard won't notice the PC locking");
        return;
    }
    if WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) == 0 {
        log::warn!("lock notifications unavailable");
    }
    if RegisterPowerSettingNotification(hwnd, &GUID_CONSOLE_DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE) == 0 {
        log::warn!("display notifications unavailable");
    }
    let mut msg: MSG = std::mem::zeroed();
    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        DispatchMessageW(&msg);
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match (msg, wparam as u32) {
        (WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK) => set(&LOCKED, true),
        (WM_WTSSESSION_CHANGE, WTS_SESSION_UNLOCK) => set(&LOCKED, false),
        // Windows waits for this one before sleeping, so the lights get to go out first.
        (WM_POWERBROADCAST, PBT_APMSUSPEND) => set(&SUSPENDED, true),
        (WM_POWERBROADCAST, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => set(&SUSPENDED, false),
        (WM_POWERBROADCAST, PBT_POWERSETTINGCHANGE) => {
            // The display state is the only setting this window signed up for.
            let setting = &*(lparam as *const POWERBROADCAST_SETTING);
            if setting.DataLength > 0 {
                set(&DISPLAY_OFF, setting.Data[0] == DISPLAY_OFF_VALUE);
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn set(flag: &AtomicBool, on: bool) {
    flag.store(on, Ordering::Relaxed);
    let away = LOCKED.load(Ordering::Relaxed) || DISPLAY_OFF.load(Ordering::Relaxed) || SUSPENDED.load(Ordering::Relaxed);
    if AWAY.swap(away, Ordering::Relaxed) != away {
        if let Some(app) = APP.get() {
            keyboard::state::pc_away(app, away);
        }
    }
}
