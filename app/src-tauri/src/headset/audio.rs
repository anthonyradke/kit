//! Background thread watching Windows audio endpoints.
//!
//! When the Nova 5's endpoints appear (startup, dongle plugged in, endpoint
//! recreated by a driver update) and "keep default" is on, they become the
//! default output and input for every role. They are not forced back if you
//! later pick another device yourself.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sysaudio::{Endpoint, Flow};
use tauri::{AppHandle, Emitter, Manager};

use super::state::Shared;

pub enum Cmd {
    /// Make the headset the default now.
    UseHeadset,
    /// Re-check right away (after installing the APO, toggling keep-default).
    Refresh,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub output: Option<String>,
    pub input: Option<String>,
    pub default_output: Option<String>,
    pub default_input: Option<String>,
    /// The headset is the default for both directions and all roles.
    pub headset_is_default: bool,
    /// Noise suppression APO installed on the headset mic.
    pub apo_installed: bool,
    /// Installed, but an older DLL than the one this version of kit ships.
    pub apo_outdated: bool,
    /// kit's EQ APO installed on the headset output.
    pub eq_apo_installed: bool,
    pub eq_apo_outdated: bool,
    /// What kit's APOs in the audio engine say about themselves, when one
    /// has loaded since the audio service started.
    pub apo_health: Option<ApoHealth>,
    pub error: Option<String>,
    /// A scan has finished (the window doesn't need this).
    #[serde(skip)]
    pub scanned: bool,
}

/// `kit_apo::health`'s report, read back from the registry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ApoHealth {
    /// The audiodg process that wrote it.
    pub pid: u32,
    pub mic: MicHealth,
    pub eq: EqHealth,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MicHealth {
    /// Mic streams open now, and how many of them run the denoiser.
    pub streams: u32,
    pub denoising: u32,
    /// A mic stream at another rate than 48 kHz (passed through), if any.
    pub other_rate: u32,
    /// "notLoaded", "loading", "loaded" or "failed".
    pub model: String,
    pub model_ms: u32,
    /// Times the denoiser fell behind since the audio service started.
    pub underruns: u32,
    /// It hit an error and passes the mic through until the service restarts.
    pub failed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EqHealth {
    pub streams: u32,
    pub rate: u32,
    pub on: bool,
    pub curve_error: bool,
    pub failed: bool,
}

/// The APOs' report, if the audio engine process that wrote it still runs
/// (a report from before the service restarted says nothing about now).
pub fn apo_health() -> Option<ApoHealth> {
    let health: ApoHealth = serde_json::from_str(&sysaudio::apo::health()?).ok()?;
    let alive = crate::setup::processes().iter().any(|(pid, name)| *pid == health.pid && name == "audiodg.exe");
    alive.then_some(health)
}

const POLL_EVERY: Duration = Duration::from_secs(2);

pub fn spawn(app: AppHandle, rx: Receiver<Cmd>) {
    std::thread::Builder::new()
        .name("audio".into())
        .spawn(move || run(app, rx))
        .expect("spawn audio thread");
}

fn run(app: AppHandle, rx: Receiver<Cmd>) {
    sysaudio::com_init();
    let mut status = Status::default();
    // Endpoint ids seen last time; a new id means a device (re)appeared.
    let mut seen: [Option<String>; 2] = [None, None];
    loop {
        let mut force = false;
        match rx.recv_timeout(POLL_EVERY) {
            Ok(Cmd::UseHeadset) => force = true,
            Ok(Cmd::Refresh) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let keep_default = app.state::<Shared>().core.lock().unwrap().keep_default;
        let next = match scan() {
            Ok(eps) => {
                let ids = [&eps.output, &eps.input].map(|e| e.as_ref().map(|e| e.id.clone()));
                let appeared = ids.iter().zip(&seen).any(|(now, before)| now.is_some() && now != before);
                seen = ids;
                let error = if force || (keep_default && appeared) { apply(&eps) } else { None };
                describe(eps, error)
            }
            Err(e) => Status { error: Some(e.to_string()), ..Status::default() },
        };
        if next != status {
            status = next;
            *app.state::<Shared>().audio.lock().unwrap() = status.clone();
            let _ = app.emit("audio", &status);
        }
    }
}

struct Endpoints {
    output: Option<Endpoint>,
    input: Option<Endpoint>,
}

fn scan() -> sysaudio::Result<Endpoints> {
    Ok(Endpoints { output: sysaudio::find_arctis(Flow::Output)?, input: sysaudio::find_arctis(Flow::Input)? })
}

/// Set kit's defaults; returns what failed.
fn apply(eps: &Endpoints) -> Option<String> {
    let mut error = None;
    for ep in [&eps.output, &eps.input].into_iter().flatten() {
        if let Err(e) = sysaudio::set_default(&ep.id) {
            error = Some(format!("Couldn't set {}: {e}", ep.name));
        }
    }
    error
}

fn describe(eps: Endpoints, error: Option<String>) -> Status {
    let Endpoints { output: out, input: inp } = eps;
    let defaults = |flow| [false, true].map(|comm| sysaudio::default_endpoint(flow, comm).ok().flatten());
    let [out_def, out_comm] = defaults(Flow::Output);
    let [in_def, in_comm] = defaults(Flow::Input);
    let is = |ep: &Option<Endpoint>, d: &Option<Endpoint>| matches!((ep, d), (Some(a), Some(b)) if a.id == b.id);
    let headset_is_default = out.is_some()
        && inp.is_some()
        && is(&out, &out_def)
        && is(&out, &out_comm)
        && is(&inp, &in_def)
        && is(&inp, &in_comm);
    let installed = |ep: &Option<Endpoint>, effect| {
        ep.as_ref().is_some_and(|ep| {
            let s = sysaudio::apo::status(ep, effect);
            s.registered && s.on_endpoint
        })
    };
    let apo_installed = installed(&inp, sysaudio::apo::Effect::MicDenoise);
    let eq_apo_installed = installed(&out, sysaudio::apo::Effect::HeadsetEq);
    let outdated = || super::elevate::apo_dll().is_some_and(|dll| !sysaudio::apo::is_current(&dll));
    Status {
        apo_installed,
        apo_outdated: apo_installed && outdated(),
        eq_apo_installed,
        eq_apo_outdated: eq_apo_installed && outdated(),
        apo_health: (apo_installed || eq_apo_installed).then(apo_health).flatten(),
        output: out.map(|e| e.name),
        input: inp.map(|e| e.name),
        default_output: out_def.map(|e| e.name),
        default_input: in_def.map(|e| e.name),
        headset_is_default,
        error,
        scanned: true,
    }
}
