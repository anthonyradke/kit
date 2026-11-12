//! kit's headphone EQ in software: a preamp, a cascade of biquads and a
//! lookahead peak limiter, all in floating point.
//!
//! This is the processing kit's EQ APO runs on the headset's endpoint (no
//! allocation once built, so it is safe on the audio engine's thread), and
//! what `headset-probe ab` plays for the blind comparison with Sonar.
//! Curves read and write the same text format as Equalizer APO and AutoEQ
//! (`Preamp: -6 dB`, `Filter 1: ON PK Fc 55 Hz Gain 4 dB Q 0.707`).

mod design;
mod limiter;

use std::f64::consts::PI;
use std::fmt;

pub use design::{Design, MAX_SECTIONS};
pub use limiter::Limiter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Kind::Peaking => "PK",
            Kind::LowShelf => "LSC",
            Kind::HighShelf => "HSC",
            Kind::LowPass => "LPQ",
            Kind::HighPass => "HPQ",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "PK" | "PEQ" => Kind::Peaking,
            "LS" | "LSC" => Kind::LowShelf,
            "HS" | "HSC" => Kind::HighShelf,
            "LP" | "LPQ" => Kind::LowPass,
            "HP" | "HPQ" => Kind::HighPass,
            _ => return None,
        })
    }
}

/// One filter of a curve. Gain is ignored by low and high pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter {
    pub kind: Kind,
    pub freq: f64,
    pub gain_db: f64,
    pub q: f64,
}

impl Filter {
    pub fn new(kind: Kind, freq: f64, gain_db: f64, q: f64) -> Self {
        Filter { kind, freq, gain_db, q }
    }

    /// The filter's coefficients at `rate`, from the RBJ Audio EQ Cookbook
    /// (shelves take Q like the peaking filter does).
    pub fn biquad(&self, rate: f64) -> Biquad {
        let w = 2.0 * PI * self.freq / rate;
        let (cos, alpha) = (w.cos(), w.sin() / (2.0 * self.q));
        let a = 10f64.powf(self.gain_db / 40.0);
        let sa = 2.0 * a.sqrt() * alpha;
        let [b0, b1, b2, a0, a1, a2] = match self.kind {
            Kind::Peaking => [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
            Kind::LowShelf => [
                a * (a + 1.0 - (a - 1.0) * cos + sa),
                2.0 * a * (a - 1.0 - (a + 1.0) * cos),
                a * (a + 1.0 - (a - 1.0) * cos - sa),
                a + 1.0 + (a - 1.0) * cos + sa,
                -2.0 * (a - 1.0 + (a + 1.0) * cos),
                a + 1.0 + (a - 1.0) * cos - sa,
            ],
            Kind::HighShelf => [
                a * (a + 1.0 + (a - 1.0) * cos + sa),
                -2.0 * a * (a - 1.0 + (a + 1.0) * cos),
                a * (a + 1.0 + (a - 1.0) * cos - sa),
                a + 1.0 - (a - 1.0) * cos + sa,
                2.0 * (a - 1.0 - (a + 1.0) * cos),
                a + 1.0 - (a - 1.0) * cos - sa,
            ],
            Kind::LowPass => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
            Kind::HighPass => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
        };
        Biquad { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 }
    }
}

/// Normalized biquad coefficients (a0 = 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Biquad {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Biquad {
    /// |H| in dB at `freq`.
    pub fn response_db(&self, freq: f64, rate: f64) -> f64 {
        let w = 2.0 * PI * freq / rate;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let (nr, ni) = (self.b0 + self.b1 * c1 + self.b2 * c2, -(self.b1 * s1 + self.b2 * s2));
        let (dr, di) = (1.0 + self.a1 * c1 + self.a2 * c2, -(self.a1 * s1 + self.a2 * s2));
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// A whole EQ: gain in front, then each filter in turn.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Curve {
    pub preamp_db: f64,
    pub filters: Vec<Filter>,
}

impl Curve {
    /// The curve's gain in dB at `freq`, preamp included.
    pub fn response_db(&self, freq: f64, rate: f64) -> f64 {
        self.preamp_db + self.filters.iter().map(|f| f.biquad(rate).response_db(freq, rate)).sum::<f64>()
    }

