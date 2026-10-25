//! RNNoise (nnnoiseless) wrapped for arbitrary buffer sizes and channel counts.
//!
//! RNNoise works on fixed 480-sample frames at 48 kHz. The audio engine hands
//! us whatever its period is, so each channel keeps a small FIFO. When the
//! period is exactly 480 frames (the usual 10 ms engine period) nothing waits
//! in the FIFO and the only delay is RNNoise's own frame.

use std::collections::VecDeque;

use nnnoiseless::DenoiseState;

pub const FRAME: usize = DenoiseState::FRAME_SIZE;
/// RNNoise's input and output are scaled like 16-bit PCM.
const SCALE: f32 = 32768.0;
/// Room reserved up front so the real-time path never reallocates.
const FIFO_CAPACITY: usize = 16 * FRAME;

struct Channel {
    state: Box<DenoiseState<'static>>,
    input: VecDeque<f32>,
    output: VecDeque<f32>,
    /// The previous input frame: RNNoise output lags its input by one frame,
    /// so this is the dry signal that lines up with the current output.
    dry: [f32; FRAME],
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
    /// The first RNNoise output has fade-in artifacts; the dry signal is used instead.
    primed: bool,
}

pub struct Denoiser {
    channels: Vec<Channel>,
}

impl Denoiser {
    pub fn new(channels: usize) -> Self {
        let channels = (0..channels.max(1))
            .map(|_| Channel {
                state: DenoiseState::new(),
                input: VecDeque::with_capacity(FIFO_CAPACITY),
                output: VecDeque::with_capacity(FIFO_CAPACITY),
                dry: [0.0; FRAME],
                frame_in: [0.0; FRAME],
                frame_out: [0.0; FRAME],
                primed: false,
            })
            .collect();
        Denoiser { channels }
    }

    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    /// Process `frames` interleaved frames from `input` into `output`.
    /// `strength` 0..=1 blends between the dry and the denoised signal.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize, strength: f32) {
        let n = self.channels.len();
        let strength = strength.clamp(0.0, 1.0);
        for (c, ch) in self.channels.iter_mut().enumerate() {
            if ch.input.len() + frames > FIFO_CAPACITY {
                // The engine never sends more than a few periods at once; if it
                // does, drop the backlog rather than grow on the audio thread.
                ch.input.clear();
            }
            ch.input.extend(input.iter().skip(c).step_by(n).take(frames));
            while ch.input.len() >= FRAME {
                for (dst, src) in ch.frame_in.iter_mut().zip(ch.input.drain(..FRAME)) {
                    *dst = src * SCALE;
                }
                ch.state.process_frame(&mut ch.frame_out, &ch.frame_in);
                for i in 0..FRAME {
                    let wet = if ch.primed { ch.frame_out[i] } else { ch.dry[i] };
                    let s = (wet * strength + ch.dry[i] * (1.0 - strength)) / SCALE;
                    if ch.output.len() < FIFO_CAPACITY {
                        ch.output.push_back(s);
                    }
                }
                ch.dry = ch.frame_in;
                ch.primed = true;
            }
            // Pad the front once so later calls always find enough samples.
            while ch.output.len() < frames {
                ch.output.push_front(0.0);
            }
            for (i, s) in ch.output.drain(..frames).enumerate() {
                output[i * n + c] = s;
            }
        }
    }

    pub fn reset(&mut self) {
        let n = self.channels.len();
        *self = Denoiser::new(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, amplitude: f32) -> Vec<f32> {
        let mut x: u32 = 1;
        (0..len)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                ((x >> 8) as f32 / 16777216.0 - 0.5) * amplitude
            })
            .collect()
    }

    fn run(input: &[f32], strength: f32) -> Vec<f32> {
        let mut d = Denoiser::new(1);
        let mut out = vec![0.0; input.len()];
        for (i, o) in input.chunks(FRAME).zip(out.chunks_mut(FRAME)) {
            d.process(i, o, FRAME, strength);
        }
        out
    }

    /// Lag that best maps `input` onto `out`, and the worst error at that lag.
    fn best_lag(input: &[f32], out: &[f32]) -> (usize, f32) {
        (0..FRAME * 3)
            .map(|lag| {
                let err = (FRAME * 4..FRAME * 9).map(|i| (out[i] - input[i - lag]).abs()).fold(0.0, f32::max);
                (lag, err)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap()
    }

    /// With strength 0 the output is the input delayed by exactly one frame.
    #[test]
    fn dry_path_is_a_one_frame_delay() {
        let input = noise(FRAME * 10, 0.5);
        assert_eq!(best_lag(&input, &run(&input, 0.0)), (FRAME, 0.0));
    }

    /// RNNoise's own delay matches the dry path, so partial strength doesn't comb filter.
    /// (A signal under its silence threshold passes ungated, apart from its DC filter.)
    #[test]
    fn wet_path_matches_dry_delay() {
        let input = noise(FRAME * 10, 1e-8);
        assert_eq!(best_lag(&input, &run(&input, 1.0)).0, FRAME);
    }

    #[test]
    fn odd_buffer_sizes_and_stereo() {
        let mut d = Denoiser::new(2);
        let input = noise(441 * 2, 0.5);
        let mut out = vec![0.0; 441 * 2];
        for _ in 0..50 {
            d.process(&input, &mut out, 441, 1.0);
        }
        assert!(out.iter().all(|s| s.is_finite()));
    }
}
