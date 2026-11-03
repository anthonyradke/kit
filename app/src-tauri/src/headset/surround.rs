//! Virtual 7.1 surround.
//!
//! Games play to VB-Audio's virtual cable ("CABLE Input"), which kit sets up
//! as 7.1 speakers and makes the Windows default for everything but calls.
//! This thread records what the cable plays (loopback, all 8 channels),
//! places each channel as a virtual speaker around the listener
//! (`spatial::Renderer`) and plays the result on the headset. The headset's
//! onboard EQ still applies on top. The cable and the headset run on separate
//! clocks; `spatial::Pacer` keeps the headset's buffer steady between them.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde::Serialize;
use spatial::{Pacer, Renderer, Speaker, SpeakerTest};
use sysaudio::{Flow, Loopback, Render};
use tauri::{AppHandle, Emitter, Manager};

use super::state::Shared;

pub enum Cmd {
    /// Settings changed; re-read them.
    Refresh,
    /// Play noise through each virtual speaker in turn.
    Test,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// VB-CABLE is installed.
    pub cable: bool,
    /// Sound is flowing from the cable to the headset.
    pub running: bool,
    /// What apps are sending right now: "7.1", "5.1", "Stereo", or none while nothing plays.
    pub playing: Option<String>,
    /// From an app playing to the cable to the sound reaching the headset, roughly.
    pub latency_ms: Option<u32>,
    /// The speaker the test is playing through.
    pub testing: Option<String>,
    pub error: Option<String>,
}

/// Frames kept queued in the headset: enough to ride out a late wake-up.
const TARGET: usize = 960;
const RENDER_BUFFER: Duration = Duration::from_millis(100);
const POLL: Duration = Duration::from_millis(2);
const IDLE_POLL: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(2);
/// No audio from the cable for this long means nothing is playing; keep the
/// headset stream fed with silence (which also lets the room ring out).
const SILENT_AFTER: Duration = Duration::from_millis(40);
/// Above this many frames queued, drop input to get the latency back.
const TOO_MUCH: usize = 3 * TARGET;
/// Loopback capture adds about one engine period.
const ENGINE_PERIOD_MS: u32 = 10;

pub fn spawn(app: AppHandle, rx: Receiver<Cmd>) {
    std::thread::Builder::new()
        .name("surround".into())
        .spawn(move || run(app, rx))
        .expect("spawn surround thread");
}

/// Run with the "Pro Audio" scheduling class, so a busy game can't starve it.
fn boost_thread() {
    use windows::core::w;
    use windows::Win32::System::Threading::AvSetMmThreadCharacteristicsW;
    let mut index = 0u32;
    let _ = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut index) };
}

