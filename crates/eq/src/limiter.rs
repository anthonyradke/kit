//! A lookahead peak limiter, linked across channels, so EQ boosts on top of
//! full-scale audio come out quieter instead of clipping.
//!
//! The gain each frame needs is held over the lookahead (a running minimum),
//! released slowly, then smoothed by a moving average of the same length.
//! With the audio delayed by that length the gain has fully reached what a
//! peak needs by the time the peak comes out, without stepping.

/// 1 ms at 48 kHz.
pub const LOOKAHEAD: usize = 48;
/// -0.3 dBFS, room for the conversion to the headset's integer format.
pub const CEILING: f32 = 0.966;
const RELEASE_SECONDS: f32 = 0.12;

pub struct Limiter {
    channels: usize,
    /// `LOOKAHEAD` frames of delayed audio.
    audio: Vec<f32>,
    need: Vec<f32>,
    held: Vec<f32>,
    pos: usize,
    release: f32,
    released: f32,
    sum: f64,
}

impl Limiter {
    pub fn new(rate: u32, channels: usize) -> Self {
        Limiter {
            channels,
            audio: vec![0.0; LOOKAHEAD * channels],
            need: vec![1.0; LOOKAHEAD],
            held: vec![1.0; LOOKAHEAD],
            pos: 0,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * rate as f32)).exp(),
            released: 1.0,
            sum: LOOKAHEAD as f64,
        }
    }

    /// Frames of delay.
    pub fn latency(&self) -> usize {
        LOOKAHEAD - 1
    }

    /// Limit one frame in place; it is replaced by the frame from
    /// [`Limiter::latency`] frames ago.
    pub fn process(&mut self, frame: &mut [f32]) {
        let c = self.channels;
        let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let need = if peak > CEILING { CEILING / peak } else { 1.0 };
        let i = self.pos;
        let next = (i + 1) % LOOKAHEAD;
        let min = {
            self.need[i] = need;
            self.need.iter().copied().fold(1.0f32, f32::min)
        };
        // Down at once, back up slowly; never above what the window needs.
        self.released = if min < self.released { min } else { self.released + (min - self.released) * self.release };
        self.sum += (self.released - self.held[i]) as f64;
        self.held[i] = self.released;
        let gain = (self.sum / LOOKAHEAD as f64) as f32;
        for (k, s) in frame.iter_mut().enumerate() {
            let input = *s;
            // The average can only lag the need by rounding; clamp what's left.
            *s = (self.audio[next * c + k] * gain).clamp(-CEILING, CEILING);
            self.audio[i * c + k] = input;
        }
        self.pos = next;
    }

    pub fn reset(&mut self) {
        self.audio.fill(0.0);
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
        let mut l = Limiter::new(48000, 2);
        let x: Vec<f32> = (0..1000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let y: Vec<f32> = x
            .iter()
            .map(|&s| {
                let mut f = [s, -s];
                l.process(&mut f);
                f[0]
            })
            .collect();
        for n in LOOKAHEAD..1000 {
            assert!((y[n] - x[n - l.latency()]).abs() < 1e-6);
        }
    }

    #[test]
    fn loud_audio_never_exceeds_the_ceiling() {
        let mut l = Limiter::new(48000, 2);
        for i in 0..48000 {
            let s = (i as f32 * 0.01).sin() * 2.5;
            let mut f = [s, s * 0.5];
            l.process(&mut f);
            assert!(f[0].abs() <= CEILING && f[1].abs() <= CEILING);
        }
    }
}
