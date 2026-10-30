//! The Mic page's test: record a short sample and play it back, or hear the mic live.
//!
//! The mic is opened natively rather than through the webview, which would
//! pop up a permission prompt and choose its own stream settings. This stream
//! reads the mic as an app does, after noise suppression, and the headphones
//! are the headset's own, so listening live can't feed back through speakers.
//!
//! One thread per action and none when idle. The sample stays in memory only,
//! until the window closes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sysaudio::{Capture, Endpoint, Flow, Render, RATE};
use tauri::{AppHandle, Emitter, Manager};

use super::state::Shared;

/// The longest sample, `MIC_SECONDS` in headset.js.
const MAX_SECONDS: usize = 15;
const MAX_SAMPLES: usize = MAX_SECONDS * RATE as usize;
/// Bars in a full sample, `MIC_BARS` in headset.js.
const BARS: usize = 180;
const BAR_SAMPLES: usize = MAX_SAMPLES / BARS;
/// How often the streams are serviced. The mic delivers 10 ms at a time.
const POLL: Duration = Duration::from_millis(5);
/// How often the Mic page hears about progress.
const TICK: Duration = Duration::from_millis(50);
const PLAY_BUFFER: Duration = Duration::from_millis(200);
/// The most mic that can wait for the headphones while listening live.
const LIVE_BUFFER: Duration = Duration::from_millis(60);
/// Queued before listening starts, so a late wake-up doesn't click.
const LIVE_CUSHION: usize = RATE as usize * 30 / 1000;
const DEVICE_GONE: u32 = 0x8889_0004; // AUDCLNT_E_DEVICE_INVALIDATED
const ACCESS_DENIED: u32 = 0x8007_0005; // E_ACCESSDENIED

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Idle,
    Recording,
    Playing,
    Listening,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    #[serde(skip)]
    audio: Arc<Vec<f32>>,
    seconds: f32,
    bars: Vec<f32>,
    /// Noise suppression strength when it was recorded, `None` if it was off.
    strength: Option<u8>,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    mode: Mode,
    /// Seconds recorded or played so far.
    position: f32,
    /// Bars of the mic right now, while recording or listening.
    live: Vec<f32>,
    sample: Option<Sample>,
    error: Option<String>,
}

struct Job {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

#[derive(Default)]
pub struct MicTest {
    status: Mutex<Status>,
    job: Mutex<Option<Job>>,
}

/// The loudest sample of every `BAR_SAMPLES`.
#[derive(Default)]
struct Bars {
    done: Vec<f32>,
    peak: f32,
    count: usize,
}

impl Bars {
    fn push(&mut self, samples: &[f32]) {
        for s in samples {
            self.peak = self.peak.max(s.abs());
            self.count += 1;
            if self.count == BAR_SAMPLES {
                self.done.push(self.peak);
                self.peak = 0.0;
                self.count = 0;
            }
        }
    }

    /// Forget all but the last `BARS`, for a mic that keeps on going.
    fn keep_last(&mut self) {
        let extra = self.done.len().saturating_sub(BARS);
        self.done.drain(..extra);
    }
}

fn seconds(samples: usize) -> f32 {
    samples as f32 / RATE as f32
}

/// True once every `TICK`.
fn due(last: &mut Instant) -> bool {
    let due = last.elapsed() >= TICK;
    if due {
        *last = Instant::now();
    }
    due
}

fn update(app: &AppHandle, change: impl FnOnce(&mut Status)) {
    let shared = app.state::<Shared>();
    let mut status = shared.mic_test.status.lock().unwrap();
    change(&mut status);
    let _ = app.emit("mic-test", &*status);
}

fn endpoint(flow: Flow) -> sysaudio::Result<Endpoint> {
    sysaudio::find_arctis(flow)?.ok_or_else(|| sysaudio::Error::Msg("The Arctis Nova 5 isn't connected.".into()))
}

/// The strength noise suppression is working at, `None` if it isn't.
fn suppression(app: &AppHandle) -> Option<u8> {
    let shared = app.state::<Shared>();
    let installed = shared.audio.lock().unwrap().apo_installed;
    let mic = shared.core.lock().unwrap().mic.clone();
    (installed && mic.denoise).then_some(mic.strength)
}

fn record(app: &AppHandle, stop: &AtomicBool) -> sysaudio::Result<()> {
    let mic = Capture::open(&endpoint(Flow::Input)?.id)?;
    let strength = suppression(app);
    let mut audio = Vec::with_capacity(MAX_SAMPLES);
    let mut bars = Bars::default();
    let mut tick = Instant::now();
    let mut result = Ok(());
    while !stop.load(Ordering::Relaxed) && audio.len() < MAX_SAMPLES {
        std::thread::sleep(POLL);
        let from = audio.len();
        result = mic.read(&mut audio);
        if result.is_err() {
            break;
        }
        audio.truncate(MAX_SAMPLES);
        bars.push(&audio[from..]);
        if due(&mut tick) {
            update(app, |s| {
                s.position = seconds(audio.len());
                s.live.clone_from(&bars.done);
            });
        }
    }
    // Even if the headset went away, what came before that is worth keeping.
    if !audio.is_empty() {
        let sample = Sample { seconds: seconds(audio.len()), audio: Arc::new(audio), bars: bars.done, strength };
        update(app, |s| s.sample = Some(sample));
    }
    result
}

fn play(app: &AppHandle, stop: &AtomicBool) -> sysaudio::Result<()> {
    let sample = app.state::<Shared>().mic_test.status.lock().unwrap().sample.clone();
    let Some(sample) = sample else { return Ok(()) };
    let audio = &sample.audio[..];
    let out = Render::open(&endpoint(Flow::Output)?.id, PLAY_BUFFER)?;
    let mut sent = out.write(audio)?;
    out.start()?;
    let mut tick = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        sent += out.write(&audio[sent..])?;
        let queued = out.queued()?;
        if sent == audio.len() && queued == 0 {
            break;
        }
        if due(&mut tick) {
            update(app, |s| s.position = seconds(sent - queued));
        }
        std::thread::sleep(POLL);
    }
    Ok(())
}

