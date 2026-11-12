//! Self-update from GitHub Releases. kit checks quietly in the background and
//! only installs when asked (Settings or the tray); no popups.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::tray;

/// How long a check may take, and the download of an update.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Let startup (and sign-in, when launched with Windows) settle before the first check.
const FIRST_CHECK: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    pending: Mutex<Option<Update>>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub current: String,
    /// Newer version on GitHub, if any.
    pub available: Option<String>,
    pub notes: Option<String>,
    pub checking: bool,
    /// A check has finished at least once.
    pub checked: bool,
    /// Download progress in percent while installing.
    pub progress: Option<u8>,
    pub error: Option<String>,
}

pub fn init(app: &AppHandle) {
    let status = Status { current: app.package_info().version.to_string(), ..Status::default() };
    app.manage(Shared { status: Mutex::new(status), ..Shared::default() });
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            tauri::async_runtime::block_on(check(&app));
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

pub fn status(app: &AppHandle) -> Status {
    app.state::<Shared>().status.lock().unwrap().clone()
}

fn set(app: &AppHandle, f: impl FnOnce(&mut Status)) {
    let status = {
        let shared = app.state::<Shared>();
        let mut s = shared.status.lock().unwrap();
        f(&mut s);
        s.clone()
    };
    let _ = app.emit("update", status);
}

pub async fn check(app: &AppHandle) {
    let busy = {
        let s = app.state::<Shared>();
        let s = s.status.lock().unwrap();
        s.checking || s.progress.is_some()
    };
    if busy {
        return;
    }
    set(app, |s| s.checking = true);
    let h = app.clone();
    let updater = app
        .updater_builder()
        .timeout(CHECK_TIMEOUT)
        // On Windows the installer starts and kit ends right away, without
        // the usual exit: undo what kit changed first. This replaces the
        // plugin's own hook, so that runs too.
        .on_before_exit(move || {
            crate::before_exit(&h);
            h.cleanup_before_exit();
        })
        .build();
    let result = match updater {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let had_update = status(app).available.is_some();
    match result {
        Ok(update) => {
            let (available, notes) = match &update {
                Some(u) => (Some(u.version.clone()), u.body.clone().filter(|b| !b.trim().is_empty())),
                None => (None, None),
            };
            *app.state::<Shared>().pending.lock().unwrap() = update;
            set(app, |s| {
                s.available = available;
                s.notes = notes;
                s.error = None;
            });
        }
        // Offline is normal for a tray app; keep whatever we knew and say why in Settings.
        Err(e) => set(app, |s| s.error = Some(e.to_string())),
    }
    set(app, |s| {
        s.checking = false;
        s.checked = true;
    });
    if had_update != status(app).available.is_some() {
        tray::refresh(app);
    }
}

/// Download, verify and run the installer. On success the process exits and
/// the installer starts the new kit, so this only returns on failure.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let mut update = app.state::<Shared>().pending.lock().unwrap().clone().ok_or("No update to install.")?;
    update.timeout = Some(DOWNLOAD_TIMEOUT);
    if status(app).progress.is_some() {
        return Ok(());
    }
    set(app, |s| {
        s.progress = Some(0);
        s.error = None;
    });
    let mut done = 0u64;
    let result = update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                if let Some(total) = total.filter(|&t| t > 0) {
                    let pct = (done * 100 / total).min(100) as u8;
                    if status(app).progress != Some(pct) {
                        set(app, |s| s.progress = Some(pct));
                    }
                }
            },
            || {},
        )
        .await;
    let err = match result {
        Ok(()) => "The installer didn't start.".to_string(),
        Err(e) => e.to_string(),
    };
    set(app, |s| {
        s.progress = None;
        s.error = Some(err.clone());
    });
    Err(err)
}

#[tauri::command]
pub fn update_state(app: AppHandle) -> Status {
    status(&app)
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Status {
    check(&app).await;
    status(&app)
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}
