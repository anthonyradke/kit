//! Low-level Windows input: side-button interception, macro recording and
//! synthetic input playback.
//!
//! Hook callbacks must return quickly (Windows silently removes slow hooks), so
//! they only touch atomics and a short-lived mutex and hand work to other
//! threads over channels.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::config::{KeyRef, MouseButton, Step};
use crate::exec;

const MODE_OFF: u8 = 0;
const MODE_MACRO: u8 = 1;
const MODE_COMBO: u8 = 2;

const MSG_KEYBOARD_ON: u32 = WM_APP + 1;
const MSG_KEYBOARD_OFF: u32 = WM_APP + 2;

/// Whether Back (0) / Forward (1) presses are swallowed and handled by us.
static INTERCEPT: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];
static MODE: AtomicU8 = AtomicU8::new(MODE_OFF);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
static EXEC: OnceLock<Sender<exec::Msg>> = OnceLock::new();
static RECORD: Mutex<Recording> = Mutex::new(Recording::new());
static COMBO_DONE: Mutex<Option<Sender<Vec<KeyRef>>>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum Raw {
    Key { vk: u16, scan: u16, ext: bool, down: bool },
    Mouse { button: MouseButton, down: bool },
}

struct Recording {
    events: Vec<(u32, Raw)>,
    /// Physical keys (scan code, extended) currently held, used to drop
    /// auto-repeat, de-duplicate hook/UI events and detect combo release.
    held: Vec<(u16, bool)>,
}

impl Recording {
    const fn new() -> Self {
        Self { events: Vec::new(), held: Vec::new() }
    }
}

/// Install the mouse hook on a dedicated message-loop thread.
pub fn start(exec_tx: Sender<exec::Msg>) {
    let _ = EXEC.set(exec_tx);
    std::thread::Builder::new()
        .name("input-hook".into())
        .spawn(|| unsafe { hook_thread() })
        .expect("spawn hook thread");
}

pub fn set_intercept(back: bool, forward: bool) {
    INTERCEPT[0].store(back, Ordering::Relaxed);
    INTERCEPT[1].store(forward, Ordering::Relaxed);
}

pub fn start_recording() {
    {
        let mut rec = RECORD.lock().unwrap();
        rec.events.clear();
        rec.held.clear();
    }
    MODE.store(MODE_MACRO, Ordering::SeqCst);
    post(MSG_KEYBOARD_ON);
}

pub fn stop_recording() -> Vec<Step> {
    MODE.store(MODE_OFF, Ordering::SeqCst);
    post(MSG_KEYBOARD_OFF);
    let events = std::mem::take(&mut RECORD.lock().unwrap().events);
    to_steps(&events)
}

/// Capture one key combination; the result arrives on the returned channel
/// once all keys are released. Keys are swallowed while capturing.
pub fn capture_combo() -> std::sync::mpsc::Receiver<Vec<KeyRef>> {
    let (tx, rx) = std::sync::mpsc::channel();
    *COMBO_DONE.lock().unwrap() = Some(tx);
    {
        let mut rec = RECORD.lock().unwrap();
        rec.events.clear();
        rec.held.clear();
    }
    MODE.store(MODE_COMBO, Ordering::SeqCst);
    post(MSG_KEYBOARD_ON);
    rx
}

pub fn cancel_capture() {
    if MODE.compare_exchange(MODE_COMBO, MODE_OFF, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        post(MSG_KEYBOARD_OFF);
    }
    COMBO_DONE.lock().unwrap().take();
}

fn post(msg: u32) {
    let thread = HOOK_THREAD.load(Ordering::SeqCst);
    if thread != 0 {
        unsafe { PostThreadMessageW(thread, msg, 0, 0) };
    }
}

