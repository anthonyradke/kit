//! Render listening demos to WAV files, and time the renderer.
//!
//! cargo run --release -p spatial --example demo -- <out dir>
//!
//! Footsteps walk once around the listener, panned across 7.1 the way a game
//! engine pans (between the two nearest speakers). Each scene is rendered at a
//! few Room levels, and once as a plain stereo mix for comparison.

use std::f32::consts::PI;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use spatial::{room_level, speakers, Renderer, SpeakerTest, MASK_7_1};

const RATE: usize = 48000;
/// Azimuths of the 7.1 channels in Windows order (FL FR C LFE BL BR SL SR); `None` is the LFE.
const AZ: [Option<f32>; 8] = [Some(30.0), Some(-30.0), Some(0.0), None, Some(145.0), Some(-145.0), Some(100.0), Some(-100.0)];

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let dir = Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    let layout = speakers(8, MASK_7_1);

    let walk = footsteps_around(12.0);
    let test = {
        let mut t = SpeakerTest::new(&layout, RATE as u32);
        let mut buf = Vec::new();
        while t.fill(RATE / 10, &mut buf) {}
        buf
    };

    for (name, input) in [("footsteps-around", &walk), ("speaker-test", &test)] {
        for room in [0u8, 50, 75] {
            let mut r = Renderer::new(&layout, room_level(room));
            let mut out = Vec::new();
            r.process(input, &mut out);
            write_wav(&dir.join(format!("{name}-room{room}.wav")), &out);
        }
        write_wav(&dir.join(format!("{name}-plain-stereo.wav")), &downmix(input));
    }

    // Speed: a minute of 7.1 at the default room.
    let minute: Vec<f32> = walk.iter().cycle().take(RATE * 60 * 8).copied().collect();
    let mut r = Renderer::new(&layout, room_level(50));
    let mut out = Vec::with_capacity(RATE * 60 * 2);
    let t = Instant::now();
    r.process(&minute, &mut out);
    let secs = t.elapsed().as_secs_f64();
    println!("rendered 60 s of 7.1 in {secs:.2} s: {:.1}% of one core", secs / 60.0 * 100.0);
    println!("latency {:.1} ms", r.latency() as f64 / RATE as f64 * 1000.0);
}

/// Interleaved 7.1: a footstep every 0.4 s, walking once around in `seconds`.
fn footsteps_around(seconds: f32) -> Vec<f32> {
    let frames = (seconds * RATE as f32) as usize;
    let mut out = vec![0.0f32; frames * 8];
    let mut rng = 1u32;
    let mut noise = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    let step = (0.4 * RATE as f32) as usize;
    let (mut lp, mut lp2, mut hp_prev, mut hp) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for f in 0..frames {
        let t = (f % step) as f32 / RATE as f32;
        // A thud and a scuff: decaying noise, band limited to 150 Hz - 4 kHz.
        let env = (-t / 0.03).exp() + 0.3 * (-((t - 0.05).max(0.0)) / 0.02).exp() * (t > 0.05) as u8 as f32;
        let n = noise() * env;
        lp += (n - lp) * (2.0 * PI * 4000.0 / RATE as f32);
        lp2 += (lp - lp2) * (2.0 * PI * 4000.0 / RATE as f32);
        hp = 0.98 * (hp + lp2 - hp_prev);
        hp_prev = lp2;
        let s = hp;
        let az = 360.0 * f as f32 / frames as f32;
        for (c, g) in pan(az).iter().enumerate() {
            out[f * 8 + c] = s * g;
        }
    }
    out
}

/// Constant power gains between the two speakers either side of `az`.
fn pan(az: f32) -> [f32; 8] {
    let mut ring: Vec<(usize, f32)> = AZ.iter().enumerate().filter_map(|(c, a)| a.map(|a| (c, a.rem_euclid(360.0)))).collect();
    ring.sort_by(|a, b| a.1.total_cmp(&b.1));
    let az = az.rem_euclid(360.0);
    let mut gains = [0.0; 8];
    for i in 0..ring.len() {
        let (c0, a0) = ring[i];
        let (c1, mut a1) = ring[(i + 1) % ring.len()];
        if a1 <= a0 {
            a1 += 360.0;
        }
        let x = if az < a0 { az + 360.0 } else { az };
        if x >= a0 && x < a1 {
            let p = (x - a0) / (a1 - a0) * PI / 2.0;
            gains[c0] = p.cos();
            gains[c1] = p.sin();
        }
    }
    gains
}

/// The usual 7.1 to stereo downmix, what a game sends to a plain stereo
/// device, at the same loudness as the rendered demos.
fn downmix(input: &[f32]) -> Vec<f32> {
    let k = std::f32::consts::FRAC_1_SQRT_2;
    input
        .as_chunks::<8>()
        .0
        .iter()
        .flat_map(|c| {
            let l = c[0] + k * c[2] + k * c[4] + k * c[6];
            let r = c[1] + k * c[2] + k * c[5] + k * c[7];
            [l * 0.94, r * 0.94]
        })
        .collect()
}

fn write_wav(path: &Path, stereo: &[f32]) {
    let data: Vec<u8> = stereo.iter().flat_map(|&x| ((x.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes()).collect();
    let mut f = std::fs::File::create(path).unwrap();
    let mut header = Vec::new();
    header.extend(b"RIFF");
    header.extend((36 + data.len() as u32).to_le_bytes());
    header.extend(b"WAVEfmt ");
    header.extend(16u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(2u16.to_le_bytes());
    header.extend((RATE as u32).to_le_bytes());
    header.extend((RATE as u32 * 4).to_le_bytes());
    header.extend(4u16.to_le_bytes());
    header.extend(16u16.to_le_bytes());
    header.extend(b"data");
    header.extend((data.len() as u32).to_le_bytes());
    f.write_all(&header).unwrap();
    f.write_all(&data).unwrap();
}
