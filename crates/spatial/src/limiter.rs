//! A stereo lookahead peak limiter, so the virtual speakers and room adding
//! up can never clip.
//!
//! The gain needed by each sample is held for the lookahead (a running
//! minimum), released slowly, then smoothed by a moving average of the same
//! length. With the audio delayed by that length, the gain has fully reached
//! what a peak needs by the time the peak comes out, without stepping.

pub const LOOKAHEAD: usize = 48;
pub const CEILING: f32 = 0.966; // -0.3 dBFS
const RELEASE_SECONDS: f32 = 0.12;

pub struct Limiter {
    audio: Vec<[f32; 2]>,
    need: Vec<f32>,
    held: Vec<f32>,
    pos: usize,
    release: f32,
    released: f32,
    sum: f64,
}

impl Limiter {
    pub fn new(rate: u32) -> Self {
        Limiter {
            audio: vec![[0.0; 2]; LOOKAHEAD],
            need: vec![1.0; LOOKAHEAD],
            held: vec![1.0; LOOKAHEAD],
            pos: 0,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * rate as f32)).exp(),
            released: 1.0,
            sum: LOOKAHEAD as f64,
        }
    }

    /// Limit one frame; the result is the frame from `LOOKAHEAD - 1` frames ago.
    pub fn process(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let peak = frame[0].abs().max(frame[1].abs());
        let need = if peak > CEILING { CEILING / peak } else { 1.0 };
        let i = self.pos;
        let out_frame = self.audio[(i + 1) % LOOKAHEAD];
        self.audio[i] = frame;
        self.need[i] = need;
        let min = self.need.iter().copied().fold(1.0f32, f32::min);
        // Down at once, back up slowly; never above what the window needs.
        self.released = if min < self.released { min } else { self.released + (min - self.released) * self.release };
        self.sum += (self.released - self.held[i]) as f64;
        self.held[i] = self.released;
        self.pos = (i + 1) % LOOKAHEAD;
        let gain = (self.sum / LOOKAHEAD as f64) as f32;
        // The average can only lag the need by rounding; clamp what's left.
        [(out_frame[0] * gain).clamp(-CEILING, CEILING), (out_frame[1] * gain).clamp(-CEILING, CEILING)]
    }

    pub fn reset(&mut self) {
        self.audio.fill([0.0; 2]);
        self.need.fill(1.0);
        self.held.fill(1.0);
        self.released = 1.0;
        self.sum = LOOKAHEAD as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_audio_passes_unchanged_after_the_delay() {
        let mut l = Limiter::new(48000);
        let x: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let y: Vec<f32> = x.iter().map(|&s| l.process([s, -s])[0]).collect();
        for n in LOOKAHEAD..1000 {
            assert!((y[n] - x[n - (LOOKAHEAD - 1)]).abs() < 1e-6);
        }
    }

    #[test]
    fn loud_audio_never_exceeds_the_ceiling_and_keeps_its_shape() {
        let mut l = Limiter::new(48000);
        let mut loud = Vec::new();
        for i in 0..48000 {
            let s = (i as f32 * 0.01).sin() * if i > 10000 { 4.0 } else { 0.2 };
            let [a, b] = l.process([s, s * 0.5]);
            assert!(a.abs() <= CEILING && b.abs() <= CEILING);
            if i > 20000 {
                loud.push(a);
            }
        }
        // Turned down, not squared off: a sine's peak is sqrt(2) times its RMS.
        let rms = (loud.iter().map(|x| x * x).sum::<f32>() / loud.len() as f32).sqrt();
        let peak = loud.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(peak / rms > 1.38, "crest factor {}", peak / rms);
        assert!(peak > CEILING * 0.95);
    }
}
