//! Turning a [`Curve`] into the biquads that run.
//!
//! Sonar runs its EQ at 96 kHz (measured: its filters match RBJ biquads
//! designed at 96 kHz to 0.02 dB). At 48 kHz the same RBJ formulas squeeze a
//! filter's top side toward Nyquist (the bilinear transform's cramping), so a
//! treble boost comes out up to 2 dB short at 16-20 kHz. [`Curve::design`]
//! instead designs every filter at twice the rate and then finds the biquad
//! at the real rate whose response matches it:
//!
//! 1. the poles are the double-rate filter's poles squared (one sample at the
//!    real rate is two at the double rate, so these are the same
//!    continuous-time poles),
//! 2. the numerator is the least-squares fit of the magnitude over 20 Hz to
//!    20 kHz, which is linear in the right parameters (Vicanek, "Matched
//!    second order digital filters", 2016),
//! 3. a few Gauss-Newton steps on the error in dB then move all five
//!    coefficients together.
//!
//! For Sonar's Main preset that lands within 0.1 dB of the 96 kHz filters
//! everywhere up to 20 kHz (0.01 dB rms). It takes well under a millisecond
//! per filter, so it runs whenever a curve changes but never on the audio
//! thread.

use std::f64::consts::{LN_10, PI};

use crate::{Biquad, Curve, Filter};

/// Most filters a [`Design`] holds: the headset's 10 bands, the bass slider
/// and room to spare.
pub const MAX_SECTIONS: usize = 16;

/// A curve ready to run: linear preamp gain and up to [`MAX_SECTIONS`]
/// biquads, in a fixed-size block so it can be copied into a running
/// processor without allocating.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Design {
    pub preamp: f64,
    len: usize,
    sections: [Biquad; MAX_SECTIONS],
}

const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };

impl Default for Design {
    fn default() -> Self {
        Design { preamp: 1.0, len: 0, sections: [IDENTITY; MAX_SECTIONS] }
    }
}

impl Design {
    /// Filters past [`MAX_SECTIONS`] are dropped.
    pub fn new(preamp_db: f64, sections: impl IntoIterator<Item = Biquad>) -> Self {
        let mut d = Design { preamp: 10f64.powf(preamp_db / 20.0), ..Design::default() };
        for s in sections.into_iter().take(MAX_SECTIONS) {
            d.sections[d.len] = s;
            d.len += 1;
        }
        d
    }

    pub fn sections(&self) -> &[Biquad] {
        &self.sections[..self.len]
    }

    /// The design's gain in dB at `freq`, preamp included.
    pub fn response_db(&self, freq: f64, rate: f64) -> f64 {
        20.0 * self.preamp.log10() + self.sections().iter().map(|s| s.response_db(freq, rate)).sum::<f64>()
    }
}

impl Curve {
    /// Every filter as an RBJ biquad at `rate` itself: the headset
    /// firmware's way, and what `headset.js` draws for it.
    pub fn design_rbj(&self, rate: f64) -> Design {
        Design::new(self.preamp_db, self.filters.iter().map(|f| f.biquad(rate)))
    }

    /// Every filter as Sonar runs it (designed at twice `rate`), matched at
    /// `rate`. See the module docs.
    pub fn design(&self, rate: f64) -> Design {
        Design::new(self.preamp_db, self.filters.iter().map(|f| f.matched(rate)))
    }
}

/// Where the fit looks: 20 Hz to 20 kHz (or 90% of Nyquist), log spaced.
const POINTS: usize = 160;
const ITERATIONS: usize = 10;

/// cos and sin of w and 2w at one frequency.
#[derive(Clone, Copy)]
struct Point {
    w: f64,
    c1: f64,
    s1: f64,
    c2: f64,
    s2: f64,
}

impl Point {
    fn new(w: f64) -> Self {
        Point { w, c1: w.cos(), s1: w.sin(), c2: (2.0 * w).cos(), s2: (2.0 * w).sin() }
    }

    /// |numerator|^2 and |denominator|^2 of `q` here, with their real and
    /// imaginary parts.
    fn parts(&self, q: &Biquad) -> ([f64; 2], [f64; 2]) {
        let n = [q.b0 + q.b1 * self.c1 + q.b2 * self.c2, -(q.b1 * self.s1 + q.b2 * self.s2)];
        let d = [1.0 + q.a1 * self.c1 + q.a2 * self.c2, -(q.a1 * self.s1 + q.a2 * self.s2)];
        (n, d)
    }

    fn db(&self, q: &Biquad) -> f64 {
        let (n, d) = self.parts(q);
        10.0 * ((n[0] * n[0] + n[1] * n[1]) / (d[0] * d[0] + d[1] * d[1])).log10()
    }
}

/// Poles strictly inside the unit circle (the stability triangle).
fn stable(q: &Biquad) -> bool {
    q.a2.abs() < 1.0 && q.a1.abs() < 1.0 + q.a2 && [q.b0, q.b1, q.b2, q.a1, q.a2].iter().all(|x| x.is_finite())
}

