//! Mic noise suppression with DeepFilterNet 3, for any buffer size and
//! channel count.
//!
//! DeepFilterNet replaced RNNoise. RNNoise only sets one gain per frequency
//! band, so while you talk the noise between the harmonics of your voice
//! comes through almost untouched, then drops 20 dB in the pauses: the
//! background "breathes" with every word, which sounds like wind. DeepFilterNet
//! also filters inside the harmonics; on the same test recordings it removes
//! 1-11 dB more noise during speech and 10-20 dB more in the pauses, with the
//! voice itself just as intact.
//!
//! Inference allocates and takes 5-10% of a core, so it runs on its own
//! thread, never on audiodg's real-time thread. The real-time side only moves
//! samples through two lock-free rings and wakes the worker. It's one mic, so
//! the channels are mixed to mono and the result goes to every channel.
//!
//! An 80 Hz high-pass runs first. A voice has nothing below it, and it's where
//! wind, breath pops, desk thumps and fan rumble sit.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread::Thread;
use std::time::Duration;

use rtrb::{Consumer, Producer, RingBuffer};

/// The only rate the model runs at.
pub const SAMPLE_RATE: f32 = 48000.0;
/// DeepFilterNet's hop: it takes and returns 10 ms at a time.
pub const FRAME: usize = 480;
/// How far the model's output lags its input: one window plus two frames of lookahead.
const MODEL_DELAY: usize = 3 * FRAME;
/// Output kept in reserve so the worker can run up to a frame late without a dropout.
const CUSHION: usize = FRAME;
/// Samples the output lags the input once running, for a 10 ms engine period.
pub const LATENCY: usize = MODEL_DELAY + FRAME + CUSHION;
/// Room in each ring: far more than a few periods, so the real-time side never waits.
const RING: usize = 32 * FRAME;
/// How long the mic stays silent waiting for the model before passing through.
const LOAD_TIMEOUT: usize = 5 * SAMPLE_RATE as usize;
const HIGH_PASS_HZ: f64 = 80.0;

/// Second-order Butterworth high-pass (RBJ cookbook), transposed direct
/// form II in f64 so the poles this close to DC stay accurate.
#[derive(Clone, Copy)]
struct HighPass {
    b: [f64; 3],
    a: [f64; 2],
    s: [f64; 2],
}

impl HighPass {
    fn new(freq: f64) -> Self {
        let w = 2.0 * std::f64::consts::PI * freq / SAMPLE_RATE as f64;
        let alpha = w.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let c = w.cos();
        HighPass {
            b: [(1.0 + c) / 2.0 / a0, -(1.0 + c) / a0, (1.0 + c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            s: [0.0; 2],
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b[0] * x + self.s[0];
        self.s[0] = self.b[1] * x - self.a[0] * y + self.s[1];
        self.s[1] = self.b[2] * x - self.a[1] * y;
        y as f32
    }
}

/// How much noise the model may remove, in dB, for a strength of 0..=1.
/// Strength is the share of the noise removed, so 0.9 leaves a tenth of it
/// (20 dB). At full strength there's no limit.
pub fn attenuation_db(strength: f32) -> f32 {
    let strength = strength.clamp(0.0, 1.0);
    if strength >= 0.999 {
        100.0
    } else {
        // Never quite 0: the model hands the input back undelayed at 0 dB.
        (-20.0 * (1.0 - strength).log10()).max(0.1)
    }
}

/// State shared by the real-time side and the worker.
struct Shared {
    /// `attenuation_db` for the worker, as f32 bits.
    atten_db: AtomicU32,
    /// The model is loaded and the worker is taking input.
    ready: AtomicBool,
    /// The model failed to load or panicked; pass the mic through.
    failed: AtomicBool,
    stop: AtomicBool,
    /// Periods that found too little output (the worker fell behind).
    underruns: AtomicU32,
}

pub struct Denoiser {
    channels: usize,
    high_pass: HighPass,
    to_worker: Producer<f32>,
    from_worker: Consumer<f32>,
    shared: Arc<Shared>,
    worker: Thread,
    /// Output has started; from here every period takes its samples from the ring.
    running: bool,
    /// Frames of silence handed out while the model loads.
    waited: usize,
    /// Samples sent to the worker, and taken back (output or skipped).
    sent: u64,
    taken: u64,
    /// Output samples still to throw away as they arrive: ones a dropout
    /// already filled with silence, or from before a restart. Keeps the
    /// latency from growing.
    skip: u64,
}

impl Denoiser {
    /// Start the worker, which loads the model in the background (a few
    /// hundred ms; the mic is silent meanwhile). Spawns a thread and
    /// allocates: not for the real-time thread. `log` is called from the
    /// worker only.
    pub fn new(channels: usize, log: fn(&str)) -> std::io::Result<Self> {
        let (to_worker, worker_in) = RingBuffer::new(RING);
        let (worker_out, from_worker) = RingBuffer::new(RING);
        let shared = Arc::new(Shared {
            atten_db: AtomicU32::new(attenuation_db(0.9).to_bits()),
            ready: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            underruns: AtomicU32::new(0),
        });
        let s = shared.clone();
        let worker = std::thread::Builder::new()
            .name("kit-denoise".into())
            .spawn(move || {
                let s2 = s.clone();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&s2, worker_in, worker_out, log)));
                if r.is_err() {
                    log("denoise worker panicked; passing the mic through");
                    s.failed.store(true, Ordering::Release);
                }
            })?
            .thread()
            .clone();
        Ok(Denoiser { channels: channels.max(1), high_pass: HighPass::new(HIGH_PASS_HZ), to_worker, from_worker, shared, worker, running: false, waited: 0, sent: 0, taken: 0, skip: 0 })
    }

