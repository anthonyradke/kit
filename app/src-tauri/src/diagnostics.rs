//! One text file with what's needed to look into a problem on someone else's
//! PC: versions, what kit sees of each device and of Windows audio, the
//! settings, and the end of kit's log and the audio engine's. Nothing is sent
//! anywhere; the user saves the file and passes it on.

use std::fmt::Write;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::{backup, headset, keyboard, logging, mouse, setup, store};

/// Lines of each log to include, from the end.
const LOG_LINES: usize = 600;
const APO_LOG_LINES: usize = 200;

fn json(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("(couldn't write it out: {e})"))
}

fn tail(path: &Path, lines: usize) -> String {
    match std::fs::read(path) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let all: Vec<&str> = text.lines().collect();
            all[all.len().saturating_sub(lines)..].join("\n")
        }
        Err(e) => format!("({}: {e})", path.display()),
    }
}

fn section(out: &mut String, title: &str, body: &str) {
    let _ = write!(out, "\n== {title} ==\n{}\n", body.trim_end());
}

/// Every audio endpoint and the defaults, as Windows reports them.
fn endpoints() -> String {
    std::thread::scope(|s| {
        s.spawn(|| {
            sysaudio::com_init();
            let mut out = String::new();
            for flow in [sysaudio::Flow::Output, sysaudio::Flow::Input] {
                let default = |comm| sysaudio::default_endpoint(flow, comm).ok().flatten().map(|e| e.id);
                let (main, comm) = (default(false), default(true));
                match sysaudio::endpoints(flow) {
                    Ok(eps) => {
                        for e in eps {
                            let mark = match (main.as_ref() == Some(&e.id), comm.as_ref() == Some(&e.id)) {
                                (true, true) => " [default, calls]",
                                (true, false) => " [default]",
                                (false, true) => " [calls]",
                                _ => "",
                            };
                            let _ = writeln!(out, "{flow:?}: {} ({}){mark}", e.name, e.device);
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(out, "{flow:?}: couldn't list ({e})");
                    }
                }
            }
            out
        })
        .join()
        .unwrap_or_else(|_| "(listing the endpoints failed)".into())
    })
}

/// The whole report.
pub fn report(app: &AppHandle) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "kit diagnostics, {}", logging::file_stamp());
    let _ = writeln!(out, "kit {} on {}", app.package_info().version, logging::windows_version());

    section(&mut out, "Devices picked in setup", &json(&setup::get(app)));
    section(&mut out, "Vendor apps running", &format!("{:?}", setup::vendor_apps()));
    let problems = store::problems();
    section(&mut out, "Settings files kit couldn't read", &if problems.is_empty() { "none".into() } else { problems.join("\n") });

    let m = app.state::<mouse::state::Shared>();
    section(&mut out, "Mouse", &json(&*m.status.lock().unwrap()));
    let k = app.state::<keyboard::state::Shared>();
    let mut kb = serde_json::to_value(&*k.status.lock().unwrap()).unwrap_or(Value::Null);
    if let Some(o) = kb.as_object_mut() {
        // Hundreds of numbers that say nothing about a problem.
        for long in ["keymap", "macros", "zones"] {
            o.remove(long);
        }
    }
    section(&mut out, "Keyboard", &json(&kb));
    let h = app.state::<headset::state::Shared>();
    section(&mut out, "Headset", &json(&*h.headset.lock().unwrap()));
    section(&mut out, "Audio (kit's view, with what its audio effects report)", &json(&*h.audio.lock().unwrap()));
    section(&mut out, "Surround", &json(&*h.surround.lock().unwrap()));
    section(&mut out, "Windows audio endpoints", &endpoints());

    section(&mut out, "mouse.json (as kit runs it)", &json(&m.core.lock().unwrap().config));
    section(&mut out, "keyboard.json (as kit runs it)", &json(&*k.core.lock().unwrap()));
    section(&mut out, "headset.json (as kit runs it)", &json(&*h.core.lock().unwrap()));

    if let Ok(dir) = app.path().app_log_dir() {
        let mut log = String::new();
        if logging::old_log_path(&dir).exists() {
            log += &tail(&logging::old_log_path(&dir), LOG_LINES);
            log += "\n";
        }
        log += &tail(&logging::log_path(&dir), LOG_LINES);
        section(&mut out, "kit's log", &log);
    }
    section(&mut out, "Audio engine log (kit's audio effects)", &tail(&sysaudio::apo::log_path(), APO_LOG_LINES));
    out
}

/// Save the report where the user picks. Its path, or nothing if they cancelled.
#[tauri::command(async)]
pub fn diagnostics_export(app: AppHandle, file_name: String) -> Result<Option<String>, String> {
    let Some(path) = backup::file_dialog(&app, ("Text", "txt"), move |d| d.set_file_name(file_name).save_file()) else { return Ok(None) };
    let text = report(&app);
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    log::info!("saved diagnostics to {}", path.display());
    Ok(Some(path.display().to_string()))
}

/// Show kit's log folder in Explorer.
#[tauri::command]
pub fn open_log_folder(app: AppHandle) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    let _ = std::fs::create_dir_all(&dir);
    let dir = HSTRING::from(dir.as_os_str());
    unsafe { ShellExecuteW(None, w!("open"), PCWSTR(dir.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    Ok(())
}
