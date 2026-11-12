//! Live settings shared with the kit app through the registry: the mic's
//! noise suppression and the headset EQ.
//!
//! The app writes `HKLM\SOFTWARE\kit` (the installer grants Users write
//! access to that key); one thread per audiodg process waits for changes and
//! publishes them as atomics the real-time path can read without locking.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Once;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_MORE_DATA, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

pub const KEY: PCWSTR = w!("SOFTWARE\\kit");
pub const VALUE_ENABLED: PCWSTR = w!("MicDenoise");
pub const VALUE_STRENGTH: PCWSTR = w!("MicStrength");
/// DWORD: the headset EQ runs (1) or passes audio through (0).
pub const VALUE_EQ_ON: PCWSTR = w!("HeadsetEqOn");
/// REG_SZ: the curve, in Equalizer APO's text format (`kit_eq::Curve`).
pub const VALUE_EQ: PCWSTR = w!("HeadsetEq");

static ENABLED: AtomicBool = AtomicBool::new(true);
/// Percent, 0..=100.
static STRENGTH: AtomicU32 = AtomicU32::new(90);
static START: Once = Once::new();

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn strength() -> f32 {
    STRENGTH.load(Ordering::Relaxed).min(100) as f32 / 100.0
}

/// Start the watcher thread (once per process). Not in tests, which set
/// what they need themselves and mustn't pick up this PC's real settings.
pub fn start() {
    if cfg!(test) {
        return;
    }
    START.call_once(|| {
        let _ = std::thread::Builder::new().name("kit-settings".into()).spawn(watch);
    });
}

fn read_dword(key: HKEY, name: PCWSTR) -> Option<u32> {
    let mut value = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(key, None, name, RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as *mut _), Some(&mut size))
    };
    r.is_ok().then_some(value)
}

fn read_string(key: HKEY, name: PCWSTR) -> Option<String> {
    // The value can grow between asking its size and reading it, when the
    // app writes a longer curve right then: ask again.
    for _ in 0..3 {
        let mut size = 0u32;
        unsafe { RegGetValueW(key, None, name, RRF_RT_REG_SZ, None, None, Some(&mut size)) }.ok().ok()?;
        let mut buf = vec![0u16; size as usize / 2 + 1];
        let mut size = (buf.len() * 2) as u32;
        let r = unsafe { RegGetValueW(key, None, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) };
        if r == ERROR_MORE_DATA {
            continue;
        }
        r.ok().ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        return Some(String::from_utf16_lossy(&buf[..len]));
    }
    None
}

/// The headset EQ as the registry has it. A curve that doesn't parse turns
/// the EQ off rather than playing something else; one that can't be read
/// right now (it's being rewritten) leaves the last good one running.
fn read_eq(key: HKEY, last: &mut Option<kit_eq::Curve>) -> crate::eq::Wanted {
    let on = read_dword(key, VALUE_EQ_ON).unwrap_or(0) != 0;
    let curve = match read_string(key, VALUE_EQ) {
        Some(text) => match kit_eq::Curve::parse(&text) {
            Ok(c) => Some(c),
            Err(e) => {
                crate::log(&format!("headset EQ: unreadable curve ({e})"));
                None
            }
        },
        None => last.clone(),
    };
    last.clone_from(&curve);
    crate::health::EQ_ON.store(on, Ordering::Relaxed);
    crate::health::CURVE_ERROR.store(on && curve.is_none(), Ordering::Relaxed);
    crate::eq::Wanted { on: on && curve.is_some(), curve: curve.unwrap_or_default() }
}

fn watch() {
    // Auto-reset: set by the registry when a value changes.
    let Ok(changed) = (unsafe { CreateEventW(None, false, false, None) }) else {
        crate::log("settings: couldn't create the change event");
        return;
    };
    let mut last_curve = None;
    loop {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, KEY, None, KEY_READ | KEY_NOTIFY, &mut key) }.is_err() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        loop {
            // Armed before reading, so a change made while the values are
            // being read (the app writes several in a row) sets the event
            // and is read on the next pass instead of being missed.
            let r = unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, Some(changed), true) };
            if let Some(v) = read_dword(key, VALUE_ENABLED) {
                ENABLED.store(v != 0, Ordering::Relaxed);
            }
            if let Some(v) = read_dword(key, VALUE_STRENGTH) {
                STRENGTH.store(v.min(100), Ordering::Relaxed);
            }
            crate::eq::set_wanted(read_eq(key, &mut last_curve));
            if r.is_err() {
                break;
            }
            // Between changes, publish what the APOs are doing every few seconds.
            let w = loop {
                crate::health::publish();
                let w = unsafe { WaitForSingleObject(changed, 5000) };
                if w != WAIT_TIMEOUT {
                    break w;
                }
            };
            if w != WAIT_OBJECT_0 {
                break;
            }
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