    pub fn underruns(&self) -> u32 {
        self.shared.underruns.load(Ordering::Relaxed)
    }

    /// Process `frames` interleaved frames from `input` into `output`.
    /// `strength` 0..=1 limits how much noise is removed. Returns false when
    /// the caller should pass the input through instead: the model failed,
    /// or is taking far too long to load. While it loads normally the output
    /// is silence; passing the mic through until then would make the switch
    /// to the delayed, denoised mic repeat a few ms of speech. Real-time
    /// safe: no allocation, no blocking.
    pub fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize, strength: f32) -> bool {
        if self.shared.failed.load(Ordering::Acquire) {
            return false;
        }
        if !self.shared.ready.load(Ordering::Acquire) {
            if self.waited >= LOAD_TIMEOUT {
                return false;
            }
            self.waited += frames;
            output[..frames * self.channels].fill(0.0);
            return true;
        }
        // Counted before this period's input goes in, so how much is queued
        // (and so the latency) doesn't depend on how fast the worker wakes.
        let mut available = self.from_worker.slots();
        if self.skip > 0 {
            let n = (self.skip as usize).min(available);
            self.discard(n);
            self.skip -= n as u64;
            available -= n;
        }
        self.shared.atten_db.store(attenuation_db(strength).to_bits(), Ordering::Relaxed);
        let n = self.channels;
        let scale = 1.0 / n as f32;
        let mut sent_now = 0;
        if let Ok(mut chunk) = self.to_worker.write_chunk_uninit(frames.min(self.to_worker.slots())) {
            let (a, b) = chunk.as_mut_slices();
            for (i, slot) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                let mono: f32 = input[i * n..i * n + n].iter().sum::<f32>() * scale;
                slot.write(self.high_pass.run(mono));
                sent_now += 1;
            }
            // SAFETY: every slot of both slices was just written.
            unsafe { chunk.commit_all() };
            self.sent += sent_now;
        }
        self.worker.unpark();

        if !self.running {
            // Wait for the first output plus the cushion; silence until then.
            if available < frames + CUSHION {
                output[..frames * n].fill(0.0);
                return true;
            }
            self.running = true;
        }
        let take = frames.min(available);
        if let Ok(chunk) = self.from_worker.read_chunk(take) {
            let (a, b) = chunk.as_slices();
            for (i, &s) in a.iter().chain(b.iter()).enumerate() {
                output[i * n..i * n + n].fill(s);
            }
            chunk.commit_all();
            self.taken += take as u64;
        }
        output[take * n..frames * n].fill(0.0);
        if take < frames {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
            // Skip the samples this dropout replaced once they arrive, but only
            // ones that were sent before this period and aren't already being
            // skipped: input the full ring refused never comes back.
            let due = (self.sent - self.taken).saturating_sub(sent_now + self.skip);
            self.skip += ((frames - take) as u64).min(due);
        }
        true
    }

    /// Drop everything already sent to the worker and wait for the cushion
    /// again, e.g. after suppression was off for a while. Real-time safe.
    pub fn restart(&mut self) {
        self.skip = self.sent - self.taken;
        self.running = false;
    }

    fn discard(&mut self, n: usize) {
        if let Ok(chunk) = self.from_worker.read_chunk(n) {
            chunk.commit_all();
            self.taken += n as u64;
        }
    }
}

