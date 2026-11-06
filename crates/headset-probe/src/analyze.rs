//! Turning a loopback take of the test program into numbers: frequency
//! response, gain, and how loud signals are handled.

use std::f64::consts::PI;

use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

use crate::signals::{self, db_to_amp, Program, Segment, RATE};

/// Log-spaced points, 48 per octave, 20 Hz to 20 kHz.
pub fn grid() -> Vec<f64> {
    (0..=478).map(|i| 20.0 * 2f64.powf(i as f64 / 48.0)).collect()
}

pub fn db(x: f64) -> f64 {
    20.0 * x.max(1e-12).log10()
}

fn rfft(x: &[f64], n: usize) -> Vec<Complex<f64>> {
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut buf = vec![0.0; n];
    let m = x.len().min(n);
    buf[..m].copy_from_slice(&x[..m]);
    let mut out = fft.make_output_vec();
    fft.process(&mut buf, &mut out).expect("fft");
    out
}

fn irfft(mut spec: Vec<Complex<f64>>, n: usize) -> Vec<f64> {
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_inverse(n);
    // The DC and Nyquist bins of a real signal's spectrum are real.
    spec[0].im = 0.0;
    if let Some(last) = spec.last_mut() {
        last.im = 0.0;
    }
    let mut out = vec![0.0; n];
    fft.process(&mut spec, &mut out).expect("ifft");
    out.iter_mut().for_each(|v| *v /= n as f64);
    out
}

/// The lag (in `-before..=after`) at which `b[t + lag]` best matches `a[t]`.
pub(crate) fn best_lag(a: &[f64], b: &[f64], before: usize, after: usize) -> isize {
    let n = (a.len() + b.len()).next_power_of_two();
    let (fa, fb) = (rfft(a, n), rfft(b, n));
    let cross: Vec<_> = fa.iter().zip(&fb).map(|(x, y)| x.conj() * y).collect();
    let r = irfft(cross, n);
    let mut best = (0isize, f64::MIN);
    for lag in -(before as isize)..=after as isize {
        let v = r[lag.rem_euclid(n as isize) as usize];
        if v > best.1 {
            best = (lag, v);
        }
    }
    best.0
}

fn mono(frames: &[[f32; 2]], from: usize, len: usize) -> Vec<f64> {
    (from..from + len).map(|i| frames.get(i).map_or(0.0, |f| (f[0] + f[1]) as f64)).collect()
}

/// Recorded channel `ch`, `len` frames from `from` (zeros past the end).
fn chan(frames: &[[f32; 2]], from: isize, len: usize, ch: usize) -> Vec<f64> {
    (0..len as isize)
        .map(|i| usize::try_from(from + i).ok().and_then(|j| frames.get(j)).map_or(0.0, |f| f[ch] as f64))
        .collect()
}

/// Where the program starts in the recording, and how much that moves
/// between the three sweeps (early, middle and late in the take).
pub struct Alignment {
    pub delay: isize,
    pub drift: usize,
}

impl Alignment {
    pub fn find(prog: &Program, rec: &[[f32; 2]]) -> Option<Self> {
        if rms(&mono(rec, 0, rec.len())) < 1e-7 {
            return None;
        }
        // Coarse: the first sweep against the start of the recording.
        let first = prog.segment("sweep L");
        let played = mono(&prog.frames, 0, first.start + first.len);
        let search = 3 * RATE;
        let recorded = mono(rec, 0, played.len() + search);
        let coarse = best_lag(&played, &recorded, 0, search);
        // Exact: the peak of each sweep's impulse response. Cross-correlation
        // alone lands late when bass boosts dominate the signal.
        let peak_at = |name: &str, ch: usize| -> isize {
            let seg = prog.segment(name);
            let window = RATE as isize / 20;
            let from = seg.start as isize + coarse - window;
            let ir = raw_ir(&prog.channel(seg, ch), &chan(rec, from, seg.len + RATE / 2, ch));
            let k = (0..2 * window as usize).max_by(|&a, &b| ir[a].abs().total_cmp(&ir[b].abs())).unwrap();
            coarse - window + k as isize
        };
        let delays = [peak_at("sweep L", 0), peak_at("sweep R", 1), peak_at("sweep 0 dBFS", 0)];
        let drift = (delays.iter().max().unwrap() - delays.iter().min().unwrap()) as usize;
        Some(Alignment { delay: delays[0], drift })
    }

