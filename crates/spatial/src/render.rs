//! Multichannel in, binaural stereo out: every channel becomes a virtual
//! speaker around the listener in a measured room.

use realfft::num_complex::Complex;

use crate::conv::{Fft, History, Partitioned, Spectrum, BINS, BLOCK};
use crate::filters::{self, Filters};
use crate::layout::{self, Speaker};
use crate::limiter::{Limiter, LOOKAHEAD};

/// The subwoofer channel, low passed and added to both ears.
const LFE_GAIN: f32 = 0.7;
const LFE_CUTOFF: f32 = 120.0;
/// Largest room level (Room slider at 100).
pub const ROOM_MAX: f32 = 4.0;

/// The Room slider (0-100) as a level: 0 is no room at all, 50 the room as
/// measured, 100 four times that (+12 dB).
pub fn room_level(slider: u8) -> f32 {
    if slider == 0 {
        0.0
    } else {
        ROOM_MAX.powf((slider.min(100) as f32 - 50.0) / 50.0)
    }
}

enum Route {
    Speaker,
    Lfe,
    Drop,
}

struct Virtual {
    input: usize,
    direction: usize,
    filter: [Partitioned; 2],
    history: History,
}

struct Tail {
    filter: [Partitioned; 2],
    history: History,
}

pub struct Renderer {
    filters: &'static Filters,
    channels: usize,
    routes: Vec<Route>,
    speakers: Vec<Virtual>,
    tail: Option<Tail>,
    room: f32,
    fft: Fft,
    /// Per input channel: the previous block and the one filling up.
    frames: Vec<Vec<f32>>,
    fill: usize,
    lfe: Lfe,
    limiter: Limiter,
    spec: Spectrum,
    sum: Spectrum,
    acc: [Spectrum; 2],
    ears: [Vec<f32>; 2],
}

impl Renderer {
    /// A renderer for a stream laid out as `layout` (see [`layout::speakers`]).
    pub fn new(layout: &[Option<Speaker>], room: f32) -> Self {
        let filters = filters::builtin();
        let mut fft = Fft::new();
        let mut speakers = Vec::new();
        let routes = layout
            .iter()
            .enumerate()
            .map(|(input, s)| match s.and_then(|s| layout::azimuth(s, layout).map(|az| (s, az))) {
                Some((_, az)) => {
                    let direction = filters.nearest(az);
                    let filter = speaker_filter(&mut fft, filters, direction, room);
                    let history = History::new(filter[0].len);
                    speakers.push(Virtual { input, direction, filter, history });
                    Route::Speaker
                }
                None if *s == Some(Speaker::Lfe) => Route::Lfe,
                None => Route::Drop,
            })
            .collect();
        let tail = make_tail(&mut fft, filters, room);
        Renderer {
            filters,
            channels: layout.len(),
            routes,
            speakers,
            tail,
            room,
            fft,
            frames: vec![vec![0.0; 2 * BLOCK]; layout.len()],
            fill: 0,
            lfe: Lfe::new(filters.rate, filters.direct_delay),
            limiter: Limiter::new(filters.rate),
            spec: vec![Complex::default(); BINS],
            sum: vec![Complex::default(); BINS],
            acc: [vec![Complex::default(); BINS], vec![Complex::default(); BINS]],
            ears: [vec![0.0; BLOCK], vec![0.0; BLOCK]],
        }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn rate(&self) -> u32 {
        self.filters.rate
    }

    /// Frames from a sample going in to its direct sound coming out, at most.
    pub fn latency(&self) -> usize {
        BLOCK + LOOKAHEAD - 1 + self.filters.direct_delay
    }

    /// Change the room level (see [`room_level`]) without interrupting the sound.
    pub fn set_room(&mut self, room: f32) {
        if room == self.room {
            return;
        }
        self.room = room;
        for v in &mut self.speakers {
            v.filter = speaker_filter(&mut self.fft, self.filters, v.direction, room);
        }
        match (&mut self.tail, room > 0.0) {
            (Some(t), true) => {
                let next = make_tail(&mut self.fft, self.filters, room).unwrap();
                t.filter = next.filter;
            }
            (None, true) => self.tail = make_tail(&mut self.fft, self.filters, room),
            (_, false) => self.tail = None,
        }
    }

    /// Render interleaved frames of [`Renderer::channels`] channels and
    /// append interleaved stereo to `out`. Audio comes out in blocks, so a
    /// short input may produce nothing yet.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for frame in input.chunks_exact(self.channels) {
            for (buf, &x) in self.frames.iter_mut().zip(frame) {
                buf[BLOCK + self.fill] = x;
            }
            self.fill += 1;
            if self.fill == BLOCK {
                self.block(out);
                self.fill = 0;
            }
        }
    }

    /// Forget all sound so far (after a gap or a glitch in the input).
    pub fn reset(&mut self) {
        for v in &mut self.speakers {
            v.history.clear();
        }
        if let Some(t) = &mut self.tail {
            t.history.clear();
        }
        for f in &mut self.frames {
            f.fill(0.0);
        }
        self.fill = 0;
        self.lfe.reset();
        self.limiter.reset();
    }

    fn block(&mut self, out: &mut Vec<f32>) {
        for a in &mut self.acc {
            a.fill(Complex::default());
        }
        self.sum.fill(Complex::default());
        for v in &mut self.speakers {
            self.fft.forward(&self.frames[v.input], &mut self.spec);
            v.history.push().copy_from_slice(&self.spec);
            if self.tail.is_some() {
                for (s, x) in self.sum.iter_mut().zip(&self.spec) {
                    *s += x;
                }
            }
            for (f, a) in v.filter.iter().zip(&mut self.acc) {
                f.apply(&v.history, a);
            }
        }
        if let Some(t) = &mut self.tail {
            t.history.push().copy_from_slice(&self.sum);
            for (f, a) in t.filter.iter().zip(&mut self.acc) {
                f.apply(&t.history, a);
            }
        }
        for (a, e) in self.acc.iter_mut().zip(&mut self.ears) {
            self.fft.inverse_tail(a, e);
        }
        let lfe = self.routes.iter().position(|r| matches!(r, Route::Lfe));
        out.reserve(2 * BLOCK);
        for n in 0..BLOCK {
            let low = lfe.map_or(0.0, |c| self.lfe.process(self.frames[c][BLOCK + n]) * LFE_GAIN);
            let [l, r] = self.limiter.process([self.ears[0][n] + low, self.ears[1][n] + low]);
            out.push(l);
            out.push(r);
        }
        for f in &mut self.frames {
            f.copy_within(BLOCK.., 0);
        }
    }
}

