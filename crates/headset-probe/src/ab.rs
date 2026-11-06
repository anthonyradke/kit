//! `headset-probe ab`: a level-matched blind ABX test between Sonar and kit.
//!
//! A is Sonar: the music plays into Sonar's Game device and Sonar's EQ.
//! B is kit: the music runs through `kit_eq` (the fitted curve, a preamp and
//! the limiter, as the render APO will) and plays straight to the Nova 5.
//! Both streams run all the time from the same position; the one not being
//! heard gets silence, so switching is instant and seamless.
//!
//! Before listening, with the headset muted, a pink noise burst goes
//! through each path and loopback records the Nova 5 endpoint. That gives
//! the paths' delay difference (B is delayed to line up with A) and their
//! level difference (the louder one is turned down, never the quieter one
//! up). A second burst pair checks the result.

use std::io::BufRead;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use kit_eq::{Curve, Processor};
use sysaudio::{Loopback, Render};

use crate::analyze::{self, db, rms};
use crate::capture::{endpoint_named, nova, Muted};
use crate::decode;
use crate::signals::{self, db_to_amp, RATE};

const BUFFER: Duration = Duration::from_millis(100);
const FADE: usize = RATE / 100;
const BURST: usize = 3 * RATE;
/// Room for loopback to catch up and both paths to drain between bursts.
const BURST_GAP: usize = RATE;
/// Calibration repeats until the paths are this close, or gives up.
const CALIBRATION_PASSES: usize = 4;
const MAX_LAG: isize = 2;
const MAX_LEVEL_DB: f64 = 0.1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Path_ {
    A,
    B,
}