    /// The most the filters (without the preamp) lift any frequency from
    /// 20 Hz to 20 kHz, in dB; 0 if they only cut.
    pub fn max_boost_db(&self, rate: f64) -> f64 {
        let filters = Curve { preamp_db: 0.0, filters: self.filters.clone() };
        (0..=2000)
            .map(|k| 20.0 * 1000f64.powf(k as f64 / 2000.0))
            .map(|f| filters.response_db(f, rate))
            .fold(0.0, f64::max)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut curve = Curve::default();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            let err = |what: &str| format!("line {}: {what}: {line}", n + 1);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((head, rest)) = line.split_once(':') else { return Err(err("expected `Preamp:` or `Filter:`")) };
            let words: Vec<&str> = rest.split_whitespace().collect();
            if head.trim().eq_ignore_ascii_case("preamp") {
                curve.preamp_db = words.first().and_then(|w| w.parse::<f64>().ok()).filter(|p| p.is_finite()).ok_or_else(|| err("bad preamp"))?;
                continue;
            }
            if !head.trim().to_ascii_lowercase().starts_with("filter") {
                return Err(err("unknown line"));
            }
            if words.first() == Some(&"OFF") {
                continue;
            }
            let words = if words.first() == Some(&"ON") { &words[1..] } else { &words[..] };
            let kind = words.first().and_then(|w| Kind::parse(w)).ok_or_else(|| err("unknown filter type"))?;
            let value = |key: &str| -> Option<f64> {
                let i = words.iter().position(|w| w.eq_ignore_ascii_case(key))?;
                words.get(i + 1)?.parse().ok()
            };
            let freq = value("Fc").ok_or_else(|| err("missing Fc"))?;
            let filter = Filter {
                kind,
                freq,
                gain_db: value("Gain").unwrap_or(0.0),
                q: value("Q").unwrap_or(std::f64::consts::FRAC_1_SQRT_2),
            };
            let finite = [filter.freq, filter.gain_db, filter.q].iter().all(|x| x.is_finite());
            if !finite || filter.freq <= 0.0 || filter.q <= 0.0 {
                return Err(err("Fc and Q must be positive numbers"));
            }
            curve.filters.push(filter);
        }
        Ok(curve)
    }
}

impl fmt::Display for Curve {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Preamp: {:.2} dB", self.preamp_db)?;
        for (i, x) in self.filters.iter().enumerate() {
            writeln!(f, "Filter {}: ON {} Fc {:.2} Hz Gain {:.3} dB Q {:.4}", i + 1, x.kind.code(), x.freq, x.gain_db, x.q)?;
        }
        Ok(())
    }
}

/// Below this a filter's state is as good as silence. Zeroing it keeps a
/// decaying filter out of the denormal range, where the CPU runs its math
/// many times slower; with no input, the rounding in a recursive filter can
/// otherwise hold the state there for good.
pub const TINY: f64 = 1e-30;

/// Runs a [`Design`] on interleaved audio, every channel alike, in a
/// transposed direct form II with 64-bit state (low-frequency filters at
/// 48 kHz lose precision in 32-bit). Optionally followed by a [`Limiter`].
/// Nothing allocates after [`Processor::new`], so it can run on the audio
/// engine's thread.
pub struct Processor {
    channels: usize,
    design: Design,
    /// `[s1, s2]` per channel per section, room for [`MAX_SECTIONS`].
    state: Vec<[f64; 2]>,
    limiter: Option<Limiter>,
    frame: Vec<f32>,
}

impl Processor {
    pub fn new(design: &Design, rate: u32, channels: usize, limit: bool) -> Self {
        Processor {
            channels,
            design: *design,
            state: vec![[0.0; 2]; MAX_SECTIONS * channels],
            limiter: limit.then(|| Limiter::new(rate, channels)),
            frame: vec![0.0; channels],
        }
    }