fn speaker_filter(fft: &mut Fft, filters: &Filters, direction: usize, room: f32) -> [Partitioned; 2] {
    let d = &filters.directions[direction];
    [0, 1].map(|ear| {
        let len = d.direct[ear].len().max(d.early[ear].len());
        let mut ir = vec![0.0f32; len];
        for (o, x) in ir.iter_mut().zip(&d.direct[ear]) {
            *o += x;
        }
        for (o, x) in ir.iter_mut().zip(&d.early[ear]) {
            *o += x * room;
        }
        Partitioned::new(fft, &ir)
    })
}

fn make_tail(fft: &mut Fft, filters: &Filters, room: f32) -> Option<Tail> {
    if room <= 0.0 {
        return None;
    }
    let filter = [0, 1].map(|ear| {
        let ir: Vec<f32> = filters.tail[ear].iter().map(|x| x * room).collect();
        Partitioned::new(fft, &ir)
    });
    let history = History::new(filter[0].len);
    Some(Tail { filter, history })
}

/// Low pass (2nd order Butterworth) and a delay matching the direct sound.
struct Lfe {
    b: [f32; 3],
    a: [f32; 2],
    x: [f32; 2],
    y: [f32; 2],
    delay: Vec<f32>,
    pos: usize,
}

impl Lfe {
    fn new(rate: u32, delay: usize) -> Self {
        let w = 2.0 * std::f32::consts::PI * LFE_CUTOFF / rate as f32;
        let alpha = w.sin() / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let c = w.cos();
        Lfe {
            b: [(1.0 - c) / 2.0 / a0, (1.0 - c) / a0, (1.0 - c) / 2.0 / a0],
            a: [-2.0 * c / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
            delay: vec![0.0; delay.max(1)],
            pos: 0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1] - self.a[0] * self.y[0] - self.a[1] * self.y[1];
        // Zero rather than denormal once the channel goes quiet: with no input
        // the rounding would otherwise hold it there, many times slower.
        let y = if y.abs() < 1e-20 { 0.0 } else { y };
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        let out = self.delay[self.pos];
        self.delay[self.pos] = y;
        self.pos = (self.pos + 1) % self.delay.len();
        out
    }

    fn reset(&mut self) {
        self.x = [0.0; 2];
        self.y = [0.0; 2];
        self.delay.fill(0.0);
    }
}

/// Pink noise bursts through each virtual speaker in turn, clockwise from
/// front left, like the Windows speaker test.
pub struct SpeakerTest {
    channels: usize,
    order: Vec<(usize, Speaker)>,
    pos: usize,
    rate: usize,
    rng: u32,
    pink: [f32; 7],
}

const BURST: f32 = 0.7;
const GAP: f32 = 0.3;

impl SpeakerTest {
    pub fn new(layout: &[Option<Speaker>], rate: u32) -> Self {
        let mut order: Vec<(usize, Speaker, i32)> = layout
            .iter()
            .enumerate()
            .filter_map(|(c, s)| s.and_then(|s| layout::azimuth(s, layout).map(|az| (c, s, az))))
            .collect();
        order.sort_by_key(|&(_, _, az)| (30 - az).rem_euclid(360));
        SpeakerTest { channels: layout.len(), order: order.into_iter().map(|(c, s, _)| (c, s)).collect(), pos: 0, rate: rate as usize, rng: 0x1234_5678, pink: [0.0; 7] }
    }