/// What each path gets this frame.
enum Feed<'a> {
    /// The music, to the path being heard.
    Music(&'a [[f32; 2]]),
    /// Calibration: this burst to A at frame `a`, to B at frame `b`.
    Bursts { noise: &'a [f32], a: usize, b: usize },
}

struct Player {
    a: Render,
    b: Render,
    kit: Processor,
    /// Gain into Sonar and into kit, for level matching.
    gain_a: f32,
    gain_b: f32,
    /// Frames each path is held back by, so both reach the headset together.
    delay_a: Vec<[f32; 2]>,
    delay_b: Vec<[f32; 2]>,
    delay_pos: usize,
    /// 0 = A heard, 1 = B heard; moves toward `target` over `FADE` frames.
    mix: f32,
    target: Path_,
    /// Frames written so far.
    t: usize,
    /// Position in the music.
    pos: usize,
}

impl Player {
    fn open(sonar: &str, headset: &str, curve: &Curve) -> Result<Self> {
        let a = Render::open_channels(sonar, 2, BUFFER).context("open Sonar Game")?;
        let b = Render::open_channels(headset, 2, BUFFER).context("open the Nova 5")?;
        Ok(Player {
            a,
            b,
            kit: Processor::new(&curve.design(RATE as f64), RATE as u32, 2, true),
            gain_a: 1.0,
            gain_b: 1.0,
            delay_a: vec![[0.0; 2]; 1],
            delay_b: vec![[0.0; 2]; 1],
            delay_pos: 0,
            mix: 0.0,
            target: Path_::A,
            t: 0,
            pos: 0,
        })
    }

    fn set_delays(&mut self, a: usize, b: usize) {
        self.delay_a = vec![[0.0; 2]; a + 1];
        self.delay_b = vec![[0.0; 2]; b + 1];
        self.delay_pos = 0;
    }

    fn start(&self) -> Result<()> {
        self.a.start()?;
        self.b.start()?;
        Ok(())
    }

    /// Write as many frames as both streams have room for.
    fn pump(&mut self, feed: &Feed) -> Result<()> {
        let room = (self.a.capacity() - self.a.queued()?).min(self.b.capacity() - self.b.queued()?);
        if room == 0 {
            return Ok(());
        }
        let mut out_a = Vec::with_capacity(2 * room);
        let mut out_b = Vec::with_capacity(2 * room);
        let mut kit_in = Vec::with_capacity(2 * room);
        let mut mixes = Vec::with_capacity(room);
        for _ in 0..room {
            let (to_a, to_b) = match feed {
                Feed::Music(m) => {
                    let f = m[self.pos % m.len()];
                    self.pos = (self.pos + 1) % m.len();
                    (f, f)
                }
                Feed::Bursts { noise, a, b } => {
                    let pick = |start: usize| {
                        self.t.checked_sub(start).and_then(|i| noise.get(i)).map_or([0.0; 2], |&s| [s, s])
                    };
                    (pick(*a), pick(*b))
                }
            };
            let step = 1.0 / FADE as f32;
            self.mix = match self.target {
                Path_::A => (self.mix - step).max(0.0),
                Path_::B => (self.mix + step).min(1.0),
            };
            // During calibration both paths play their own bursts.
            let m = if matches!(feed, Feed::Music(_)) { self.mix } else { f32::NAN };
            mixes.push(m);
            out_a.extend(to_a.map(|s| s * self.gain_a));
            kit_in.extend(to_b.map(|s| s * self.gain_b));
            self.t += 1;
        }
        // kit's processing runs on every frame, heard or not, so its
        // filters and limiter are always in the state the music left them in.
        self.kit.process(&mut kit_in);
        let (da, db_) = (self.delay_a.len(), self.delay_b.len());
        for (i, m) in mixes.into_iter().enumerate() {
            let a = [out_a[2 * i], out_a[2 * i + 1]];
            let b = [kit_in[2 * i], kit_in[2 * i + 1]];
            let ia = self.delay_pos % da;
            let ib = self.delay_pos % db_;
            let (a, b) = (std::mem::replace(&mut self.delay_a[ia], a), std::mem::replace(&mut self.delay_b[ib], b));
            self.delay_pos += 1;
            let (wa, wb) = if m.is_nan() { (1.0, 1.0) } else { (1.0 - m, m) };
            out_b.extend(b.map(|s| s * wb));
            out_a[2 * i] = a[0] * wa;
            out_a[2 * i + 1] = a[1] * wa;
        }
        // Both have room for `room` frames, so these write everything.
        self.a.write(&out_a)?;
        self.b.write(&out_b)?;
        Ok(())
    }
}

/// The result of one calibration pass.
struct Match {
    /// How many frames later B's burst reached the headset than A's, beyond
    /// the gap they were played with.
    lag: isize,
    /// B's level minus A's, dB.
    level_db: f64,
    /// Largest difference between the paths' responses, 20 Hz to 20 kHz (B - A).
    worst_db: f64,
    worst_at: f64,
}

fn calibrate(p: &mut Player, lb: &Loopback, noise: &[f32]) -> Result<Match> {
    let mut rec = Vec::new();
    lb.read(&mut rec)?;
    rec.clear();
    // Bursts are placed relative to the first frame written now.
    let a_at = p.t + BURST_GAP / 2;
    let b_at = a_at + BURST + BURST_GAP;
    let end = b_at + BURST + BURST_GAP;
    while p.t < end {
        p.pump(&Feed::Bursts { noise, a: a_at, b: b_at })?;
        lb.read(&mut rec)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    let until = Instant::now() + Duration::from_millis(400);
    while Instant::now() < until {
        lb.read(&mut rec)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    let ch = lb.format().channels;
    let mono: Vec<f64> = rec.chunks_exact(ch).map(|f| (f[0] + f[1]) as f64 / 2.0).collect();
    // Find A's burst in the recording, then B's the planned gap later. Both
    // paths play the same noise, so A's is looked for only before B's could
    // start; searching everywhere can land on B's when it's the louder one.
    let played: Vec<f64> = noise.iter().map(|&s| s as f64).collect();
    let gap = (b_at - a_at) as isize;
    let window = RATE as isize / 5;
    let ia = analyze::best_lag(&played, &mono, 0, (gap - window) as usize);
    let from = (ia + gap - window).max(0) as usize;
    let span = |i: isize| -> Result<&[f64]> {
        let i = usize::try_from(i).ok().filter(|&i| i + BURST <= mono.len());
        i.map(|i| &mono[i..i + BURST]).context("a calibration burst is missing from the recording")
    };
    let sa = span(ia)?;
    let rest = &mono[from.min(mono.len())..(from + BURST + 2 * window as usize).min(mono.len())];
    let ib = from as isize + analyze::best_lag(sa, rest, 0, 2 * window as usize);
    let lag = ib - ia - gap;
    let sb = span(ib)?;
    for (s, name) in [(sa, "Sonar (A)"), (sb, "kit (B)")] {
        if rms(s) < db_to_amp(-70.0) {
            bail!("nothing from {name} reached the headset during calibration");
        }
    }
    // Skip the bursts' edges, where a level or delay change would show.
    let core = |s: &[f64]| s[RATE / 4..BURST - RATE / 4].to_vec();
    let (ca, cb) = (core(sa), core(sb));
    let level_db = db(rms(&cb) / rms(&ca));
    let (h, coh) = analyze::noise_response(&ca, &cb);
    let grid = analyze::grid();
    let (mut worst_db, mut worst_at) = (0.0, 0.0);
    for ((d, c), f) in h.iter().zip(&coh).zip(&grid) {
        if *c > 0.9 && (d - level_db).abs() > worst_db {
            (worst_db, worst_at) = ((d - level_db).abs(), *f);
        }
    }
    Ok(Match { lag, level_db, worst_db, worst_at })
}

fn warn_if_headset_eq_on() {
    let Ok(h) = arctis::Headset::open() else { return };
    let Ok(st) = h.settings() else { return };
    if st.eq.iter().flatten().any(|b| b.gain != 0.0) {
        println!("note: the headset's own EQ isn't flat; it colours A and B alike (`headset-probe flat` resets it)");
    }
}

/// One random bit from the clock (no RNG crate needed for a coin toss).
fn coin() -> bool {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let mut x = n as u64 ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    x & 1 == 1
}

/// Chance of `right` or more out of `n` by guessing.
fn p_value(right: usize, n: usize) -> f64 {
    let choose = |n: usize, k: usize| (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64);
    (right..=n).map(|k| choose(n, k)).sum::<f64>() / 2f64.powi(n as i32)
}

pub fn run(music: &Path, curve_path: &Path, preamp: Option<f64>) -> Result<()> {
    let music = decode::decode(music)?;
    if music.len() < RATE {
        bail!("the music is shorter than a second");
    }
    let text = std::fs::read_to_string(curve_path).with_context(|| format!("read {}", curve_path.display()))?;
    let mut curve = Curve::parse(&text).map_err(anyhow::Error::msg)?;
    // Default: enough headroom that full-scale music never reaches the
    // limiter, as kit's Auto preamp does (the boost of the response it matches).
    let boost = curve.max_boost_db(2.0 * RATE as f64);
    curve.preamp_db = preamp.unwrap_or(-boost.ceil().max(0.0));
    let sonar = endpoint_named("SteelSeries Sonar - Gaming")?;
    let headset = nova()?;
    warn_if_headset_eq_on();
    println!("A = Sonar Game -> {}", headset.name);
    println!("B = kit ({} filters, preamp {:+.1} dB, limiter on; curve peaks at {boost:+.2} dB) -> the same", curve.filters.len(), curve.preamp_db);

    // Recording starts before anything plays, as `capture` does.
    let lb = Loopback::open(&headset.id)?;
    let mut p = Player::open(&sonar.id, &headset.id, &curve)?;
    let noise: Vec<f32> = signals::pink_noise(BURST, db_to_amp(signals::PINK_RMS_DB));
    // Calibrate muted; loopback still records.
    let muted = Muted::new(&headset.id)?;
    p.pump(&Feed::Bursts { noise: &noise, a: usize::MAX, b: usize::MAX })?;
    p.start()?;
    // B's delay minus A's, frames; B's gain minus A's, dB.
    let (mut offset, mut rel_db) = (0isize, 0.0f64);
    let mut passes: Vec<Match> = Vec::new();
    for pass in 1..=CALIBRATION_PASSES {
        let m = calibrate(&mut p, &lb, &noise)?;
        let good = m.lag.abs() <= MAX_LAG && m.level_db.abs() <= MAX_LEVEL_DB;
        // B late: hold A back; B early: hold B back. The louder one is turned down.
        offset -= m.lag;
        rel_db -= m.level_db;
        passes.push(m);
        if good || pass == CALIBRATION_PASSES {
            break;
        }
        p.set_delays((-offset).max(0) as usize, offset.max(0) as usize);
        p.gain_a = db_to_amp((-rel_db).min(0.0)) as f32;
        p.gain_b = db_to_amp(rel_db.min(0.0)) as f32;
    }
    drop(muted);
    let first = &passes[0];
    println!(
        "calibration: B reached the headset {:+.1} ms from A and {:+.2} dB from it",
        first.lag as f64 * 1000.0 / RATE as f64,
        first.level_db,
    );
    for (i, m) in passes.iter().enumerate().skip(1) {
        println!(
            "check {i}: {:+} samples apart and {:+.3} dB; responses differ by at most {:.2} dB (at {:.0} Hz)",
            m.lag, m.level_db, m.worst_db, m.worst_at
        );
    }
    let last = passes.last().unwrap();
    if last.lag.abs() > MAX_LAG || last.level_db.abs() > MAX_LEVEL_DB {
        println!("warning: the match is off; results may give themselves away");
    }

    println!("\nABX: X is A or B at random each trial.");
    println!("  a / b / x   listen to A, B or X        A / B   answer: X is A, X is B");
    println!("  r           back to the start          q       quit and show the score\n");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            if tx.send(line.trim().to_string()).is_err() {
                break;
            }
        }
    });
    listen(&mut p, &music, &rx)
}