impl Drop for Denoiser {
    /// The worker exits on its own; it isn't joined, so dropping never waits
    /// on a model that's still loading.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.worker.unpark();
    }
}

fn work(shared: &Shared, mut input: Consumer<f32>, mut output: Producer<f32>, log: fn(&str)) {
    raise_priority();
    let started = std::time::Instant::now();
    // The thresholds of DeepFilterNet's own command-line tool. The library's
    // defaults (-10, 30, 20 dB) zero out quiet syllables and skip the
    // harmonic filter on clear speech; on the test recordings they cost
    // 5-8 dB of the voice between 250 Hz and 2 kHz.
    let rp = df::tract::RuntimeParams::default_with_ch(1)
        .with_atten_lim(f32::from_bits(shared.atten_db.load(Ordering::Relaxed)))
        .with_thresholds(-15.0, 35.0, 35.0);
    // The model DeepFilterNet ships, built into the DLL (`default-model`).
    let mut model = match df::tract::DfTract::new(df::tract::DfParams::default(), &rp) {
        Ok(m) => m,
        Err(e) => {
            log(&format!("denoise model failed to load: {e}; passing the mic through"));
            shared.failed.store(true, Ordering::Release);
            return;
        }
    };
    if model.hop_size != FRAME || model.sr != SAMPLE_RATE as usize {
        log(&format!("denoise model has hop {} at {} Hz, expected {FRAME} at 48000", model.hop_size, model.sr));
        shared.failed.store(true, Ordering::Release);
        return;
    }
    log(&format!("denoise model loaded in {} ms", started.elapsed().as_millis()));
    shared.ready.store(true, Ordering::Release);

    let mut atten = f32::NAN;
    let mut noisy = ndarray::Array2::<f32>::zeros((1, FRAME));
    let mut enhanced = ndarray::Array2::<f32>::zeros((1, FRAME));
    while !shared.stop.load(Ordering::Acquire) {
        while input.slots() >= FRAME {
            let Ok(chunk) = input.read_chunk(FRAME) else { break };
            let (a, b) = chunk.as_slices();
            for (dst, &src) in noisy.iter_mut().zip(a.iter().chain(b.iter())) {
                *dst = src;
            }
            chunk.commit_all();
            let want = f32::from_bits(shared.atten_db.load(Ordering::Relaxed));
            if want != atten {
                model.set_atten_lim(want);
                atten = want;
            }
            if let Err(e) = model.process(noisy.view(), enhanced.view_mut()) {
                log(&format!("denoise failed: {e}; passing the mic through"));
                shared.failed.store(true, Ordering::Release);
                return;
            }
            // A full ring means the real-time side stopped reading; drop rather than wait.
            if let Ok(mut chunk) = output.write_chunk_uninit(FRAME.min(output.slots())) {
                let (a, b) = chunk.as_mut_slices();
                for (slot, &s) in a.iter_mut().chain(b.iter_mut()).zip(enhanced.iter()) {
                    slot.write(s);
                }
                // SAFETY: both slices together are at most FRAME long, and every slot was written.
                unsafe { chunk.commit_all() };
            }
        }
        // Woken by every period; the timeout only bounds how long a stop can go unseen.
        std::thread::park_timeout(Duration::from_millis(50));
    }
}

/// Run the worker at audio priority (MMCSS), like the engine's own threads.
#[cfg(windows)]
fn raise_priority() {
    use windows::core::w;
    use windows::Win32::System::Threading::{
        AvSetMmThreadCharacteristicsW, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
    };
    let mut index = 0u32;
    if unsafe { AvSetMmThreadCharacteristicsW(w!("Audio"), &mut index) }.is_err() {
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
    }
}

