//! A lookahead peak limiter, linked across channels, so EQ boosts on top of
//! full-scale audio come out quieter instead of clipping.
//!
//! The gain each frame needs is the lowest any frame needs over the
//! lookahead plus a hold, then released slowly and smoothed by a moving
//! average as long as the lookahead. With the audio delayed by that length,
//! the gain has fully reached what a peak needs by the time the peak comes
//! out, without stepping.
//!
//! The hold is what keeps bass clean. Without it the gain dips at every
//! peak of a loud bass note and recovers between them, which is distortion
//! on the bass (about 1% at 40-60 Hz with a 1 ms lookahead and no hold) and
//! modulates everything else with it. Held for longer than half a period of
//! the lowest bass, the gain stays put through the note and only follows the
//! music's level. The release matches what Windows' own limiter was measured
//! doing on Sonar's output: after a loud 63 Hz burst it held about 17 ms and
//! came back with a 45 ms time constant (docs/sonar-eq.md).

/// How far ahead the gain looks, in seconds.
const LOOKAHEAD_SECONDS: f64 = 0.005;
/// How long a reduction is held after the peak that needed it: longer than
/// half a period of 20 Hz.
const HOLD_SECONDS: f64 = 0.025;
const RELEASE_SECONDS: f64 = 0.045;
/// -0.3 dBFS, room for the conversion to the headset's integer format.
pub const CEILING: f32 = 0.966;

pub struct Limiter {
    channels: usize,
    lookahead: usize,
    /// Lookahead plus hold: the frames the needed gain is the minimum over.
    window: usize,
    /// `lookahead` frames of delayed audio.
    audio: Vec<f32>,
    pos: usize,
    /// Sliding minimum of the needed gain over `window` frames: a ring of
    /// (frame number, need) pairs with needs rising from front to back.
    queue: Vec<(u64, f32)>,
    head: usize,
    len: usize,
    frame: u64,
    release: f32,
    released: f32,
    /// The last `lookahead` released gains, for the moving average.
    smooth: Vec<f32>,
    sum: f64,
}

impl Limiter {
    pub fn new(rate: u32, channels: usize) -> Self {
        let rate = rate.max(1) as f64;
        let lookahead = ((rate * LOOKAHEAD_SECONDS).round() as usize).max(1);
        let window = lookahead + (rate * HOLD_SECONDS).round() as usize;
        Limiter {
            channels,
            lookahead,
            window,
            audio: vec![0.0; lookahead * channels],
            pos: 0,
            queue: vec![(0, 1.0); window],
            head: 0,
            len: 0,
            frame: 0,
            release: (1.0 - (-1.0 / (RELEASE_SECONDS * rate)).exp()) as f32,
            released: 1.0,
            smooth: vec![1.0; lookahead],
            sum: lookahead as f64,
        }
    }

    /// Frames of delay.
    pub fn latency(&self) -> usize {
        self.lookahead - 1
    }

    /// Add this frame's need and return the lowest need in the window.
    fn window_min(&mut self, need: f32) -> f32 {
        let cap = self.window;
        // Needs at least as high as this one can never be the minimum again.
        while self.len > 0 && self.queue[(self.head + self.len - 1) % cap].1 >= need {
            self.len -= 1;
        }
        self.queue[(self.head + self.len) % cap] = (self.frame, need);
        self.len += 1;
        // Drop what has left the window (the newest entry never has).
        while self.frame - self.queue[self.head].0 >= cap as u64 {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        self.frame += 1;
        self.queue[self.head].1
    }

    /// Limit one frame in place; it is replaced by the frame from
    /// [`Limiter::latency`] frames ago.
    pub fn process(&mut self, frame: &mut [f32]) {
        let c = self.channels;
        let peak = frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let need = if peak > CEILING { CEILING / peak } else { 1.0 };
        let min = self.window_min(need);
        // Down at once, back up slowly; never above what the window needs.
        self.released = if min < self.released { min } else { self.released + (min - self.released) * self.release };
        let i = self.pos;
        self.sum += (self.released - self.smooth[i]) as f64;
        self.smooth[i] = self.released;
        let gain = (self.sum / self.lookahead as f64) as f32;
        let next = (i + 1) % self.lookahead;
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
        self.pos = 0;
        self.head = 0;
        self.len = 0;
        self.frame = 0;
        self.released = 1.0;
        self.smooth.fill(1.0);
        self.sum = self.lookahead as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn run(l: &mut Limiter, x: &[f32]) -> Vec<f32> {
        x.iter()
            .map(|&s| {
                let mut f = [s, -s];
                l.process(&mut f);
                f[0]
            })
            .collect()
    }

    /// Amplitude of the component at `f` (Hann-windowed DFT) in `x` at 48 kHz.
    fn amp(x: &[f32], f: f64) -> f64 {
        let n = x.len();
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in x.iter().enumerate() {
            let w = 0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos();
            let ph = 2.0 * PI * f * i as f64 / 48000.0;
            re += s as f64 * w * ph.cos();
            im -= s as f64 * w * ph.sin();
        }
        4.0 * (re * re + im * im).sqrt() / n as f64
    }

    #[test]
    fn quiet_audio_passes_unchanged_after_the_delay() {
        let mut l = Limiter::new(48000, 2);
        let x: Vec<f32> = (0..4000).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let y = run(&mut l, &x);
        assert_eq!(l.latency(), 239);
        for n in l.latency()..x.len() {
            assert!((y[n] - x[n - l.latency()]).abs() < 1e-6);
        }
    }

    #[test]
    fn loud_audio_never_exceeds_the_ceiling() {
        let mut l = Limiter::new(48000, 2);
        for i in 0..48000 {
            let s = (i as f32 * 0.01).sin() * 2.5 * if i % 7000 < 300 { 1.8 } else { 1.0 };
            let mut f = [s, s * 0.5];
            l.process(&mut f);
            assert!(f[0].abs() <= CEILING && f[1].abs() <= CEILING);
        }
    }

    #[test]
    fn loud_bass_is_turned_down_without_distortion() {
        // 40 Hz at 2x full scale: the old 1 ms limiter without a hold put
        // about 1% distortion on this; held, the gain sits still.
        let mut l = Limiter::new(48000, 2);
        let x: Vec<f32> = (0..96000).map(|i| (2.0 * (2.0 * PI * 40.0 * i as f64 / 48000.0).sin()) as f32).collect();
        let y = run(&mut l, &x);
        let tail = &y[48000..];
        let fund = amp(tail, 40.0);
        let harmonics = (2..=15).map(|k| amp(tail, 40.0 * k as f64).powi(2)).sum::<f64>().sqrt();
        assert!((fund - CEILING as f64).abs() < 0.01, "{fund}");
        assert!(20.0 * (harmonics / fund).log10() < -80.0, "THD {:.1} dB", 20.0 * (harmonics / fund).log10());
    }

    #[test]
    fn gain_comes_back_after_a_loud_burst() {
        let mut l = Limiter::new(48000, 1);
        let tone = |i: usize, a: f64| (a * (2.0 * PI * 1000.0 * i as f64 / 48000.0).sin()) as f32;
        let mut x: Vec<f32> = (0..4800).map(|i| tone(i, 2.0)).collect();
        x.extend((4800..96000).map(|i| tone(i, 0.5)));
        let y: Vec<f32> = x
            .iter()
            .map(|&s| {
                let mut f = [s];
                l.process(&mut f);
                f[0]
            })
            .collect();
        // A second after the burst, the quiet tone is back to full level.
        let last = &y[y.len() - 4800..];
        let peak = last.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!((peak - 0.5).abs() < 0.005, "{peak}");
    }
}