fn run(app: AppHandle, rx: Receiver<Cmd>) {
    sysaudio::com_init();
    boost_thread();
    let mut pipe: Option<Pipeline> = None;
    let mut status = Status::default();
    let mut next_try = Instant::now();
    let mut next_scan = Instant::now();
    let mut test = false;
    loop {
        loop {
            match rx.try_recv() {
                Ok(Cmd::Test) => test = true,
                Ok(Cmd::Refresh) => next_try = Instant::now(),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let (on, room) = {
            let shared = app.state::<Shared>();
            let core = shared.core.lock().unwrap();
            (core.headset.surround, core.headset.room)
        };
        let mut next = status.clone();
        if Instant::now() >= next_scan {
            next.cable = sysaudio::find_cable().ok().flatten().is_some();
            next_scan = Instant::now() + RETRY;
        }
        if !on {
            pipe = None;
            test = false;
            next = Status { cable: next.cable, ..Status::default() };
        } else {
            if pipe.is_none() && Instant::now() >= next_try {
                match Pipeline::open(room) {
                    Ok(p) => {
                        next.error = None;
                        pipe = Some(p);
                    }
                    Err(e) => {
                        next.error = Some(e);
                        next_try = Instant::now() + RETRY;
                    }
                }
            }
            if let Some(p) = &mut pipe {
                if std::mem::take(&mut test) {
                    p.test = Some(SpeakerTest::new(&p.layout, p.renderer.rate()));
                }
                p.renderer.set_room(spatial::room_level(room));
                if let Err(e) = p.pump() {
                    eprintln!("surround stopped: {e}");
                    pipe = None;
                    next.error = Some(friendly(&e));
                    next_try = Instant::now() + RETRY;
                }
            }
            next.running = pipe.is_some();
            next.playing = pipe.as_ref().and_then(|p| p.playing());
            next.latency_ms = pipe.as_ref().map(|p| p.latency_ms());
            next.testing = pipe.as_ref().and_then(|p| p.testing());
        }
        if next != status {
            status = next;
            *app.state::<Shared>().surround.lock().unwrap() = status.clone();
            let _ = app.emit("surround", &status);
        }
        std::thread::sleep(if pipe.is_some() { POLL } else { IDLE_POLL });
    }
}

fn friendly(e: &sysaudio::Error) -> String {
    // AUDCLNT_E_DEVICE_INVALIDATED: the headset turned off or the cable's format changed.
    if matches!(e, sysaudio::Error::Win(w) if w.code().0 as u32 == 0x8889_0004) {
        return "The headset or the cable went away; reconnecting.".into();
    }
    format!("Surround stopped: {e}")
}

struct Pipeline {
    loopback: Loopback,
    render: Render,
    renderer: Renderer,
    pacer: Pacer,
    layout: Vec<Option<Speaker>>,
    test: Option<SpeakerTest>,
    input: Vec<f32>,
    rendered: Vec<f32>,
    out: Vec<f32>,
    last_input: Instant,
    /// Loudest sample per channel since `meter_start`, and the last full second's.
    peaks: Vec<f32>,
    heard: Vec<f32>,
    meter_start: Instant,
}

impl Pipeline {
    fn open(room: u8) -> Result<Self, String> {
        let cable = sysaudio::find_cable().map_err(|e| e.to_string())?.ok_or("VB-CABLE isn't installed.")?;
        let headset = sysaudio::find_arctis(Flow::Output).map_err(|e| e.to_string())?.ok_or("Waiting for the headset.")?;
        let format = sysaudio::ensure_7_1(&cable.id).map_err(|e| format!("Couldn't set CABLE Input to 7.1: {e}"))?;
        if format.rate != sysaudio::RATE {
            return Err(format!("CABLE Input runs at {} Hz; set it to 48000 Hz in Sound settings.", format.rate));
        }
        let loopback = Loopback::open(&cable.id).map_err(|e| format!("Couldn't record CABLE Input: {e}"))?;
        let format = loopback.format();
        let layout = spatial::speakers(format.channels, format.mask);
        let renderer = Renderer::new(&layout, spatial::room_level(room));
        let render = Render::open_channels(&headset.id, 2, RENDER_BUFFER).map_err(|e| format!("Couldn't play on the headset: {e}"))?;
        // Start with the target queued so the first wake-up can't run dry.
        render.write(&vec![0.0; TARGET * 2]).map_err(|e| e.to_string())?;
        render.start().map_err(|e| e.to_string())?;
        Ok(Pipeline {
            loopback,
            render,
            renderer,
            pacer: Pacer::new(sysaudio::RATE, TARGET),
            test: None,
            input: Vec::new(),
            rendered: Vec::new(),
            out: Vec::new(),
            last_input: Instant::now(),
            peaks: vec![0.0; layout.len()],
            heard: vec![0.0; layout.len()],
            meter_start: Instant::now(),
            layout,
        })
    }

    fn pump(&mut self) -> sysaudio::Result<()> {
        let ch = self.renderer.channels();
        self.input.clear();
        self.loopback.read(&mut self.input)?;
        let queued = self.render.queued()?;
        if !self.input.is_empty() {
            self.last_input = Instant::now();
            self.meter();
        }
        let behind = queued + self.pacer.pending();
        if self.input.is_empty() && self.last_input.elapsed() >= SILENT_AFTER && behind < TARGET {
            // Nothing is playing: keep the headset fed at the target.
            self.input.resize((TARGET - behind) * ch, 0.0);
        }
        if let Some(t) = &mut self.test {
            let frames = self.input.len() / ch;
            self.input.clear();
            if !t.fill(frames, &mut self.input) && t.current().is_none() {
                self.test = None;
            }
        }
        if behind > TOO_MUCH {
            // Woke up far too late or the headset stalled: catch up rather than lag.
            self.input.clear();
            self.pacer.reset();
        }
        self.rendered.clear();
        self.renderer.process(&self.input, &mut self.rendered);
        self.out.clear();
        self.pacer.process(&self.rendered, queued, &mut self.out);
        let mut written = 0;
        while written < self.out.len() {
            let n = self.render.write(&self.out[written..])?;
            if n == 0 {
                break;
            }
            written += n;
        }
        Ok(())
    }

    fn meter(&mut self) {
        for frame in self.input.chunks_exact(self.peaks.len()) {
            for (p, &x) in self.peaks.iter_mut().zip(frame) {
                *p = p.max(x.abs());
            }
        }
        if self.meter_start.elapsed() >= Duration::from_secs(1) {
            self.heard.copy_from_slice(&self.peaks);
            self.peaks.fill(0.0);
            self.meter_start = Instant::now();
        }
    }

    /// What apps sent over the last second, judged by which channels had sound.
    fn playing(&self) -> Option<String> {
        if self.last_input.elapsed() > Duration::from_secs(2) {
            return None;
        }
        let active = |s: Speaker| self.layout.iter().zip(&self.heard).any(|(l, &p)| *l == Some(s) && p > 1e-4);
        let sides = active(Speaker::SideLeft) || active(Speaker::SideRight);
        let rears = active(Speaker::BackLeft) || active(Speaker::BackRight);
        let any = self.heard.iter().any(|&p| p > 1e-4);
        Some(
            match (sides && rears, sides || rears, any) {
                (true, _, _) => "7.1",
                (_, true, _) => "5.1",
                (_, _, true) => "Stereo",
                _ => return None,
            }
            .into(),
        )
    }

    fn latency_ms(&self) -> u32 {
        let frames = self.renderer.latency() + TARGET;
        (frames as u64 * 1000 / sysaudio::RATE as u64) as u32 + ENGINE_PERIOD_MS
    }

    fn testing(&self) -> Option<String> {
        self.test.as_ref().and_then(|t| t.current()).map(|s| s.name().to_string())
    }
}