    pub fn of(&self, seg: &Segment) -> isize {
        seg.start as isize + self.delay
    }
}

pub fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn peak(x: &[f64]) -> f64 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// Magnitude on [`grid`], smoothed over 1/48 octave, from a fine spectrum
/// `power` (|H|^2) with bins `df` apart.
fn to_grid(power: &[f64], df: f64) -> Vec<f64> {
    grid()
        .iter()
        .map(|&f| {
            let (lo, hi) = (f * 2f64.powf(-1.0 / 96.0) / df, f * 2f64.powf(1.0 / 96.0) / df);
            let (a, b) = (lo.ceil() as usize, hi.floor() as usize);
            let p = if b >= a && b < power.len() {
                power[a..=b].iter().sum::<f64>() / (b - a + 1) as f64
            } else {
                let x = f / df;
                let i = (x.floor() as usize).min(power.len() - 2);
                let t = x - i as f64;
                power[i] * (1.0 - t) + power[i + 1] * t
            };
            10.0 * p.max(1e-24).log10()
        })
        .collect()
}

const IR_PRE: usize = 512;

/// Deconvolve `y` by `x` (circular, long enough that nothing wraps onto the
/// response's start).
fn raw_ir(x: &[f64], y: &[f64]) -> Vec<f64> {
    let n = (x.len().max(y.len()) + RATE / 2).next_power_of_two() * 2;
    let (fx, fy) = (rfft(x, n), rfft(y, n));
    let floor = fx.iter().map(|c| c.norm_sqr()).fold(0.0, f64::max) * 1e-9;
    let h: Vec<_> = fx.iter().zip(&fy).map(|(x, y)| y * x.conj() / (x.norm_sqr() + floor)).collect();
    irfft(h, n)
}

/// The path's linear impulse response from a sweep: deconvolve the
/// recording by what was played and keep only the main response (the sweep
/// puts harmonic distortion well before it). Starts `IR_PRE` frames early.
pub fn linear_ir(x: &[f64], y: &[f64]) -> Vec<f64> {
    let ir = raw_ir(x, y);
    let n = ir.len();
    const POST: usize = 16384;
    const FADE_IN: usize = 256;
    const FADE_OUT: usize = 4096;
    let len = IR_PRE + POST;
    (0..len)
        .map(|k| {
            let v = ir[(k as isize - IR_PRE as isize).rem_euclid(n as isize) as usize];
            let w = if k < FADE_IN {
                0.5 - 0.5 * (PI * k as f64 / FADE_IN as f64).cos()
            } else if k >= len - FADE_OUT {
                0.5 + 0.5 * (PI * (k - (len - FADE_OUT)) as f64 / FADE_OUT as f64).cos()
            } else {
                1.0
            };
            v * w
        })
        .collect()
}

/// The response in dB on [`grid`] of an impulse response from [`linear_ir`].
pub fn ir_response(ir: &[f64]) -> Vec<f64> {
    let m = 32768;
    let spec = rfft(ir, m);
    let power: Vec<f64> = spec.iter().map(|c| c.norm_sqr()).collect();
    to_grid(&power, RATE as f64 / m as f64)
}

/// What a linear path with impulse response `ir` outputs for `x`.
fn predict(x: &[f64], ir: &[f64]) -> Vec<f64> {
    let n = (x.len() + ir.len()).next_power_of_two();
    let (fx, fh) = (rfft(x, n), rfft(ir, n));
    let y = irfft(fx.iter().zip(&fh).map(|(a, b)| a * b).collect(), n);
    y[IR_PRE..IR_PRE + x.len()].to_vec()
}