    /// The speaker playing now, `None` once the test is over.
    pub fn current(&self) -> Option<Speaker> {
        let slot = ((BURST + GAP) * self.rate as f32) as usize;
        self.order.get(self.pos / slot).map(|&(_, s)| s)
    }

    /// Append `frames` frames of the test; false once it's over.
    pub fn fill(&mut self, frames: usize, out: &mut Vec<f32>) -> bool {
        let slot = ((BURST + GAP) * self.rate as f32) as usize;
        let on = (BURST * self.rate as f32) as usize;
        let fade = self.rate / 100;
        for _ in 0..frames {
            let i = self.pos / slot;
            let t = self.pos % slot;
            let start = out.len();
            out.resize(start + self.channels, 0.0);
            if let Some(&(c, _)) = self.order.get(i) {
                if t < on {
                    let env = (t.min(on - t) as f32 / fade as f32).min(1.0);
                    out[start + c] = self.pink() * env * 0.25;
                }
            }
            self.pos += 1;
        }
        self.pos < slot * self.order.len()
    }

    fn pink(&mut self) -> f32 {
        // xorshift white noise through Paul Kellet's pink filter.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let w = self.rng as f32 / u32::MAX as f32 * 2.0 - 1.0;
        let b = &mut self.pink;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.153_852;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let p = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        p * 0.11
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{speakers, MASK_7_1};

    fn naive(x: &[f32], h: &[f32]) -> Vec<f32> {
        (0..x.len()).map(|n| h.iter().enumerate().take(n + 1).map(|(k, &hk)| hk as f64 * x[n - k] as f64).sum::<f64>() as f32).collect()
    }

    fn render(r: &mut Renderer, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        r.process(input, &mut out);
        out
    }

    fn ear(out: &[f32], e: usize) -> Vec<f32> {
        out.iter().skip(e).step_by(2).copied().collect()
    }

    /// Noise in one channel of a 7.1 stream, quiet enough not to touch the limiter.
    fn one_channel(channel: usize, frames: usize) -> Vec<f32> {
        let mut x = vec![0.0f32; frames * 8];
        let mut s = 12345u32;
        for f in 0..frames {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            x[f * 8 + channel] = ((s >> 8) as f32 / (1 << 24) as f32 - 0.5) * 0.1;
        }
        x
    }

    #[test]
    fn output_is_the_speaker_filters_convolved() {
        let f = filters::builtin();
        let layout = speakers(8, MASK_7_1);
        let room = room_level(50);
        let mut r = Renderer::new(&layout, room);
        let frames = BLOCK * 60;
        let x = one_channel(0, frames);
        let out = render(&mut r, &x);
        assert_eq!(out.len(), frames * 2);
        let mono: Vec<f32> = x.iter().step_by(8).copied().collect();
        let d = &f.directions[f.nearest(30)];
        for e in 0..2 {
            let mut ir = vec![0.0f32; f.tail[e].len()];
            for (i, v) in d.direct[e].iter().enumerate() {
                ir[i] += v;
            }
            for (i, v) in d.early[e].iter().enumerate() {
                ir[i] += v * room;
            }
            for (i, v) in f.tail[e].iter().enumerate() {
                ir[i] += v * room;
            }
            let want = naive(&mono, &ir);
            let got = ear(&out, e);
            let delay = LOOKAHEAD - 1;
            let err = (delay..frames).map(|n| (got[n] - want[n - delay]).abs()).fold(0.0, f32::max);
            assert!(err < 1e-4, "ear {e}: max error {err}");
        }
    }

    #[test]
    fn speakers_land_on_the_right_side() {
        let layout = speakers(8, MASK_7_1);
        let energy = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        // Channel order: FL FR C LFE BL BR SL SR.
        for (ch, left_louder) in [(0, true), (1, false), (4, true), (5, false), (6, true), (7, false)] {
            let mut r = Renderer::new(&layout, room_level(50));
            let out = render(&mut r, &one_channel(ch, BLOCK * 40));
            let (l, rr) = (energy(&ear(&out, 0)), energy(&ear(&out, 1)));
            assert_eq!(l > rr, left_louder, "channel {ch}: left {l} right {rr}");
        }
        let mut r = Renderer::new(&layout, room_level(50));
        let out = render(&mut r, &one_channel(2, BLOCK * 40));
        let (l, rr) = (energy(&ear(&out, 0)), energy(&ear(&out, 1)));
        assert!((l / rr).log10().abs() < 0.15, "center is off center: {l} vs {rr}");
    }

    #[test]
    fn subwoofer_reaches_both_ears_low_passed() {
        let layout = speakers(8, MASK_7_1);
        let mut r = Renderer::new(&layout, 0.0);
        let tone = |hz: f32| {
            let mut x = vec![0.0f32; BLOCK * 80 * 8];
            for (f, frame) in x.chunks_mut(8).enumerate() {
                frame[3] = (f as f32 * hz * 2.0 * std::f32::consts::PI / 48000.0).sin() * 0.5;
            }
            x
        };
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let out = render(&mut r, &tone(50.0));
        let (l, rr) = (ear(&out, 0), ear(&out, 1));
        assert_eq!(l, rr);
        let low = rms(&l[BLOCK * 10..]);
        assert!((low / (0.5 / 2f32.sqrt()) - LFE_GAIN).abs() < 0.05, "50 Hz gain {low}");
        r.reset();
        let out = render(&mut r, &tone(2000.0));
        assert!(rms(&ear(&out, 0)[BLOCK * 10..]) < low * 0.01);
    }

    #[test]
    fn silence_stays_silent_and_loud_input_doesnt_clip() {
        let layout = speakers(8, MASK_7_1);
        let mut r = Renderer::new(&layout, room_level(100));
        let out = render(&mut r, &vec![0.0; BLOCK * 8 * 20]);
        assert!(out.iter().all(|&x| x == 0.0));
        // Full scale noise in every channel at the biggest room.
        let mut x = vec![0.0f32; BLOCK * 8 * 200];
        let mut s = 99u32;
        for v in &mut x {
            s = s.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = if s >> 31 == 0 { 1.0 } else { -1.0 };
        }
        let out = render(&mut r, &x);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() <= crate::limiter::CEILING));
    }

