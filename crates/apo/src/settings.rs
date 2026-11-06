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
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

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
    let mut size = 0u32;
    unsafe { RegGetValueW(key, None, name, RRF_RT_REG_SZ, None, None, Some(&mut size)) }.ok().ok()?;
    let mut buf = vec![0u16; size as usize / 2 + 1];
    let mut size = (buf.len() * 2) as u32;
    unsafe { RegGetValueW(key, None, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut size)) }
        .ok()
        .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

/// The headset EQ as the registry has it. A curve that doesn't parse turns
/// the EQ off rather than playing something else.
fn read_eq(key: HKEY) -> crate::eq::Wanted {
    let text = read_string(key, VALUE_EQ).unwrap_or_default();
    let curve = kit_eq::Curve::parse(&text);
    if let Err(e) = &curve {
        crate::log(&format!("headset EQ: unreadable curve ({e})"));
    }
    let on = read_dword(key, VALUE_EQ_ON).unwrap_or(0) != 0 && curve.is_ok();
    crate::eq::Wanted { on, curve: curve.unwrap_or_default() }
}

fn watch() {
    loop {
        let mut key = HKEY::default();
        if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, KEY, None, KEY_READ | KEY_NOTIFY, &mut key) }.is_err() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        loop {
            if let Some(v) = read_dword(key, VALUE_ENABLED) {
                ENABLED.store(v != 0, Ordering::Relaxed);
            }
            if let Some(v) = read_dword(key, VALUE_STRENGTH) {
                STRENGTH.store(v.min(100), Ordering::Relaxed);
            }
            crate::eq::set_wanted(read_eq(key));
            // Blocks until a value under the key changes.
            let r = unsafe { RegNotifyChangeKeyValue(key, false, REG_NOTIFY_CHANGE_LAST_SET, None, false) };
            if r.is_err() {
                break;
            }
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
