//! kit's log file, `%LOCALAPPDATA%\com.anthonyradke.kit\logs\kit.log`.
//!
//! The release build has no console, so this is where device errors, settings
//! problems and panics go. The file is kept under 1 MB, with the previous one
//! as `kit.1.log`, and both go into the diagnostics export.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};

const MAX_BYTES: u64 = 1024 * 1024;

struct Logger {
    file: Mutex<Option<(File, u64)>>,
    path: PathBuf,
}

static LOGGER: std::sync::OnceLock<Logger> = std::sync::OnceLock::new();

pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("kit.log")
}

pub fn old_log_path(dir: &Path) -> PathBuf {
    dir.join("kit.1.log")
}

/// Local time as `2026-11-12 14:03:07.215`.
fn timestamp() -> String {
    #[cfg(windows)]
    unsafe {
        let mut t = std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
        windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
    }
    #[cfg(not(windows))]
    {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        format!("{}.{:03}", d.as_secs(), d.subsec_millis())
    }
}

fn open(path: &Path) -> Option<(File, u64)> {
    let file = OpenOptions::new().create(true).append(true).open(path).ok()?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    Some((file, len))
}

impl Logger {
    fn write_line(&self, line: &str) {
        if cfg!(debug_assertions) {
            eprintln!("{line}");
        }
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        if guard.as_ref().is_some_and(|(_, len)| *len > MAX_BYTES) {
            *guard = None;
            if let Some(dir) = self.path.parent() {
                let _ = std::fs::rename(&self.path, old_log_path(dir));
            }
        }
        if guard.is_none() {
            *guard = open(&self.path);
        }
        if let Some((file, len)) = guard.as_mut() {
            if writeln!(file, "{line}").is_ok() {
                *len += line.len() as u64 + 1;
            }
        }
    }
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        // kit's own modules at Info, the libraries only when something is wrong.
        let ours = m.target().starts_with("kit") || m.target().starts_with("hidpp") || m.target().starts_with("via") || m.target().starts_with("arctis") || m.target().starts_with("sysaudio");
        m.level() <= if ours { Level::Info } else { Level::Warn }
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let target = record.target().strip_prefix("kit::").unwrap_or(record.target());
            self.write_line(&format!("{} {:<5} {target}: {}", timestamp(), record.level(), record.args()));
        }
    }

    fn flush(&self) {
        if let Some((file, _)) = self.file.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            let _ = file.flush();
        }
    }
}

/// Start logging to `dir`, and log panics (with the thread they happened on)
/// before the default handler runs.
pub fn init(dir: &Path, version: &str) {
    let _ = std::fs::create_dir_all(dir);
    let logger = LOGGER.get_or_init(|| Logger { file: Mutex::new(None), path: log_path(dir) });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        let backtrace = std::backtrace::Backtrace::force_capture();
        log::error!(target: "kit::panic", "thread '{name}' {info}\n{backtrace}");
        default(info);
    }));
    log::info!("kit {version} started on {}", windows_version());
}

/// "Windows 10.0.26100" from the kernel's own numbers (GetVersionEx lies to
/// apps without a manifest entry for each release).
pub fn windows_version() -> String {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
        #[repr(C)]
        struct OsVersionInfo {
            size: u32,
            major: u32,
            minor: u32,
            build: u32,
            platform: u32,
            csd: [u16; 128],
        }
        let ntdll: Vec<u16> = "ntdll.dll\0".encode_utf16().collect();
        let module = GetModuleHandleW(ntdll.as_ptr());
        if !module.is_null() {
            if let Some(f) = GetProcAddress(module, c"RtlGetVersion".as_ptr().cast()) {
                let f: unsafe extern "system" fn(*mut OsVersionInfo) -> i32 = std::mem::transmute(f);
                let mut v = OsVersionInfo { size: std::mem::size_of::<OsVersionInfo>() as u32, major: 0, minor: 0, build: 0, platform: 0, csd: [0; 128] };
                if f(&mut v) == 0 {
                    return format!("Windows {}.{}.{}", v.major, v.minor, v.build);
                }
            }
        }
        "Windows".into()
    }
    #[cfg(not(windows))]
    {
        std::env::consts::OS.into()
    }
}

/// Record a device error for the window, logging it when it's a new one (the
/// workers retry every few seconds, and the log shouldn't fill with repeats).
pub fn set_error(slot: &mut Option<String>, device: &str, error: String) {
    if slot.as_ref() != Some(&error) {
        log::warn!(target: "kit::device", "{device}: {error}");
    }
    *slot = Some(error);
}