    /// Switch to another design, keeping the filters' state so the audio
    /// carries on (sections that weren't running start from rest).
    pub fn set_design(&mut self, design: &Design) {
        let (old, new) = (self.design.sections().len(), design.sections().len());
        if new > old {
            for ch in 0..self.channels {
                self.state[ch * MAX_SECTIONS + old..ch * MAX_SECTIONS + new].fill([0.0; 2]);
            }
        }
        self.design = *design;
    }

    /// Frames the limiter delays the audio by (0 without it).
    pub fn latency(&self) -> usize {
        self.limiter.as_ref().map_or(0, |l| l.latency())
    }

    /// Process whole frames of `buf` in place.
    pub fn process(&mut self, buf: &mut [f32]) {
        let sections = self.design.sections();
        let n = sections.len();
        let preamp = self.design.preamp;
        for frame in buf.chunks_exact_mut(self.channels) {
            for (ch, s) in frame.iter_mut().enumerate() {
                let mut x = *s as f64 * preamp;
                let state = &mut self.state[ch * MAX_SECTIONS..ch * MAX_SECTIONS + n];
                for (q, st) in sections.iter().zip(state) {
                    let y = q.b0 * x + st[0];
                    st[0] = q.b1 * x - q.a1 * y + st[1];
                    st[1] = q.b2 * x - q.a2 * y;
                    x = y;
                }
                *s = if x.abs() < TINY { 0.0 } else { x as f32 };
            }
            if let Some(l) = &mut self.limiter {
                self.frame.copy_from_slice(frame);
                l.process(&mut self.frame);
                frame.copy_from_slice(&self.frame);
            }
        }
        let mut broken = false;
        for ch in 0..self.channels {
            for st in &mut self.state[ch * MAX_SECTIONS..ch * MAX_SECTIONS + n] {
                for v in st {
                    if v.abs() < TINY {
                        *v = 0.0;
                    }
                    broken |= !v.is_finite();
                }
            }
        }
        // A NaN from upstream would ring in the filters forever: start over.
        if broken {
            self.reset();
            buf.fill(0.0);
        }
    }

