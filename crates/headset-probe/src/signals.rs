//! The test program `capture` plays: one stereo stream at 48 kHz made of
//! segments separated by silence, so the analysis can find each one again in
//! the recording.

use std::f64::consts::PI;

use realfft::RealFftPlanner;

pub const RATE: usize = 48000;
/// Silence after every segment, long enough for filter tails and any
/// limiter release to settle before the next one starts.
const GAP: usize = RATE / 2;

pub const SWEEP_F1: f64 = 15.0;
pub const SWEEP_F2: f64 = 22000.0;
const SWEEP_SECONDS: f64 = 6.0;
const FADE: usize = RATE / 100;
/// Low enough that +8 dB of EQ stays well clear of full scale.
pub const SWEEP_LOW_DB: f64 = -20.0;
pub const PINK_RMS_DB: f64 = -26.0;
pub const STEP_LEVELS_DB: [f64; 9] = [-24.0, -21.0, -18.0, -15.0, -12.0, -9.0, -6.0, -3.0, 0.0];
pub const STEP_FREQS: [f64; 3] = [63.0, 1000.0, 10000.0];
const STEP_LEN: usize = RATE * 2 / 5;
pub const JUMP_FREQ: f64 = 63.0;
pub const JUMP_LOW_DB: f64 = -24.0;
pub const JUMP_HIGH_DB: f64 = 0.0;

#[derive(Debug, Clone)]
pub struct Segment {
    pub name: String,
    /// First frame in the program.
    pub start: usize,
    pub len: usize,
}

pub struct Program {
    pub frames: Vec<[f32; 2]>,
    pub segments: Vec<Segment>,
}

pub fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

impl Program {
    pub fn standard() -> Self {
        let mut p = Program { frames: Vec::new(), segments: Vec::new() };
        p.push("silence", vec![[0.0; 2]; RATE]);
        let sweep = sweep(db_to_amp(SWEEP_LOW_DB));
        p.push("sweep L", sweep.iter().map(|&s| [s, 0.0]).collect());
        p.push("sweep R", sweep.iter().map(|&s| [0.0, s]).collect());
        p.push("pink", pink_noise(8 * RATE, db_to_amp(PINK_RMS_DB)).into_iter().map(|s| [s, s]).collect());
        let full = sweep_scaled(&sweep, db_to_amp(-SWEEP_LOW_DB));
        p.push("sweep 0 dBFS", full.iter().map(|&s| [s, s]).collect());
        for f in STEP_FREQS {
            let s = steps(f);
            p.push(&format!("steps {f} Hz"), s.into_iter().map(|s| [s, s]).collect());
        }
        p.push("jump", jump().into_iter().map(|s| [s, s]).collect());
        p
    }

    fn push(&mut self, name: &str, frames: Vec<[f32; 2]>) {
        self.segments.push(Segment { name: name.into(), start: self.frames.len(), len: frames.len() });
        self.frames.extend(frames);
        self.frames.extend(std::iter::repeat_n([0.0; 2], GAP));
    }

    pub fn segment(&self, name: &str) -> &Segment {
        self.segments.iter().find(|s| s.name == name).expect("segment")
    }

    /// One channel of a segment, as played.
    pub fn channel(&self, seg: &Segment, ch: usize) -> Vec<f64> {
        self.frames[seg.start..seg.start + seg.len].iter().map(|f| f[ch] as f64).collect()
    }
}

/// Instantaneous frequency of the sweep `t` seconds in.
pub fn sweep_freq(t: f64) -> f64 {
    SWEEP_F1 * ((SWEEP_F2 / SWEEP_F1).ln() * t / SWEEP_SECONDS).exp()
}

fn sweep(amp: f64) -> Vec<f32> {
    let n = (SWEEP_SECONDS * RATE as f64) as usize;
    let k = (SWEEP_F2 / SWEEP_F1).ln();
    (0..n)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let phase = 2.0 * PI * SWEEP_F1 * SWEEP_SECONDS / k * ((k * t / SWEEP_SECONDS).exp() - 1.0);
            (amp * phase.sin() * fade(i, n)) as f32
        })
        .collect()
}

fn sweep_scaled(s: &[f32], by: f64) -> Vec<f32> {
    s.iter().map(|&x| (x as f64 * by) as f32).collect()
}

/// Raised-cosine fade in and out, so no segment starts or ends with a click.
fn fade(i: usize, n: usize) -> f64 {
    let edge = i.min(n - 1 - i);
    if edge >= FADE {
        1.0
    } else {
        0.5 - 0.5 * (PI * edge as f64 / FADE as f64).cos()
    }
}

/// Deterministic white noise source (xorshift), uniform in [-1, 1).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// Pink noise with an exact 1/f spectrum from 15 Hz to 22 kHz, at `rms`.
pub fn pink_noise(n: usize, rms: f64) -> Vec<f32> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut planner = RealFftPlanner::<f64>::new();
    let inv = planner.plan_fft_inverse(n);
    let mut spec = inv.make_input_vec();
    let df = RATE as f64 / n as f64;
    for (k, c) in spec.iter_mut().enumerate() {
        let f = k as f64 * df;
        if (SWEEP_F1..=SWEEP_F2).contains(&f) {
            let phase = PI * rng.next();
            *c = realfft::num_complex::Complex::from_polar(1.0 / f.sqrt(), phase);
        }
    }
    let mut out = inv.make_output_vec();
    inv.process(&mut spec, &mut out).expect("ifft");
    let now = (out.iter().map(|x| x * x).sum::<f64>() / n as f64).sqrt();
    out.iter().enumerate().map(|(i, x)| (x * rms / now * fade(i, n)) as f32).collect()
}

fn tone(freq: f64, amps: &[(usize, f64)]) -> Vec<f32> {
    let n: usize = amps.iter().map(|a| a.0).sum();
    let mut out = Vec::with_capacity(n);
    for &(len, amp) in amps {
        for _ in 0..len {
            let i = out.len();
            let t = i as f64 / RATE as f64;
            out.push((amp * (2.0 * PI * freq * t).sin()) as f32);
        }
    }
    let n = out.len();
    for (i, s) in out.iter_mut().enumerate() {
        *s = (*s as f64 * fade(i, n)) as f32;
    }
    out
}

fn steps(freq: f64) -> Vec<f32> {
    let amps: Vec<_> = STEP_LEVELS_DB.iter().map(|&db| (STEP_LEN, db_to_amp(db))).collect();
    tone(freq, &amps)
}

/// Where step `i` of a "steps" segment starts and how long it is.
pub fn step_span(i: usize) -> (usize, usize) {
    (i * STEP_LEN, STEP_LEN)
}

/// Lengths of the jump's low, high and low parts.
pub const JUMP_PARTS: [usize; 3] = [RATE, RATE, 2 * RATE];

fn jump() -> Vec<f32> {
    let (lo, hi) = (db_to_amp(JUMP_LOW_DB), db_to_amp(JUMP_HIGH_DB));
    tone(JUMP_FREQ, &[(JUMP_PARTS[0], lo), (JUMP_PARTS[1], hi), (JUMP_PARTS[2], lo)])
}
