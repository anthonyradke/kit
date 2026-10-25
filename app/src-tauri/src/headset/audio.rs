//! Background thread watching Windows audio endpoints.
//!
//! When the Nova 5's endpoints appear (startup, dongle plugged in, endpoint
//! recreated by a driver update) and "keep default" is on, they become the
//! default output and input for every role. They are not forced back if you
//! later pick another device yourself.

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
    /// The headset is the default for both directions (all roles).
    pub headset_is_default: bool,
    /// Noise suppression APO installed on the headset mic.
    pub apo_installed: bool,
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
    // Endpoint ids seen last time; a new id means the headset (re)appeared.
    let mut seen: (Option<String>, Option<String>) = (None, None);
    loop {
        let mut force = false;
        match rx.recv_timeout(POLL_EVERY) {
            Ok(Cmd::UseHeadset) => force = true,
            Ok(Cmd::Refresh) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let keep_default = app.state::<Shared>().core.lock().unwrap().keep_default;
        let next = match scan() {
            Ok((out, inp)) => {
                let appeared = (out.as_ref().map(|e| &e.id) != seen.0.as_ref() && out.is_some())
                    || (inp.as_ref().map(|e| &e.id) != seen.1.as_ref() && inp.is_some());
                seen = (out.as_ref().map(|e| e.id.clone()), inp.as_ref().map(|e| e.id.clone()));
                let mut error = None;
                if force || (keep_default && appeared) {
                    for ep in out.iter().chain(inp.iter()) {
                        if let Err(e) = sysaudio::set_default(&ep.id) {
                            error = Some(format!("Couldn't set {}: {e}", ep.name));
                        }
                    }
                }
                describe(out, inp, error)
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

fn scan() -> sysaudio::Result<(Option<Endpoint>, Option<Endpoint>)> {
    Ok((sysaudio::find_arctis(Flow::Output)?, sysaudio::find_arctis(Flow::Input)?))
}

fn describe(out: Option<Endpoint>, inp: Option<Endpoint>, error: Option<String>) -> Status {
    let defaults = |flow| {
        [false, true].map(|comm| sysaudio::default_endpoint(flow, comm).ok().flatten())
    };
    let [out_def, out_comm] = defaults(Flow::Output);
    let [in_def, in_comm] = defaults(Flow::Input);
    let is = |ep: &Option<Endpoint>, d: &Option<Endpoint>| matches!((ep, d), (Some(a), Some(b)) if a.id == b.id);
    let headset_is_default = out.is_some()
        && inp.is_some()
        && is(&out, &out_def)
        && is(&out, &out_comm)
        && is(&inp, &in_def)
        && is(&inp, &in_comm);
    Status {
        apo_installed: inp.as_ref().is_some_and(|ep| {
            let s = sysaudio::apo::status(ep);
            s.registered && s.on_endpoint
        }),
        output: out.map(|e| e.name),
        input: inp.map(|e| e.name),
        default_output: out_def.map(|e| e.name),
        default_input: in_def.map(|e| e.name),
        headset_is_default,
        error,
    }
}
