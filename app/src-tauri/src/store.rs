//! Reading and writing kit's JSON settings files.
//!
//! A file that can't be read is never silently replaced: one that doesn't
//! parse is copied aside first, and one that can't even be opened (another
//! program holding it at sign-in, say) is retried, then left alone for the
//! rest of the session so the defaults kit falls back to never overwrite it.
//! Either way the problem is kept for the window and the diagnostics file.

use std::collections::HashSet;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PROTECTED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

/// What went wrong reading settings this session, oldest first.
pub fn problems() -> Vec<String> {
    PROBLEMS.lock().unwrap().clone()
}

fn report(problem: String) {
    log::warn!("{problem}");
    PROBLEMS.lock().unwrap().push(problem);
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Read `path` as text, from UTF-8 (with or without a byte order mark) or
/// the UTF-16 Notepad can save as.
fn read_text(path: &Path) -> std::io::Result<String> {
    let bytes = std::fs::read(path)?;
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16(&units).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e));
    }
    let text = String::from_utf8(bytes).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// Copy an unusable file next to itself under a name that doesn't replace an
/// earlier copy. The copy's name, if it worked.
fn set_aside(path: &Path) -> Option<String> {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let copy = path.with_extension(format!("json.{secs}.bak"));
    std::fs::copy(path, &copy).ok().map(|_| file_name(&copy))
}

/// The settings in `path`, or `T::default()` when there are none yet or they
/// can't be used.
pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T {
    let mut tries = 0;
    let text = loop {
        match read_text(path) {
            Ok(text) => break text,
            Err(e) if e.kind() == ErrorKind::NotFound => return T::default(),
            // Antivirus or a sync client can hold the file for a moment at sign-in.
            Err(e) if tries < 10 && e.kind() != ErrorKind::InvalidData => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let name = file_name(path);
                match set_aside(path) {
                    Some(copy) => report(format!("{name} couldn't be read ({e}), so kit started with defaults. The old file is kept as {copy}.")),
                    None => {
                        PROTECTED.lock().unwrap().get_or_insert_with(HashSet::new).insert(path.to_owned());
                        report(format!("{name} couldn't be read ({e}). kit is using defaults and won't save over the file until it restarts."));
                    }
                }
                return T::default();
            }
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            let name = file_name(path);
            match set_aside(path) {
                Some(copy) => report(format!("{name} wasn't valid ({e}), so kit started with defaults. The old file is kept as {copy}.")),
                None => {
                    PROTECTED.lock().unwrap().get_or_insert_with(HashSet::new).insert(path.to_owned());
                    report(format!("{name} wasn't valid ({e}). kit is using defaults and won't save over the file until it restarts."));
                }
            }
            T::default()
        }
    }
}

/// Write `value` to `path` so that a crash or power cut leaves either the old
/// file or the new one, never half of one.
pub fn save<T: Serialize + ?Sized>(path: &Path, value: &T) -> std::io::Result<()> {
    if PROTECTED.lock().unwrap().as_ref().is_some_and(|p| p.contains(path)) {
        return Err(std::io::Error::other(format!("{} is kept as it is until kit restarts", file_name(path))));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(tmp, path)
}

/// Whether `id` is safe to use as a profile, macro or preset id: it ends up
/// in the window's HTML and in file names.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_from_a_file_are_checked() {
        for id in ["18c9f0a1b2c3", "seed-main", "a_b"] {
            assert!(valid_id(id), "{id}");
        }
        for id in ["", "x\"><img src=n onerror=alert(1)>", "a b", "é", &"a".repeat(65)] {
            assert!(!valid_id(id), "{id}");
        }
    }

    #[test]
    fn notepad_utf16_reads() {
        let dir = std::env::temp_dir().join(format!("kit-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.json");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("{\"x\":1}".encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(read_text(&path).unwrap(), "{\"x\":1}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_broken_file_is_kept_and_defaults_load() {
        let dir = std::env::temp_dir().join(format!("kit-store-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("b.json");
        std::fs::write(&path, "{ not json").unwrap();
        let value: Vec<u32> = load(&path);
        assert!(value.is_empty());
        let kept = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".bak")).count();
        assert_eq!(kept, 1);
        save(&path, &vec![1u32]).unwrap();
        assert_eq!(load::<Vec<u32>>(&path), vec![1]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