#[cfg(not(windows))]
fn raise_priority() {}

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

    fn quiet(_: &str) {}

    /// Feed `input` in engine periods at the engine's pace and return the output.
    fn run(input: &[f32], channels: usize, period: usize, strength: f32) -> Vec<f32> {
        let mut d = Denoiser::new(channels, quiet).unwrap();
        let start = std::time::Instant::now();
        while !d.shared.ready.load(Ordering::Acquire) {
            assert!(!d.shared.failed.load(Ordering::Acquire), "model failed to load");
            assert!(start.elapsed() < Duration::from_secs(30), "model took too long to load");
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut out = vec![0.0; input.len()];
        for (i, o) in input.chunks(period * channels).zip(out.chunks_mut(period * channels)) {
            assert!(d.process(i, o, i.len() / channels, strength));
            std::thread::sleep(Duration::from_secs_f32(period as f32 / SAMPLE_RATE));
        }
        assert_eq!(d.underruns(), 0);
        out
    }

    /// A late worker costs a dropout but not lasting latency, and a restart
    /// doesn't leave old audio queued.
    #[test]
    fn latency_recovers_after_a_dropout_and_a_restart() {
        let mut d = Denoiser::new(1, quiet).unwrap();
        // The model takes a few hundred ms to load; until then the mic is silent.
        let mut out = vec![1.0; FRAME];
        assert!(d.process(&noise(FRAME, 0.2), &mut out, FRAME, 1.0));
        assert!(out.iter().all(|&s| s == 0.0));
        let start = std::time::Instant::now();
        while !d.shared.ready.load(Ordering::Acquire) {
            assert!(!d.shared.failed.load(Ordering::Acquire), "model failed to load");
            assert!(start.elapsed() < Duration::from_secs(30), "model took too long to load");
            std::thread::sleep(Duration::from_millis(5));
        }
        let input = noise(FRAME, 0.2);
        let mut out = vec![0.0; FRAME];
        let mut step = |d: &mut Denoiser, wait: u64| {
            assert!(d.process(&input, &mut out, FRAME, 1.0));
            std::thread::sleep(Duration::from_millis(wait));
            d.from_worker.slots()
        };
        for _ in 0..20 {
            step(&mut d, 3);
        }
        let steady = step(&mut d, 3);
        // Two periods back to back: the second finds the worker hasn't caught up.
        step(&mut d, 0);
        step(&mut d, 0);
        for _ in 0..20 {
            step(&mut d, 3);
        }
        assert!(d.underruns() > 0);
        assert_eq!(step(&mut d, 3), steady);
        d.restart();
        for _ in 0..20 {
            step(&mut d, 3);
        }
        assert_eq!(step(&mut d, 3), steady);
    }

    #[test]
    fn strength_maps_to_an_attenuation_limit() {
        assert_eq!(attenuation_db(1.0), 100.0);
        assert!((attenuation_db(0.9) - 20.0).abs() < 1e-3);
        assert!((attenuation_db(0.5) - 6.02).abs() < 1e-2);
        assert_eq!(attenuation_db(0.0), 0.1);
    }

    #[test]
    fn high_pass_keeps_the_voice_and_drops_rumble() {
        let gain = |freq: f32| {
            let mut hp = HighPass::new(HIGH_PASS_HZ);
            let x: Vec<f32> = (0..48000).map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE).sin()).collect();
            let y: Vec<f32> = x.iter().map(|&s| hp.run(s)).collect();
            let rms = |v: &[f32]| (v[24000..].iter().map(|s| s * s).sum::<f32>() / 24000.0).sqrt();
            20.0 * (rms(&y) / rms(&x)).log10()
        };
        assert!(gain(20.0) < -20.0);
        assert!(gain(150.0) > -1.0);
        assert!(gain(1000.0).abs() < 0.01);
    }

    /// At zero strength the output is the high-passed input, delayed by `LATENCY`.
    #[test]
    fn passthrough_strength_is_a_fixed_delay() {
        let input = noise(FRAME * 200, 0.2);
        let out = run(&input, 1, FRAME, 0.0);
        let mut hp = HighPass::new(HIGH_PASS_HZ);
        let filtered: Vec<f32> = input.iter().map(|&s| hp.run(s)).collect();
        let err = |lag: usize| (FRAME * 100..FRAME * 190).map(|i| (out[i] - filtered[i - lag]).abs()).fold(0.0f32, f32::max);
        let best = (0..FRAME * 8).min_by(|&a, &b| err(a).total_cmp(&err(b))).unwrap();
        assert_eq!(best, LATENCY);
        // An attenuation limit of 0.1 dB still removes about 1% of the noise.
        assert!(err(best) < 0.02, "worst error {}", err(best));
    }

    /// Noise alone is pushed down by about the attenuation limit.
    #[test]
    fn removes_steady_noise() {
        let input = noise(FRAME * 200, 0.05);
        let out = run(&input, 1, FRAME, 0.9);
        let rms = |v: &[f32]| (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        let db = 20.0 * (rms(&out[FRAME * 100..]) / rms(&input[FRAME * 100..])).log10();
        assert!(db < -15.0, "noise only {db:.1} dB down");
    }

    #[test]
    fn odd_periods_and_stereo() {
        let input = noise(441 * 2 * 150, 0.2);
        let out = run(&input, 2, 441, 1.0);
        assert!(out.iter().all(|s| s.is_finite()));
        assert!(out.chunks(2).all(|f| f[0] == f[1]));
    }
}