    pub fn reset(&mut self) {
        self.state.fill([0.0; 2]);
        if let Some(l) = &mut self.limiter {
            l.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peaking_filter_has_its_gain_at_its_frequency_and_none_far_away() {
        let f = Filter::new(Kind::Peaking, 1000.0, 4.0, 0.707).biquad(48000.0);
        assert!((f.response_db(1000.0, 48000.0) - 4.0).abs() < 1e-9);
        assert!(f.response_db(20.0, 48000.0).abs() < 0.01);
    }

    #[test]
    fn shelves_reach_their_gain() {
        let ls = Filter::new(Kind::LowShelf, 100.0, 3.0, 0.707).biquad(48000.0);
        assert!((ls.response_db(10.0, 48000.0) - 3.0).abs() < 0.01);
        assert!(ls.response_db(10000.0, 48000.0).abs() < 0.01);
        let hs = Filter::new(Kind::HighShelf, 5000.0, -2.0, 0.707).biquad(48000.0);
        assert!((hs.response_db(23000.0, 48000.0) + 2.0).abs() < 0.05);
    }

    #[test]
    fn text_round_trips() {
        let c = Curve {
            preamp_db: -7.5,
            filters: vec![Filter::new(Kind::Peaking, 55.0, 4.0, 0.707), Filter::new(Kind::LowShelf, 80.0, 1.5, 0.72)],
        };
        assert_eq!(Curve::parse(&c.to_string()).unwrap(), c);
        let autoeq = "Preamp: -6.2 dB\nFilter 1: ON PK Fc 105 Hz Gain -3.4 dB Q 0.70\nFilter 2: OFF PK Fc 1 Hz Gain 0 dB Q 1\n";
        let p = Curve::parse(autoeq).unwrap();
        assert_eq!(p.filters.len(), 1);
        assert_eq!(p.preamp_db, -6.2);
    }

    #[test]
    fn curves_that_would_make_noise_are_refused() {
        for text in ["Preamp: NaN dB", "Filter: ON PK Fc 100 Hz Gain 3 dB Q 0", "Filter: ON PK Fc -5 Hz Gain 3 dB Q 1", "Filter: ON PK Fc 100 Hz Gain inf dB Q 1"] {
            assert!(Curve::parse(text).is_err(), "{text}");
        }
    }

    #[test]
    fn processing_matches_the_designed_response() {
        let curve = Curve { preamp_db: -3.0, filters: vec![Filter::new(Kind::Peaking, 1000.0, 6.0, 1.0)] };
        let mut p = Processor::new(&curve.design_rbj(48000.0), 48000, 2, false);
        let n = 48000;
        let mut buf: Vec<f32> =
            (0..n).flat_map(|i| [(2.0 * PI * 1000.0 * i as f64 / 48000.0).sin() as f32 * 0.1; 2]).collect();
        p.process(&mut buf);
        let tail = &buf[n..];
        let rms = (tail.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
        let db = 20.0 * (rms / (0.1 / 2f64.sqrt())).log10();
        assert!((db - 3.0).abs() < 0.01, "{db}");
    }

    #[test]
    fn silence_after_audio_settles_to_exact_zero() {
        // Low, narrow filters ring longest; with no input their state used to
        // settle into a denormal limit cycle instead of reaching zero.
        let curve = Curve {
            preamp_db: -6.0,
            filters: vec![Filter::new(Kind::Peaking, 20.0, 12.0, 10.0), Filter::new(Kind::LowShelf, 40.0, 6.0, 0.7), Filter::new(Kind::Peaking, 15000.0, -6.0, 4.0)],
        };
        let mut p = Processor::new(&curve.design(48000.0), 48000, 2, true);
        let mut buf: Vec<f32> = (0..9600).flat_map(|i| [(2.0 * PI * 30.0 * i as f64 / 48000.0).sin() as f32 * 0.5; 2]).collect();
        p.process(&mut buf);
        for _ in 0..2000 {
            let mut silent = vec![0.0f32; 960];
            p.process(&mut silent);
        }
        assert!(p.state.iter().flatten().all(|&v| v == 0.0), "state left: {:?}", p.state.iter().flatten().find(|&&v| v != 0.0));
        let mut silent = vec![0.0f32; 960];
        p.process(&mut silent);
        assert!(silent.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn a_nan_from_upstream_doesnt_stick() {
        let curve = Curve { preamp_db: 0.0, filters: vec![Filter::new(Kind::Peaking, 100.0, 6.0, 1.0)] };
        let mut p = Processor::new(&curve.design(48000.0), 48000, 1, true);
        let mut bad = vec![0.1f32; 480];
        bad[7] = f32::NAN;
        p.process(&mut bad);
        let mut next: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin() * 0.1).collect();
        p.process(&mut next);
        assert!(next.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn switching_designs_keeps_the_audio_going() {
        let tone = |i: usize| (2.0 * PI * 200.0 * i as f64 / 48000.0).sin() as f32 * 0.5;
        let one = Curve { preamp_db: 0.0, filters: vec![Filter::new(Kind::Peaking, 1000.0, 0.0, 1.0)] }.design(48000.0);
        let three = Curve { preamp_db: 0.0, filters: vec![Filter::new(Kind::Peaking, 5000.0, 0.0, 1.0); 3] }.design(48000.0);
        let mut p = Processor::new(&one, 48000, 1, false);
        let mut buf: Vec<f32> = (0..4800).map(tone).collect();
        p.process(&mut buf);
        p.set_design(&three);
        let mut more: Vec<f32> = (4800..9600).map(tone).collect();
        p.process(&mut more);
        // 0 dB filters: the tone comes through untouched, with no click at the switch.
        for (i, y) in more.iter().enumerate() {
            assert!((y - tone(4800 + i)).abs() < 1e-3, "{i}: {y}");
        }
    }
}