unsafe fn hook_thread() {
    let module = GetModuleHandleW(std::ptr::null());
    let mut msg: MSG = std::mem::zeroed();
    // Make sure this thread has a message queue before anyone posts to it.
    PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
    HOOK_THREAD.store(GetCurrentThreadId(), Ordering::SeqCst);

    let mut mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
    let mut keyboard: HHOOK = std::ptr::null_mut();
    // Windows silently drops a low-level hook that once answered too slowly
    // (common around sleep/wake), with no notification. Re-install it
    // periodically; the new hook goes in before the old one comes out.
    SetTimer(std::ptr::null_mut(), 0, 30_000, None);

    while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
        match msg.message {
            WM_TIMER => {
                let fresh = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
                if !fresh.is_null() {
                    UnhookWindowsHookEx(mouse);
                    mouse = fresh;
                }
            }
            MSG_KEYBOARD_ON if keyboard.is_null() => {
                keyboard = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module, 0);
            }
            MSG_KEYBOARD_OFF if !keyboard.is_null() => {
                UnhookWindowsHookEx(keyboard);
                keyboard = std::ptr::null_mut();
            }
            _ => {}
        }
    }
}

fn hiword(v: u32) -> u16 {
    (v >> 16) as u16
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const MSLLHOOKSTRUCT);
        if info.flags & LLMHF_INJECTED == 0 {
            let msg = wparam as u32;
            let button = match msg {
                WM_LBUTTONDOWN | WM_LBUTTONUP => Some(MouseButton::Left),
                WM_RBUTTONDOWN | WM_RBUTTONUP => Some(MouseButton::Right),
                WM_MBUTTONDOWN | WM_MBUTTONUP => Some(MouseButton::Middle),
                WM_XBUTTONDOWN | WM_XBUTTONUP if hiword(info.mouseData) == XBUTTON1 => {
                    Some(MouseButton::Back)
                }
                WM_XBUTTONDOWN | WM_XBUTTONUP => Some(MouseButton::Forward),
                _ => None,
            };
            if let Some(button) = button {
                let down = matches!(msg, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN);
                let mode = MODE.load(Ordering::Relaxed);
                if mode == MODE_MACRO {
                    // Clicks on our own window (Record/Stop buttons) are not part of the macro.
                    if !is_own_window(info.pt) {
                        RECORD.lock().unwrap().events.push((info.time, Raw::Mouse { button, down }));
                    }
                } else if mode == MODE_OFF {
                    let side = match button {
                        MouseButton::Back => Some(0),
                        MouseButton::Forward => Some(1),
                        _ => None,
                    };
                    if let Some(side) = side {
                        if INTERCEPT[side].load(Ordering::Relaxed) {
                            if let Some(tx) = EXEC.get() {
                                let _ = tx.send(exec::Msg::Button { forward: side == 1, down });
                            }
                            return 1;
                        }
                    }
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = &*(lparam as *const KBDLLHOOKSTRUCT);
        if info.flags & LLKHF_INJECTED == 0 {
            let down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let ext = info.flags & LLKHF_EXTENDED != 0;
            if handle_key(info.vkCode as u16, info.scanCode as u16, ext, down, info.time) {
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// A key event from our own window. Windows does not run low-level keyboard
/// hooks for input going to our own foreground window, so the UI forwards its
/// key events here while recording or capturing.
pub fn ui_key(code: &str, down: bool) {
    let Some((scan, ext)) = scan_code(code) else { return };
    let vk = unsafe { MapVirtualKeyW(scan as u32 | if ext { 0xE000 } else { 0 }, MAPVK_VSC_TO_VK_EX) } as u16;
    let vk = match (code, vk) {
        ("NumLock", _) => VK_NUMLOCK,
        ("Pause", _) => VK_PAUSE,
        (_, 0) => return,
        (_, vk) => vk,
    };
    handle_key(vk, scan, ext, down, unsafe { GetTickCount() });
}

/// Shared by the hook and the UI. Keys are identified by physical key
/// (scan code), so an event seen by both sources is only recorded once.
/// Returns true if the key should be swallowed.
fn handle_key(vk: u16, scan: u16, ext: bool, down: bool, time: u32) -> bool {
    let mode = MODE.load(Ordering::Relaxed);
    if mode == MODE_OFF {
        return false;
    }
    let id = (scan, ext);
    let raw = Raw::Key { vk, scan, ext, down };
    let mut rec = RECORD.lock().unwrap();
    let held = rec.held.contains(&id);
    if down && !held {
        rec.held.push(id);
        rec.events.push((time, raw));
    } else if !down && held {
        rec.held.retain(|&k| k != id);
        if mode == MODE_MACRO {
            rec.events.push((time, raw));
        }
    }
    if mode == MODE_COMBO {
        if !down && rec.held.is_empty() && !rec.events.is_empty() {
            let keys = rec
                .events
                .drain(..)
                .filter_map(|(_, r)| match r {
                    Raw::Key { vk, scan, ext, .. } => Some(key_ref(vk, scan, ext)),
                    Raw::Mouse { .. } => None,
                })
                .collect();
            MODE.store(MODE_OFF, Ordering::SeqCst);
            post(MSG_KEYBOARD_OFF);
            if let Some(tx) = COMBO_DONE.lock().unwrap().take() {
                let _ = tx.send(keys);
            }
        }
        return true;
    }
    false
}

/// Set-1 scan code (and extended flag) for a browser `KeyboardEvent.code`.
fn scan_code(code: &str) -> Option<(u16, bool)> {
    const LETTERS: [(u8, u16); 26] = [
        (b'A', 0x1E), (b'B', 0x30), (b'C', 0x2E), (b'D', 0x20), (b'E', 0x12), (b'F', 0x21), (b'G', 0x22),
        (b'H', 0x23), (b'I', 0x17), (b'J', 0x24), (b'K', 0x25), (b'L', 0x26), (b'M', 0x32), (b'N', 0x31),
        (b'O', 0x18), (b'P', 0x19), (b'Q', 0x10), (b'R', 0x13), (b'S', 0x1F), (b'T', 0x14), (b'U', 0x16),
        (b'V', 0x2F), (b'W', 0x11), (b'X', 0x2D), (b'Y', 0x15), (b'Z', 0x2C),
    ];
    if let Some(l) = code.strip_prefix("Key") {
        let b = *l.as_bytes().first()?;
        return LETTERS.iter().find(|(c, _)| *c == b).map(|&(_, s)| (s, false));
    }
    if let Some(d) = code.strip_prefix("Digit") {
        let n: u16 = d.parse().ok()?;
        return Some((if n == 0 { 0x0B } else { 0x01 + n }, false));
    }
    if let Some(f) = code.strip_prefix('F').and_then(|f| f.parse::<u16>().ok()) {
        return match f {
            1..=10 => Some((0x3A + f, false)),
            11 => Some((0x57, false)),
            12 => Some((0x58, false)),
            13..=23 => Some((0x64 + f - 13, false)),
            24 => Some((0x76, false)),
            _ => None,
        };
    }
    let (scan, ext) = match code {
        "Escape" => (0x01, false),
        "Minus" => (0x0C, false),
        "Equal" => (0x0D, false),
        "Backspace" => (0x0E, false),
        "Tab" => (0x0F, false),
        "BracketLeft" => (0x1A, false),
        "BracketRight" => (0x1B, false),
        "Enter" => (0x1C, false),
        "ControlLeft" => (0x1D, false),
        "Semicolon" => (0x27, false),
        "Quote" => (0x28, false),
        "Backquote" => (0x29, false),
        "ShiftLeft" => (0x2A, false),
        "Backslash" => (0x2B, false),
        "Comma" => (0x33, false),
        "Period" => (0x34, false),
        "Slash" => (0x35, false),
        "ShiftRight" => (0x36, false),
        "NumpadMultiply" => (0x37, false),
        "AltLeft" => (0x38, false),
        "Space" => (0x39, false),
        "CapsLock" => (0x3A, false),
        "Pause" => (0x45, false),
        "ScrollLock" => (0x46, false),
        "Numpad7" => (0x47, false),
        "Numpad8" => (0x48, false),
        "Numpad9" => (0x49, false),
        "NumpadSubtract" => (0x4A, false),
        "Numpad4" => (0x4B, false),
        "Numpad5" => (0x4C, false),
        "Numpad6" => (0x4D, false),
        "NumpadAdd" => (0x4E, false),
        "Numpad1" => (0x4F, false),
        "Numpad2" => (0x50, false),
        "Numpad3" => (0x51, false),
        "Numpad0" => (0x52, false),
        "NumpadDecimal" => (0x53, false),
        "IntlBackslash" => (0x56, false),
        "NumpadEnter" => (0x1C, true),
        "ControlRight" => (0x1D, true),
        "NumpadDivide" => (0x35, true),
        "PrintScreen" => (0x37, true),
        "AltRight" => (0x38, true),
        "NumLock" => (0x45, true),
        "Home" => (0x47, true),
        "ArrowUp" => (0x48, true),
        "PageUp" => (0x49, true),
        "ArrowLeft" => (0x4B, true),
        "ArrowRight" => (0x4D, true),
        "End" => (0x4F, true),
        "ArrowDown" => (0x50, true),
        "PageDown" => (0x51, true),
        "Insert" => (0x52, true),
        "Delete" => (0x53, true),
        "MetaLeft" => (0x5B, true),
        "MetaRight" => (0x5C, true),
        "ContextMenu" => (0x5D, true),
        _ => return None,
    };
    Some((scan, ext))
}

unsafe fn is_own_window(pt: POINT) -> bool {
    let hwnd = WindowFromPoint(pt);
    if hwnd.is_null() {
        return false;
    }
    let root = GetAncestor(hwnd, GA_ROOT);
    let mut pid = 0u32;
    GetWindowThreadProcessId(if root.is_null() { hwnd } else { root }, &mut pid);
    pid == GetCurrentProcessId()
}

fn to_steps(events: &[(u32, Raw)]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut last: Option<u32> = None;
    for &(time, raw) in events {
        if let Some(prev) = last {
            let ms = time.wrapping_sub(prev);
            if ms > 0 {
                steps.push(Step::Delay { ms });
            }
        }
        last = Some(time);
        steps.push(match raw {
            Raw::Key { vk, scan, ext, down } => Step::Key { key: key_ref(vk, scan, ext), down },
            Raw::Mouse { button, down } => Step::Mouse { button, down },
        });
    }
    steps
}

pub fn key_ref(vk: u16, scan: u16, ext: bool) -> KeyRef {
    KeyRef { vk, scan, ext, name: key_name(vk, scan, ext) }
}

fn key_name(vk: u16, scan: u16, ext: bool) -> String {
    let lparam = ((scan as i32) << 16) | if ext { 1 << 24 } else { 0 };
    let mut buf = [0u16; 64];
    let len = unsafe { GetKeyNameTextW(lparam, buf.as_mut_ptr(), buf.len() as i32) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        format!("Key 0x{vk:02X}")
    }
}

fn send(inputs: &[INPUT]) {
    unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
}

pub fn send_key(key: &KeyRef, down: bool) {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    // Scan codes work in games that ignore virtual-key input.
    if key.scan != 0 {
        flags |= KEYEVENTF_SCANCODE;
    }
    if key.ext {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: key.vk, wScan: key.scan, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    send(&[input]);
}

pub fn send_mouse(button: MouseButton, down: bool) {
    let (flags, data) = match (button, down) {
        (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::Back, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
        (MouseButton::Back, false) => (MOUSEEVENTF_XUP, XBUTTON1),
        (MouseButton::Forward, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
        (MouseButton::Forward, false) => (MOUSEEVENTF_XUP, XBUTTON2),
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    };
    send(&[input]);
}