    #[test]
    fn stereo_input_uses_the_front_pair() {
        let layout = speakers(2, 0);
        let mut r = Renderer::new(&layout, room_level(50));
        assert_eq!(r.speakers.len(), 2);
        let mut x = vec![0.0f32; BLOCK * 2 * 20];
        x[0] = 0.5;
        let out = render(&mut r, &x);
        assert!(out.iter().any(|&v| v != 0.0));
    }

    #[test]
    fn changing_the_room_takes_effect() {
        let layout = speakers(8, MASK_7_1);
        let energy = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        let mut dry = Renderer::new(&layout, 0.0);
        let mut wet = Renderer::new(&layout, 0.0);
        wet.set_room(room_level(100));
        assert!(wet.tail.is_some());
        let x = one_channel(0, BLOCK * 40);
        assert!(energy(&render(&mut wet, &x)) > 1.2 * energy(&render(&mut dry, &x)));
        wet.set_room(0.0);
        assert!(wet.tail.is_none());
    }

    #[test]
    fn speaker_test_goes_around_clockwise() {
        let layout = speakers(8, MASK_7_1);
        let mut t = SpeakerTest::new(&layout, 48000);
        let mut seen = Vec::new();
        let mut buf = Vec::new();
        while t.fill(4800, &mut buf) {
            if let Some(s) = t.current() {
                if seen.last() != Some(&s) {
                    seen.push(s);
                }
            }
        }
        use Speaker::*;
        assert_eq!(seen, [FrontLeft, FrontCenter, FrontRight, SideRight, BackRight, BackLeft, SideLeft]);
        assert_eq!(buf.len() % 8, 0);
        // Nothing ever goes to the subwoofer.
        assert!(buf.iter().skip(3).step_by(8).all(|&x| x == 0.0));
    }
}