fn listen(app: &AppHandle, stop: &AtomicBool) -> sysaudio::Result<()> {
    let mic = Capture::open(&endpoint(Flow::Input)?.id)?;
    let out = Render::open(&endpoint(Flow::Output)?.id, LIVE_BUFFER)?;
    let mut incoming = Vec::new();
    let mut bars = Bars::default();
    let mut playing = false;
    let mut tick = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        incoming.clear();
        mic.read(&mut incoming)?;
        bars.push(&incoming);
        bars.keep_last();
        // Whatever doesn't fit is dropped, so a stall can't leave the mic running late.
        out.write(&incoming)?;
        if !playing && out.queued()? >= LIVE_CUSHION {
            out.start()?;
            playing = true;
        }
        if due(&mut tick) {
            update(app, |s| s.live.clone_from(&bars.done));
        }
    }
    Ok(())
}

fn message(e: sysaudio::Error) -> String {
    match e {
        sysaudio::Error::Win(e) if e.code().0 as u32 == ACCESS_DENIED => {
            "Windows is blocking kit from the mic. Turn on \"Let desktop apps access your microphone\" in Settings, Privacy & security, Microphone.".into()
        }
        sysaudio::Error::Win(e) if e.code().0 as u32 == DEVICE_GONE => "The headset went away.".into(),
        e => e.to_string(),
    }
}

/// Stop whatever is running, then start `mode`.
pub fn set(app: &AppHandle, mode: Mode) {
    let shared = app.state::<Shared>();
    let mut job = shared.mic_test.job.lock().unwrap();
    if let Some(running) = job.take() {
        running.stop.store(true, Ordering::Relaxed);
        let _ = running.thread.join();
    }
    let run: fn(&AppHandle, &AtomicBool) -> sysaudio::Result<()> = match mode {
        Mode::Idle => return,
        Mode::Recording => record,
        Mode::Playing => play,
        Mode::Listening => listen,
    };
    update(app, |s| {
        s.mode = mode;
        s.error = None;
    });
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, app) = (stop.clone(), app.clone());
    let thread = std::thread::Builder::new()
        .name("mic-test".into())
        .spawn(move || {
            sysaudio::com_init();
            let result = run(&app, &flag);
            update(&app, |s| {
                s.mode = Mode::Idle;
                s.position = 0.0;
                s.live.clear();
                s.error = result.err().map(message);
            });
        })
        .expect("spawn mic test thread");
    *job = Some(Job { stop, thread });
}

/// Nothing left to show it on: stop, and free the sample so the tray stays tiny.
pub fn clear(app: &AppHandle) {
    set(app, Mode::Idle);
    *app.state::<Shared>().mic_test.status.lock().unwrap() = Status::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_is_the_loudest_sample_in_its_stretch() {
        let mut bars = Bars::default();
        let mut audio = vec![0.1; BAR_SAMPLES * 2 + 5];
        audio[3] = -0.5;
        audio[BAR_SAMPLES + 1] = 0.25;
        // In pieces, the way the streams deliver it.
        for piece in audio.chunks(480) {
            bars.push(piece);
        }
        assert_eq!(bars.done, [0.5, 0.25]);
    }

    #[test]
    fn a_full_sample_fills_the_strip_exactly() {
        let mut bars = Bars::default();
        bars.push(&vec![0.1; MAX_SAMPLES]);
        assert_eq!(bars.done.len(), BARS);
    }

    /// headset.js reads these names, and the audio isn't one of them.
    #[test]
    fn the_page_gets_the_names_it_reads() {
        let sample = Sample { audio: Arc::new(vec![0.5; 4]), seconds: 1.5, bars: vec![0.5], strength: Some(90) };
        let status = Status { mode: Mode::Playing, sample: Some(sample), ..Status::default() };
        let expected = serde_json::json!({
            "mode": "playing",
            "position": 0.0,
            "live": [],
            "sample": { "seconds": 1.5, "bars": [0.5], "strength": 90 },
            "error": null,
        });
        assert_eq!(serde_json::to_value(status).unwrap(), expected);
        assert_eq!(serde_json::from_str::<Mode>("\"listening\"").unwrap(), Mode::Listening);
    }

    #[test]
    fn listening_keeps_only_the_latest_bars() {
        let mut bars = Bars { done: (0..BARS + 7).map(|i| i as f32).collect(), ..Bars::default() };
        bars.keep_last();
        assert_eq!(bars.done.len(), BARS);
        assert_eq!(bars.done[0], 7.0);
        assert_eq!(*bars.done.last().unwrap(), (BARS + 6) as f32);
    }
}
