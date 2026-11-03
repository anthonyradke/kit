//! Uniformly partitioned FFT convolution (overlap-save). Long filters are cut
//! into [`BLOCK`]-sample partitions; each block of input is transformed once
//! and multiplied with every partition against the matching older input.

use std::sync::Arc;

use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

/// Samples per processing block; also the renderer's buffering latency.
pub const BLOCK: usize = 256;
const SIZE: usize = 2 * BLOCK;
pub const BINS: usize = BLOCK + 1;

pub type Spectrum = Vec<Complex<f32>>;

pub struct Fft {
    forward: Arc<dyn RealToComplex<f32>>,
    inverse: Arc<dyn ComplexToReal<f32>>,
    time: Vec<f32>,
    scratch: Vec<Complex<f32>>,
}

impl Fft {
    pub fn new() -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let forward = planner.plan_fft_forward(SIZE);
        let inverse = planner.plan_fft_inverse(SIZE);
        let scratch_len = forward.get_scratch_len().max(inverse.get_scratch_len());
        Fft { forward, inverse, time: vec![0.0; SIZE], scratch: vec![Complex::default(); scratch_len] }
    }

    /// Spectrum of `2 * BLOCK` samples (shorter input is zero padded).
    pub fn forward(&mut self, input: &[f32], out: &mut [Complex<f32>]) {
        self.time.fill(0.0);
        self.time[..input.len()].copy_from_slice(input);
        let _ = self.forward.process_with_scratch(&mut self.time, out, &mut self.scratch);
    }

    /// The last `BLOCK` samples of the inverse of `spec` (the valid part in
    /// overlap-save), scaled to undo the transform's gain. Destroys `spec`.
    pub fn inverse_tail(&mut self, spec: &mut [Complex<f32>], out: &mut [f32]) {
        // A real signal's spectrum has no imaginary part at DC and Nyquist.
        spec[0].im = 0.0;
        spec[BINS - 1].im = 0.0;
        let _ = self.inverse.process_with_scratch(spec, &mut self.time, &mut self.scratch);
        let scale = 1.0 / SIZE as f32;
        for (o, t) in out.iter_mut().zip(&self.time[BLOCK..]) {
            *o = t * scale;
        }
    }
}

impl Default for Fft {
    fn default() -> Self {
        Self::new()
    }
}

/// A filter as the spectra of its partitions. Partitions that are all zero
/// (the start of the late room) are left out.
pub struct Partitioned {
    parts: Vec<(usize, Spectrum)>,
    /// Partitions, including the skipped ones.
    pub len: usize,
}

impl Partitioned {
    pub fn new(fft: &mut Fft, ir: &[f32]) -> Self {
        let len = ir.len().div_ceil(BLOCK);
        let mut parts = Vec::new();
        for (i, chunk) in ir.chunks(BLOCK).enumerate() {
            if chunk.iter().all(|&x| x == 0.0) {
                continue;
            }
            let mut spec = vec![Complex::default(); BINS];
            fft.forward(chunk, &mut spec);
            parts.push((i, spec));
        }
        Partitioned { parts, len }
    }

    /// `acc += filter * input history`.
    pub fn apply(&self, history: &History, acc: &mut [Complex<f32>]) {
        for (age, h) in &self.parts {
            mac(acc, history.get(*age), h);
        }
    }
}

/// The spectra of the last few input blocks of one signal.
pub struct History {
    ring: Vec<Spectrum>,
    newest: usize,
}

impl History {
    pub fn new(len: usize) -> Self {
        History { ring: vec![vec![Complex::default(); BINS]; len.max(1)], newest: 0 }
    }

    /// Slot for the next block; it becomes age 0.
    pub fn push(&mut self) -> &mut [Complex<f32>] {
        self.newest = (self.newest + 1) % self.ring.len();
        &mut self.ring[self.newest]
    }

    /// The block `age` blocks ago (0 is the newest).
    pub fn get(&self, age: usize) -> &[Complex<f32>] {
        let n = self.ring.len();
        &self.ring[(self.newest + n - age % n) % n]
    }

    pub fn clear(&mut self) {
        for s in &mut self.ring {
            s.fill(Complex::default());
        }
    }
}

fn mac(acc: &mut [Complex<f32>], x: &[Complex<f32>], h: &[Complex<f32>]) {
    for ((a, x), h) in acc.iter_mut().zip(x).zip(h) {
        a.re += x.re * h.re - x.im * h.im;
        a.im += x.re * h.im + x.im * h.re;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain time-domain convolution, for checking.
    pub fn naive(x: &[f32], h: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0f32; x.len()];
        for n in 0..x.len() {
            let mut s = 0.0f64;
            for (k, &hk) in h.iter().enumerate().take(n + 1) {
                s += hk as f64 * x[n - k] as f64;
            }
            y[n] = s as f32;
        }
        y
    }

    #[test]
    fn matches_direct_convolution() {
        let mut fft = Fft::new();
        // A filter with a zero partition in the middle, like the tail.
        let mut h: Vec<f32> = (0..1500).map(|i| ((i * 7919 % 1000) as f32 / 500.0 - 1.0) * (-(i as f32) / 400.0).exp()).collect();
        h[256..512].fill(0.0);
        let x: Vec<f32> = (0..BLOCK * 12).map(|i| (i * 104729 % 997) as f32 / 498.0 - 1.0).collect();
        let filt = Partitioned::new(&mut fft, &h);
        assert_eq!(filt.parts.len(), filt.len - 1);
        let mut hist = History::new(filt.len);
        let mut frame = vec![0.0f32; SIZE];
        let mut out = Vec::new();
        for block in x.chunks(BLOCK) {
            frame.copy_within(BLOCK.., 0);
            frame[BLOCK..].copy_from_slice(block);
            let mut spec = vec![Complex::default(); BINS];
            fft.forward(&frame, &mut spec);
            hist.push().copy_from_slice(&spec);
            let mut acc = vec![Complex::default(); BINS];
            filt.apply(&hist, &mut acc);
            let mut y = vec![0.0; BLOCK];
            fft.inverse_tail(&mut acc, &mut y);
            out.extend(y);
        }
        let want = naive(&x, &h);
        let err = out.iter().zip(&want).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "max error {err}");
    }
}
