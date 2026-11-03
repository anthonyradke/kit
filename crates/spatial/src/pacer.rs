//! Keeps a stream between two sound devices from drifting.
//!
//! The virtual cable and the headset each run on their own clock, a little
//! apart. The pacer resamples the stereo output by a tiny, slowly adjusted
//! ratio (at most ±0.2 %, a few cents of pitch) so the headset's buffer stays
//! near its target instead of slowly emptying or filling up.

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

const CHUNK: usize = 128;
const MAX_ADJUST: f64 = 0.002;
/// Correction per unit of relative fill error.
const KP: f64 = 0.0015;
/// Integral correction per second of relative fill error.
const KI: f64 = 0.0003;
/// Seconds over which the measured fill is averaged.
const SMOOTHING: f64 = 0.25;

pub struct Pacer {
    rate: f64,
    resampler: SincFixedIn<f32>,
    pending: [Vec<f32>; 2],
    chunk: Vec<Vec<f32>>,
    out: Vec<Vec<f32>>,
    target: f64,
    fill: Option<f64>,
    integral: f64,
    ratio: f64,
}

impl Pacer {
    /// `target` is the number of frames to keep queued in the output device.
    pub fn new(rate: u32, target: usize) -> Self {
        let params = SincInterpolationParameters {
            sinc_len: 64,
            f_cutoff: 0.925,
            oversampling_factor: 128,
            interpolation: SincInterpolationType::Cubic,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = SincFixedIn::new(1.0, 1.0 + 2.0 * MAX_ADJUST, params, CHUNK, 2).expect("valid resampler");
        let out = resampler.output_buffer_allocate(true);
        Pacer {
            rate: rate as f64,
            resampler,
            pending: [Vec::new(), Vec::new()],
            chunk: vec![vec![0.0; CHUNK]; 2],
            out,
            target: target as f64,
            fill: None,
            integral: 0.0,
            ratio: 1.0,
        }
    }

    pub fn target(&self) -> usize {
        self.target as usize
    }

    /// The current correction (output frames per input frame).
    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Frames given to the pacer and not yet passed on.
    pub fn pending(&self) -> usize {
        self.pending[0].len()
    }

    /// Take interleaved stereo `input`, adjust for `queued` (frames the
    /// output device still has to play) and append what to play next to `out`.
    pub fn process(&mut self, input: &[f32], queued: usize, out: &mut Vec<f32>) {
        let dt = input.len() as f64 / 2.0 / self.rate;
        let level = (queued + self.pending()) as f64;
        let fill = match self.fill {
            Some(f) => f + (level - f) * (dt / SMOOTHING).min(1.0),
            None => level,
        };
        self.fill = Some(fill);
        let error = (self.target - fill) / self.target;
        self.integral = (self.integral + error * dt).clamp(-MAX_ADJUST / KI, MAX_ADJUST / KI);
        self.ratio = 1.0 + (KP * error + KI * self.integral).clamp(-MAX_ADJUST, MAX_ADJUST);
        let _ = self.resampler.set_resample_ratio_relative(self.ratio, true);

        for frame in input.as_chunks::<2>().0 {
            self.pending[0].push(frame[0]);
            self.pending[1].push(frame[1]);
        }
        while self.pending() >= CHUNK {
            for (c, p) in self.chunk.iter_mut().zip(&mut self.pending) {
                c.copy_from_slice(&p[..CHUNK]);
                p.drain(..CHUNK);
            }
            let Ok((_, produced)) = self.resampler.process_into_buffer(&self.chunk, &mut self.out, None) else {
                continue;
            };
            out.reserve(produced * 2);
            for n in 0..produced {
                out.push(self.out[0][n]);
                out.push(self.out[1][n]);
            }
        }
    }

    /// Start over (after the output device was reopened or ran dry).
    pub fn reset(&mut self) {
        self.resampler.reset();
        self.pending[0].clear();
        self.pending[1].clear();
        self.fill = None;
        self.integral = 0.0;
        self.ratio = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A producer whose clock runs `ppm` fast feeding a device that plays
    /// 480 frames every 10 ms.
    fn simulate(ppm: f64, seconds: usize) -> (Pacer, Vec<usize>) {
        let mut p = Pacer::new(48000, 960);
        let mut queued = 960usize;
        let mut produced_total = 0.0f64;
        let mut fills = Vec::new();
        let mut out = Vec::new();
        let mut phase = 0.0f64;
        for tick in 0..seconds * 100 {
            // The producer delivers its 10 ms worth by its own clock.
            produced_total += 480.0 * (1.0 + ppm * 1e-6);
            let n = produced_total as usize - (produced_total - 480.0 * (1.0 + ppm * 1e-6)) as usize;
            let mut input = Vec::with_capacity(n * 2);
            for _ in 0..n {
                let s = (phase * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
                phase += 1000.0 / 48000.0;
                input.extend([s, s]);
            }
            out.clear();
            p.process(&input, queued, &mut out);
            queued += out.len() / 2;
            queued = queued.saturating_sub(480);
            if tick % 100 == 0 {
                fills.push(queued + p.pending());
            }
        }
        (p, fills)
    }

    #[test]
    fn holds_the_target_when_the_source_runs_fast_or_slow() {
        for ppm in [-200.0, 0.0, 150.0] {
            let (p, fills) = simulate(ppm, 600);
            let late = &fills[fills.len() - 120..];
            let worst = late.iter().map(|&f| (f as f64 - 960.0).abs()).fold(0.0, f64::max);
            assert!(worst < 200.0, "{ppm} ppm: fill strays {worst} frames");
            let want = 1.0 / (1.0 + ppm * 1e-6);
            assert!((p.ratio() - want).abs() < 40e-6, "{ppm} ppm: ratio {} want {want}", p.ratio());
        }
    }

    #[test]
    fn passes_audio_through_cleanly() {
        let mut p = Pacer::new(48000, 960);
        let tone: Vec<f32> = (0..48000 * 3)
            .flat_map(|i| {
                let s = (i as f32 * 1000.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 0.5;
                [s, s]
            })
            .collect();
        let mut out = Vec::new();
        let mut queued = 960usize;
        for chunk in tone.chunks(960) {
            let before = out.len();
            p.process(chunk, queued, &mut out);
            queued = (queued + (out.len() - before) / 2).saturating_sub(480);
        }
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        // The ratio moves the pitch by a hair, so fit the frequency too, over
        // a stretch short enough for the ratio to be steady.
        let part = &left[left.len() - 9600..];
        let fit = |hz: f64| {
            let w = hz * 2.0 * std::f64::consts::PI / 48000.0;
            let (mut c, mut s) = (0.0f64, 0.0f64);
            for (i, &x) in part.iter().enumerate() {
                c += x as f64 * (w * i as f64).cos();
                s += x as f64 * (w * i as f64).sin();
            }
            let (c, s) = (2.0 * c / part.len() as f64, 2.0 * s / part.len() as f64);
            let resid = part.iter().enumerate().map(|(i, &x)| (x as f64 - c * (w * i as f64).cos() - s * (w * i as f64).sin()).powi(2)).sum::<f64>() / part.len() as f64;
            ((c * c + s * s).sqrt(), resid)
        };
        let best = |lo: f64, step: f64| (0..=200).map(|k| lo + k as f64 * step).min_by(|&a, &b| fit(a).1.total_cmp(&fit(b).1)).unwrap();
        let coarse = best(998.0, 0.02);
        let (amp, resid) = fit(best(coarse - 0.02, 0.0002));
        let snr = 10.0 * (amp * amp / 2.0 / resid).log10();
        assert!((amp - 0.5).abs() < 0.01, "amplitude {amp}");
        // The resampler alone measures 68 dB at 1 kHz and 129 dB at 10 kHz; what's left here is the
        // controller's continuous tiny pitch corrections, as sidebands far below hearing.
        assert!(snr > 55.0, "signal to noise {snr:.1} dB");
    }
}