/// Solve the `N`x`N` system `m x = v` by Gaussian elimination with partial
/// pivoting; `None` if it is singular.
fn solve<const N: usize>(mut m: [[f64; N]; N], mut v: [f64; N]) -> Option<[f64; N]> {
    for col in 0..N {
        let pivot = (col..N).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-300 {
            return None;
        }
        m.swap(col, pivot);
        v.swap(col, pivot);
        let (top, rest) = m.split_at_mut(col + 1);
        let pivot_row = &top[col];
        for (i, row) in rest.iter_mut().enumerate() {
            let k = row[col] / pivot_row[col];
            for (x, p) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                *x -= k * p;
            }
            v[col + 1 + i] -= k * v[col];
        }
    }
    let mut x = [0.0; N];
    for row in (0..N).rev() {
        let s: f64 = (row + 1..N).map(|c| m[row][c] * x[c]).sum();
        x[row] = (v[row] - s) / m[row][row];
    }
    x.iter().all(|x| x.is_finite()).then_some(x)
}

impl Filter {
    /// This filter as Sonar runs it, designed at twice `rate` and matched at
    /// `rate` (see the module docs). Falls back to the plain RBJ design at
    /// `rate` if the match can't be made.
    pub fn matched(&self, rate: f64) -> Biquad {
        self.try_matched(rate).unwrap_or_else(|| self.biquad(rate))
    }

