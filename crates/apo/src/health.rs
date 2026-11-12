//! What the APOs in this audiodg process are doing, for the kit app to show.
//!
//! The real-time paths only bump atomics; the settings thread turns them into
//! a small JSON object and writes it to `HKLM\SOFTWARE\kit\Status` (value
//! [`VALUE`]) when it changes, at most every few seconds. The installer makes
//! that subkey writable by LOCAL SERVICE, the account audiodg runs as.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;

pub const KEY: &str = r"SOFTWARE\kit\Status";
pub const VALUE: &str = "Apo";

/// Mic streams locked right now, and how many of them have the denoiser.
pub static MIC_STREAMS: AtomicU32 = AtomicU32::new(0);
pub static MIC_DENOISING: AtomicU32 = AtomicU32::new(0);
/// The rate of the last mic stream that wasn't 48 kHz (passed through), or 0.
pub static MIC_OTHER_RATE: AtomicU32 = AtomicU32::new(0);
/// [`Model`] as a number.
pub static MODEL: AtomicU8 = AtomicU8::new(Model::NotLoaded as u8);
pub static MODEL_MS: AtomicU32 = AtomicU32::new(0);
/// Periods the denoiser had too little output for, since audiodg started.
pub static UNDERRUNS: AtomicU32 = AtomicU32::new(0);

pub static EQ_STREAMS: AtomicU32 = AtomicU32::new(0);
pub static EQ_RATE: AtomicU32 = AtomicU32::new(0);
/// The app asked for the EQ to run.
pub static EQ_ON: AtomicBool = AtomicBool::new(false);
/// The curve in the registry didn't parse, so the EQ passes audio through.
pub static CURVE_ERROR: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Model {
    NotLoaded,
    Loading,
    Loaded,
    Failed,
}

impl Model {
    fn name(v: u8) -> &'static str {
        match v {
            x if x == Model::Loading as u8 => "loading",
            x if x == Model::Loaded as u8 => "loaded",
            x if x == Model::Failed as u8 => "failed",
            _ => "notLoaded",
        }
    }
}

pub fn set_model(m: Model) {
    MODEL.store(m as u8, Ordering::Relaxed);
}

/// The status as JSON, the shape `sysaudio::apo::Health` reads.
pub fn json() -> String {
    let r = |a: &AtomicU32| a.load(Ordering::Relaxed);
    let b = |a: &AtomicBool| a.load(Ordering::Relaxed);
    format!(
        concat!(
            r#"{{"pid":{},"mic":{{"streams":{},"denoising":{},"otherRate":{},"model":"{}","modelMs":{},"underruns":{},"failed":{}}},"#,
            r#""eq":{{"streams":{},"rate":{},"on":{},"curveError":{},"failed":{}}}}}"#
        ),
        std::process::id(),
        r(&MIC_STREAMS),
        r(&MIC_DENOISING),
        r(&MIC_OTHER_RATE),
        Model::name(MODEL.load(Ordering::Relaxed)),
        r(&MODEL_MS),
        r(&UNDERRUNS),
        crate::mic_failed(),
        r(&EQ_STREAMS),
        r(&EQ_RATE),
        b(&EQ_ON),
        b(&CURVE_ERROR),
        crate::eq::failed(),
    )
}

/// Write the status if it changed since the last write. Not for the
/// real-time thread.
pub fn publish() {
    static LAST: Mutex<String> = Mutex::new(String::new());
    let now = json();
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if *last == now {
        return;
    }
    if write(&now) {
        *last = now;
    }
}

#[cfg(windows)]
fn write(text: &str) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_SZ};
    if cfg!(test) {
        return true;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (key_name, value_name, data) = (wide(KEY), wide(VALUE), wide(text));
    let mut key = HKEY::default();
    unsafe {
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(key_name.as_ptr()), None, KEY_SET_VALUE, &mut key).is_err() {
            return false;
        }
        let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let ok = RegSetValueExW(key, PCWSTR(value_name.as_ptr()), None, REG_SZ, Some(bytes)).is_ok();
        let _ = RegCloseKey(key);
        ok
    }
}

#[cfg(not(windows))]
fn write(_: &str) -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_is_json() {
        let text = super::json();
        assert!(text.starts_with(r#"{"pid":"#) && text.contains(r#""model":"#) && text.ends_with("}}"), "{text}");
    }
}