/// Welch estimate of the transfer function from `x` to `y` (H1) and the
/// coherence, both on [`grid`].
pub fn noise_response(x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>) {
    const SEG: usize = 16384;
    let window: Vec<f64> = (0..SEG).map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / SEG as f64).cos()).collect();
    let bins = SEG / 2 + 1;
    let (mut pxx, mut pyy, mut pyx) = (vec![0.0; bins], vec![0.0; bins], vec![Complex::new(0.0, 0.0); bins]);
    let mut start = 0;
    while start + SEG <= x.len().min(y.len()) {
        let wx: Vec<f64> = x[start..start + SEG].iter().zip(&window).map(|(a, w)| a * w).collect();
        let wy: Vec<f64> = y[start..start + SEG].iter().zip(&window).map(|(a, w)| a * w).collect();
        let (fx, fy) = (rfft(&wx, SEG), rfft(&wy, SEG));
        for k in 0..bins {
            pxx[k] += fx[k].norm_sqr();
            pyy[k] += fy[k].norm_sqr();
            pyx[k] += fy[k] * fx[k].conj();
        }
        start += SEG / 2;
    }
    let df = RATE as f64 / SEG as f64;
    // Per bin first, then smoothed: summing cross-spectra across a band
    // would cancel wherever the path's phase turns within it.
    let h_bins: Vec<f64> = (0..bins).map(|k| pyx[k].norm_sqr() / (pxx[k] * pxx[k]).max(1e-300)).collect();
    let coh_bins: Vec<f64> = (0..bins).map(|k| pyx[k].norm_sqr() / (pxx[k] * pyy[k]).max(1e-300)).collect();
    let h = to_grid(&h_bins, df);
    let coh = to_grid(&coh_bins, df).iter().map(|d| 10f64.powf(d / 10.0)).collect();
    (h, coh)
}

/// Least-squares fit of a sine at `freq` to `y`: (amplitude, THD+N as a
/// ratio of the fundamental's RMS).
pub fn tone_fit(y: &[f64], freq: f64) -> (f64, f64) {
    let w = 2.0 * PI * freq / RATE as f64;
    let (mut ss, mut cc, mut sc, mut ys, mut yc) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &v) in y.iter().enumerate() {
        let (s, c) = (w * i as f64).sin_cos();
        ss += s * s;
        cc += c * c;
        sc += s * c;
        ys += v * s;
        yc += v * c;
    }
    let det = ss * cc - sc * sc;
    let a = (ys * cc - yc * sc) / det;
    let b = (yc * ss - ys * sc) / det;
    let amp = (a * a + b * b).sqrt();
    let resid: Vec<f64> = y
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let (s, c) = (w * i as f64).sin_cos();
            v - a * s - b * c
        })
        .collect();
    (amp, rms(&resid) / (amp / 2f64.sqrt()).max(1e-12))
}

pub struct StepRow {
    pub level_db: f64,
    pub gain_db: f64,
    pub out_peak_db: f64,
    pub thd_db: f64,
}

pub struct Report {
    pub noise_floor_db: f64,
    pub delay_ms: f64,
    pub delay_spread: usize,
    /// dB on [`grid`]: sweep L→L, R→R, pink L, pink R.
    pub sweep: [Vec<f64>; 2],
    pub crosstalk_db: f64,
    pub pink: [Vec<f64>; 2],
    pub coherence: Vec<f64>,
    pub pink_gain_db: f64,
    pub steps: Vec<(f64, Vec<StepRow>)>,
    /// Full-scale sweep: output peak (dBFS), samples at or over full scale,
    /// and (freq, gain change vs the -20 dBFS sweep) along the way.
    pub full_peak_db: f64,
    pub full_clipped: usize,
    pub full_deviation: Vec<(f64, f64)>,
    /// (ms since the jump to 0 dBFS, gain change vs low level) through the
    /// high part and after it.
    pub jump: Vec<(f64, f64)>,
}