fn listen(p: &mut Player, music: &[[f32; 2]], rx: &Receiver<String>) -> Result<()> {
    let (mut trials, mut right) = (0usize, 0usize);
    let mut x = if coin() { Path_::B } else { Path_::A };
    let mut log = Vec::new();
    println!("trial 1: playing A");
    loop {
        p.pump(&Feed::Music(music))?;
        match rx.try_recv() {
            Ok(cmd) => match cmd.as_str() {
                "a" => {
                    p.target = Path_::A;
                    println!("  A");
                }
                "b" => {
                    p.target = Path_::B;
                    println!("  B");
                }
                "x" => {
                    p.target = x;
                    println!("  X");
                }
                "r" => p.pos = 0,
                "A" | "B" => {
                    let said = if cmd == "A" { Path_::A } else { Path_::B };
                    trials += 1;
                    if said == x {
                        right += 1;
                    }
                    log.push((said, x));
                    x = if coin() { Path_::B } else { Path_::A };
                    // No feedback until the end, so answers can't steer each other.
                    println!("trial {}: playing what you had; X is new", trials + 1);
                }
                "q" => break,
                "" => {}
                other => println!("  ? {other}"),
            },
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    if trials == 0 {
        return Ok(());
    }
    println!("\n{right} of {trials} right; the chance of that or better by guessing is {:.3}", p_value(right, trials));
    for (i, (said, x)) in log.iter().enumerate() {
        println!("  trial {}: X was {x:?}, answered {said:?}{}", i + 1, if said == x { "" } else { "  (wrong)" });
    }
    println!(
        "{}",
        if p_value(right, trials) < 0.05 {
            "That's better than guessing: you can tell them apart."
        } else {
            "Not distinguishable from guessing."
        }
    );
    Ok(())
}

