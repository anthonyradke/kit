//! Background thread watching Windows audio endpoints.
//!
//! When the Nova 5's endpoints appear (startup, dongle plugged in, endpoint
//! recreated by a driver update) and "keep default" is on, they become the
//! default output and input for every role. They are not forced back if you
//! later pick another device yourself.
//!
//! With surround on, apps play to VB-CABLE instead (the default for
//! everything but calls) and the surround thread passes it on to the headset;
//! calls and the mic stay on the headset. Switching surround on or off moves
//! the defaults right away.

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use serde::Serialize;
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
    /// kit's defaults are in place: the headset for both directions and all
    /// roles, or with surround on, the cable for apps and the headset for calls.
    pub headset_is_default: bool,
    /// With surround on: VB-CABLE's input, which apps play to.
    pub cable: Option<String>,
    /// Noise suppression APO installed on the headset mic.
    pub apo_installed: bool,
    /// Installed, but an older DLL than the one this version of kit ships.
    pub apo_outdated: bool,
    /// kit's EQ APO installed on the headset output.
    pub eq_apo_installed: bool,
    pub eq_apo_outdated: bool,
    pub error: Option<String>,
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
    let mut seen: [Option<String>; 3] = [None, None, None];
    let mut was_surround = None;
    loop {
        let mut force = false;
        match rx.recv_timeout(POLL_EVERY) {
            Ok(Cmd::UseHeadset) => force = true,
            Ok(Cmd::Refresh) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let (keep_default, surround) = {
            let shared = app.state::<Shared>();
            let core = shared.core.lock().unwrap();
            (core.keep_default, core.headset.surround)
        };
        let switched = was_surround.is_some_and(|w| w != surround) || (was_surround.is_none() && surround);
        was_surround = Some(surround);
        let next = match scan(surround) {
            Ok(eps) => {
                let ids = [&eps.output, &eps.input, &eps.cable].map(|e| e.as_ref().map(|e| e.id.clone()));
                let appeared = ids.iter().zip(&seen).any(|(now, before)| now.is_some() && now != before);
                seen = ids;
                let error = if force || switched || ((keep_default || surround) && appeared) { apply(&eps) } else { None };
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
    /// Only looked for with surround on.
    cable: Option<Endpoint>,
}

fn scan(surround: bool) -> sysaudio::Result<Endpoints> {
    Ok(Endpoints {
        output: sysaudio::find_arctis(Flow::Output)?,
        input: sysaudio::find_arctis(Flow::Input)?,
        cable: if surround { sysaudio::find_cable()? } else { None },
    })
}

/// Set kit's defaults; returns what failed.
fn apply(eps: &Endpoints) -> Option<String> {
    let mut error = None;
    let mut check = |r: sysaudio::Result<()>, name: &str| {
        if let Err(e) = r {
            error = Some(format!("Couldn't set {name}: {e}"));
        }
    };
    match (&eps.output, &eps.cable) {
        (Some(out), Some(cable)) => {
            check(sysaudio::set_main_default(&cable.id), &cable.name);
            check(sysaudio::set_communications_default(&out.id), &out.name);
        }
        (Some(out), None) => check(sysaudio::set_default(&out.id), &out.name),
        _ => {}
    }
    if let Some(inp) = &eps.input {
        check(sysaudio::set_default(&inp.id), &inp.name);
    }
    error
}

/// kit's EQ is installed on the headset's output (looked up now, not from
/// the last scan, so the first settings pushed at startup are right).
pub fn eq_apo_installed() -> bool {
    sysaudio::com_init();
    let Ok(Some(out)) = sysaudio::find_arctis(Flow::Output) else { return false };
    let s = sysaudio::apo::status(&out, sysaudio::apo::Effect::HeadsetEq);
    s.registered && s.on_endpoint
}

/// Put the headset back as the default for everything (quitting with surround on).
pub fn restore_headset() {
    sysaudio::com_init();
    if let Ok(Some(out)) = sysaudio::find_arctis(Flow::Output) {
        let _ = sysaudio::set_default(&out.id);
    }
}

fn describe(eps: Endpoints, error: Option<String>) -> Status {
    let Endpoints { output: out, input: inp, cable } = eps;
    let defaults = |flow| [false, true].map(|comm| sysaudio::default_endpoint(flow, comm).ok().flatten());
    let [out_def, out_comm] = defaults(Flow::Output);
    let [in_def, in_comm] = defaults(Flow::Input);
    let is = |ep: &Option<Endpoint>, d: &Option<Endpoint>| matches!((ep, d), (Some(a), Some(b)) if a.id == b.id);
    let main_output = if cable.is_some() { &cable } else { &out };
    let headset_is_default = out.is_some()
        && inp.is_some()
        && is(main_output, &out_def)
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
        output: out.map(|e| e.name),
        input: inp.map(|e| e.name),
        default_output: out_def.map(|e| e.name),
        default_input: in_def.map(|e| e.name),
        cable: cable.map(|e| e.name),
        headset_is_default,
        error,
    }
}
