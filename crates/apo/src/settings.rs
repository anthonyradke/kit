//! Live settings shared with the headroom app through the registry.
//!
//! The app writes `HKLM\SOFTWARE\headroom` (the installer grants Users write
//! access to that key); one thread per audiodg process waits for changes and
//! publishes them as atomics the real-time path can read without locking.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Once;
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegGetValueW, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE,
    KEY_NOTIFY, KEY_READ, REG_NOTIFY_CHANGE_LAST_SET, RRF_RT_REG_DWORD,
};

pub const KEY: PCWSTR = w!("SOFTWARE\\headroom");
pub const VALUE_ENABLED: PCWSTR = w!("MicDenoise");
pub const VALUE_STRENGTH: PCWSTR = w!("MicStrength");

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

/// Start the watcher thread (once per process).
pub fn start() {
    START.call_once(|| {
        let _ = std::thread::Builder::new().name("headroom-settings".into()).spawn(watch);
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
