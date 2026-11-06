//! Fitting a biquad curve to a measured response (Nelder-Mead on the dB
//! error over the log grid, 20 Hz to 20 kHz).

use kit_eq::Curve;

use crate::model::RATE;

pub struct Fit {
    pub curve: Curve,
    pub rms_db: f64,
    pub max_db: f64,
}

/// Error of `curve` run at `rate` against `target` (dB on `grid`): (rms, max abs).
pub fn error(curve: &Curve, rate: f64, grid: &[f64], target: &[f64]) -> (f64, f64) {
    let (mut sum, mut max) = (0.0, 0.0f64);
    for (&f, &t) in grid.iter().zip(target) {
        let e = curve.response_db(f, rate) - t;
        sum += e * e;
        max = max.max(e.abs());
    }
    ((sum / grid.len() as f64).sqrt(), max)
}

fn nelder_mead(cost: &dyn Fn(&[f64]) -> f64, x0: &[f64], step: &[f64], iters: usize) -> Vec<f64> {
    let n = x0.len();
    let mut pts: Vec<Vec<f64>> = vec![x0.to_vec()];
    for i in 0..n {
        let mut p = x0.to_vec();
        p[i] += step[i];
        pts.push(p);
    }
    let mut vals: Vec<f64> = pts.iter().map(|p| cost(p)).collect();
    for _ in 0..iters {
        let mut order: Vec<usize> = (0..=n).collect();
        order.sort_by(|&a, &b| vals[a].total_cmp(&vals[b]));
        pts = order.iter().map(|&i| pts[i].clone()).collect();
        vals = order.iter().map(|&i| vals[i]).collect();
        if (vals[n] - vals[0]).abs() < 1e-12 {
            break;
        }
        let centroid: Vec<f64> = (0..n).map(|j| pts[..n].iter().map(|p| p[j]).sum::<f64>() / n as f64).collect();
        let along = |t: f64| -> Vec<f64> { (0..n).map(|j| centroid[j] + t * (pts[n][j] - centroid[j])).collect() };
        let r = along(-1.0);
        let fr = cost(&r);
        if fr < vals[0] {
            let e = along(-2.0);
            let fe = cost(&e);
            (pts[n], vals[n]) = if fe < fr { (e, fe) } else { (r, fr) };
        } else if fr < vals[n - 1] {
            (pts[n], vals[n]) = (r, fr);
        } else {
            let c = if fr < vals[n] { along(-0.5) } else { along(0.5) };
            let fc = cost(&c);
            if fc < vals[n].min(fr) {
                (pts[n], vals[n]) = (c, fc);
            } else {
                for i in 1..=n {
                    pts[i] = (0..n).map(|j| pts[0][j] + 0.5 * (pts[i][j] - pts[0][j])).collect();
                    vals[i] = cost(&pts[i]);
                }
            }
        }
    }
    let best = (0..=n).min_by(|&a, &b| vals[a].total_cmp(&vals[b])).unwrap();
    pts[best].clone()
}

/// Fit every parameter of `start`, run at 48 kHz, to `target`.
/// Frequencies and Qs move on a log scale.
pub fn fit(start: &Curve, grid: &[f64], target: &[f64]) -> Fit {
    let encode = |c: &Curve| -> Vec<f64> {
        let mut p = vec![c.preamp_db];
        for f in &c.filters {
            p.extend([f.freq.ln(), f.gain_db, f.q.ln()]);
        }
        p
    };
    let decode = |p: &[f64]| -> Curve {
        let mut c = start.clone();
        c.preamp_db = p[0];
        for (k, f) in c.filters.iter_mut().enumerate() {
            let q = &p[1 + 3 * k..4 + 3 * k];
            f.freq = q[0].exp().clamp(10.0, 23000.0);
            f.gain_db = q[1];
            f.q = q[2].exp().clamp(0.1, 20.0);
        }
        c
    };
    let cost = |p: &[f64]| error(&decode(p), RATE, grid, target).0;
    let mut p = encode(start);
    // Half a dB for the preamp and each gain, a tenth (in log units) for frequencies and Qs.
    let step: Vec<f64> = (0..p.len()).map(|i| if i == 0 || (i - 1) % 3 == 1 { 0.5 } else { 0.1 }).collect();
    // Restarts get Nelder-Mead out of the corners it collapses into.
    for _ in 0..12 {
        p = nelder_mead(&cost, &p, &step, 4000 * p.len());
    }
    let curve = decode(&p);
    let (rms_db, max_db) = error(&curve, RATE, grid, target);
    Fit { curve, rms_db, max_db }
}