    fn try_matched(&self, rate: f64) -> Option<Biquad> {
        let target = self.biquad(2.0 * rate);
        let top = 20000f64.min(0.45 * rate);
        let points: Vec<Point> =
            (0..POINTS).map(|k| 20.0 * (top / 20.0).powf(k as f64 / (POINTS - 1) as f64)).map(|f| Point::new(2.0 * PI * f / rate)).collect();
        // The target in dB, at half the angle (the double rate).
        let want: Vec<f64> = points.iter().map(|p| Point::new(p.w / 2.0).db(&target)).collect();

        // 1. Poles squared: z^2 + a1 z + a2 has roots p, q; the polynomial
        // with roots p^2, q^2 is z^2 - (p^2 + q^2) z + (pq)^2.
        let a1 = 2.0 * target.a2 - target.a1 * target.a1;
        let a2 = target.a2 * target.a2;

        // 2. |B(w)|^2 = B0 f0 + B1 f1 + B2 f2 with f1 = sin^2(w/2),
        // f0 = 1 - f1, f2 = 4 f0 f1; fit it to want * |A(w)|^2 in relative terms.
        let mut m = [[0.0; 3]; 3];
        let mut v = [0.0; 3];
        for (p, &db) in points.iter().zip(&want) {
            let f1 = (p.w / 2.0).sin().powi(2);
            let f0 = 1.0 - f1;
            let phi = [f0, f1, 4.0 * f0 * f1];
            let a_mag = (1.0 + a1 + a2).powi(2) * phi[0] + (1.0 - a1 + a2).powi(2) * phi[1] - 4.0 * a2 * phi[2];
            let t = 10f64.powf(db / 10.0) * a_mag;
            let row = phi.map(|x| x / t);
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += row[i] * row[j];
                }
                v[i] += row[i];
            }
        }
        let [big0, big1, big2] = solve(m, v)?;
        let (s0, s1) = (big0.max(0.0).sqrt(), big1.max(0.0).sqrt());
        let w = (s0 + s1) / 2.0;
        let b0 = (w + (w * w + big2).max(0.0).sqrt()) / 2.0;
        if b0 <= 0.0 {
            return None;
        }
        let start = Biquad { b0, b1: (s0 - s1) / 2.0, b2: -big2 / (4.0 * b0), a1, a2 };
        if !stable(&start) {
            return None;
        }

        // 3. Gauss-Newton (with a little damping) on the dB error.
        let error = |q: &Biquad| points.iter().zip(&want).map(|(p, w)| (p.db(q) - w).powi(2)).sum::<f64>();
        let mut best = start;
        let mut best_err = error(&best);
        let mut damping = 1e-6;
        for _ in 0..ITERATIONS {
            let mut jtj = [[0.0; 5]; 5];
            let mut jtr = [0.0; 5];
            for (p, &want) in points.iter().zip(&want) {
                let (n, d) = p.parts(&best);
                let (nn, dd) = (n[0] * n[0] + n[1] * n[1], d[0] * d[0] + d[1] * d[1]);
                let r = 10.0 * (nn / dd).log10() - want;
                let k = 10.0 / LN_10;
                // d|X|^2 / dx for each coefficient, over |X|^2, in dB.
                let j = [
                    k * 2.0 * n[0] / nn,
                    k * 2.0 * (n[0] * p.c1 - n[1] * p.s1) / nn,
                    k * 2.0 * (n[0] * p.c2 - n[1] * p.s2) / nn,
                    -k * 2.0 * (d[0] * p.c1 - d[1] * p.s1) / dd,
                    -k * 2.0 * (d[0] * p.c2 - d[1] * p.s2) / dd,
                ];
                for a in 0..5 {
                    for b in 0..5 {
                        jtj[a][b] += j[a] * j[b];
                    }
                    jtr[a] -= j[a] * r;
                }
            }
            let mut damped = jtj;
            for (i, row) in damped.iter_mut().enumerate() {
                row[i] += damping * (jtj[i][i] + 1e-12);
            }
            let Some(dx) = solve(damped, jtr) else { break };
            let next = Biquad {
                b0: best.b0 + dx[0],
                b1: best.b1 + dx[1],
                b2: best.b2 + dx[2],
                a1: best.a1 + dx[3],
                a2: best.a2 + dx[4],
            };
            let err = if stable(&next) { error(&next) } else { f64::INFINITY };
            if err < best_err {
                (best, best_err) = (next, err);
                damping = (damping * 0.1).max(1e-9);
            } else {
                damping *= 100.0;
            }
        }
        Some(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    fn worst(f: &Filter, rate: f64) -> f64 {
        let q = f.matched(rate);
        let t = f.biquad(2.0 * rate);
        (0..=400)
            .map(|k| 20.0 * 1000f64.powf(k as f64 / 400.0))
            .map(|fr| (q.response_db(fr, rate) - t.response_db(fr, 2.0 * rate)).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn sonar_main_matches_the_96k_design() {
        let pk = |f, g, q| Filter::new(Kind::Peaking, f, g, q);
        let curve = Curve {
            preamp_db: 0.0,
            filters: vec![
                pk(55.0, 4.0, 0.707),
                pk(100.0, 3.0, 0.707),
                pk(600.0, 4.0, 4.0),
                pk(5000.0, 2.0, 0.707),
                pk(8000.0, 3.0, 0.707),
                pk(12000.0, 3.0, 0.707),
                pk(80.0, 1.5, 0.707),
            ],
        };
        let d = curve.design(48000.0);
        let (mut max, mut sum, mut n) = (0f64, 0.0, 0.0);
        let (mut rbj_max, rbj) = (0f64, curve.design_rbj(48000.0));
        for k in 0..=480 {
            let f = 20.0 * 1000f64.powf(k as f64 / 480.0);
            let want = curve.response_db(f, 96000.0);
            let e = d.response_db(f, 48000.0) - want;
            max = max.max(e.abs());
            rbj_max = rbj_max.max((rbj.response_db(f, 48000.0) - want).abs());
            sum += e * e;
            n += 1.0;
        }
        let rms = (sum / n).sqrt();
        assert!(max < 0.12 && rms < 0.015, "max {max:.3} rms {rms:.4}");
        // The plain design misses by nearly 2 dB at the top.
        assert!(rbj_max > 1.5, "{rbj_max}");
    }

    #[test]
    fn every_kind_matches_and_stays_stable() {
        for kind in [Kind::Peaking, Kind::LowShelf, Kind::HighShelf, Kind::LowPass, Kind::HighPass] {
            for freq in [30.0, 200.0, 1000.0, 6000.0, 12000.0, 18000.0] {
                for (gain, q) in [(6.0, 0.707), (-10.0, 2.0), (10.0, 0.3), (3.0, 8.0)] {
                    let f = Filter::new(kind, freq, gain, q);
                    for rate in [44100.0, 48000.0] {
                        let m = f.matched(rate);
                        assert!(stable(&m), "{f:?} at {rate}");
                        // Steep filters near the top are the hard case; the
                        // match must still beat the plain design there.
                        let e = worst(&f, rate);
                        let plain = {
                            let p = f.biquad(rate);
                            let t = f.biquad(2.0 * rate);
                            (0..=400)
                                .map(|k| 20.0 * 1000f64.powf(k as f64 / 400.0))
                                .map(|fr| (p.response_db(fr, rate) - t.response_db(fr, 2.0 * rate)).abs())
                                .fold(0.0, f64::max)
                        };
                        assert!(e <= plain + 0.02, "{f:?} at {rate}: matched {e:.3} dB off, plain {plain:.3}");
                        // Filters with a gain, in the range presets use, stay close.
                        if freq <= 6000.0 && !matches!(kind, Kind::LowPass | Kind::HighPass) {
                            assert!(e < 0.3, "{f:?} at {rate}: {e:.3} dB off");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn design_holds_at_most_max_sections() {
        let curve = Curve { preamp_db: -6.0, filters: vec![Filter::new(Kind::Peaking, 1000.0, 1.0, 1.0); 20] };
        let d = curve.design(48000.0);
        assert_eq!(d.sections().len(), MAX_SECTIONS);
        assert!((d.preamp - 0.501187).abs() < 1e-5);
    }
}