pub fn analyze(prog: &Program, rec: &[[f32; 2]]) -> Option<Report> {
    let al = Alignment::find(prog, rec)?;
    let at = |seg: &Segment, extra: usize, ch: usize| chan(rec, al.of(seg), seg.len + extra, ch);

    let silence = prog.segment("silence");
    let noise_floor_db = db(rms(&[at(silence, 0, 0), at(silence, 0, 1)].concat()));

    let tail = RATE / 2;
    let sl = prog.segment("sweep L");
    let sr = prog.segment("sweep R");
    let xl = prog.channel(sl, 0);
    let xr = prog.channel(sr, 1);
    let ir_l = linear_ir(&xl, &at(sl, tail, 0));
    let sweep = [ir_response(&ir_l), ir_response(&linear_ir(&xr, &at(sr, tail, 1)))];
    let leak = ir_response(&linear_ir(&xl, &at(sl, tail, 1)));
    let crosstalk_db = grid()
        .iter()
        .zip(leak.iter().zip(&sweep[0]))
        .filter(|(f, _)| (100.0..10000.0).contains(*f))
        .map(|(_, (l, d))| l - d)
        .fold(f64::MIN, f64::max);

    let pk = prog.segment("pink");
    let xp = prog.channel(pk, 0);
    let (pl, coh) = noise_response(&xp, &at(pk, 0, 0));
    let (pr, _) = noise_response(&xp, &at(pk, 0, 1));
    let pink_gain_db = db(rms(&[at(pk, 0, 0), at(pk, 0, 1)].concat()) / rms(&xp));

    let mut steps = Vec::new();
    for f in signals::STEP_FREQS {
        let seg = prog.segment(&format!("steps {f} Hz"));
        let y = at(seg, 0, 0);
        let rows = signals::STEP_LEVELS_DB
            .iter()
            .enumerate()
            .map(|(i, &level_db)| {
                let (s, n) = signals::step_span(i);
                // Skip the first 100 ms of each step, where a limiter is still moving.
                let part = &y[s + RATE / 10..s + n - RATE / 40];
                let (amp, thd) = tone_fit(part, f);
                StepRow {
                    level_db,
                    gain_db: db(amp / db_to_amp(level_db)),
                    out_peak_db: db(peak(&[part, &at(seg, 0, 1)[s + RATE / 10..s + n - RATE / 40]].concat())),
                    thd_db: db(thd),
                }
            })
            .collect();
        steps.push((f, rows));
    }

    // The 0 dBFS sweep against 10x the -20 dBFS one: equal if nothing reacts to level.
    let full = prog.segment("sweep 0 dBFS");
    let yf = [at(full, tail, 0), at(full, tail, 1)];
    let full_peak_db = db(peak(&[&yf[0][..], &yf[1][..]].concat()));
    let full_clipped = yf.iter().flatten().filter(|v| v.abs() >= 0.99999).count();
    let low = at(sl, tail, 0);
    let scale = db_to_amp(-signals::SWEEP_LOW_DB);
    let (win, hop) = (2048, 1024);
    let mut full_deviation = Vec::new();
    let mut i = 0;
    while i + win <= full.len {
        let f = signals::sweep_freq((i + win / 2) as f64 / RATE as f64);
        if (20.0..=20000.0).contains(&f) {
            let a = rms(&yf[0][i..i + win]);
            let b = rms(&low[i..i + win]) * scale;
            full_deviation.push((f, db(a / b)));
        }
        i += hop;
    }

    // The jump: per-cycle amplitude against what the path's linear response
    // predicts, so EQ ringing at the level changes cancels out.
    let jseg = prog.segment("jump");
    let y = at(jseg, 0, 0);
    let expect = predict(&prog.channel(jseg, 0), &ir_l);
    let cycle = (RATE as f64 / signals::JUMP_FREQ).round() as usize;
    let lo_len = signals::JUMP_PARTS[0];
    let mut jump = Vec::new();
    let mut s = lo_len - 4 * cycle;
    while s + 2 * cycle <= jseg.len - RATE / 10 {
        let a = tone_fit(&y[s..s + 2 * cycle], signals::JUMP_FREQ).0;
        let b = tone_fit(&expect[s..s + 2 * cycle], signals::JUMP_FREQ).0;
        let t = (s + cycle) as f64 - lo_len as f64;
        jump.push((t * 1000.0 / RATE as f64, db(a / b)));
        s += cycle;
    }

    Some(Report {
        noise_floor_db,
        delay_ms: al.delay as f64 * 1000.0 / RATE as f64,
        delay_spread: al.drift,
        sweep,
        crosstalk_db,
        pink: [pl, pr],
        coherence: coh,
        pink_gain_db,
        steps,
        full_peak_db,
        full_clipped,
        full_deviation,
        jump,
    })
}

/// The value of a [`grid`] curve nearest `f`.
pub fn at_freq(curve: &[f64], f: f64) -> f64 {
    let i = ((f / 20.0).log2() * 48.0).round().clamp(0.0, (curve.len() - 1) as f64) as usize;
    curve[i]
}
