//! Playing a stream to one endpoint while recording another with loopback,
//! and 32-bit float WAV files for keeping the raw takes.

use std::io::{Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysaudio::{Loopback, Render};

use crate::signals::RATE;

/// Recording kept after the program ends, for the last segment's tail and
/// whatever latency the path adds.
const TAIL: Duration = Duration::from_millis(1500);
const RENDER_BUFFER: Duration = Duration::from_millis(200);

pub struct Take {
    /// The loopback recording, first two channels.
    pub frames: Vec<[f32; 2]>,
    /// The engine reported a discontinuity, or frames went missing.
    pub glitched: bool,
}

/// Play `frames` to the endpoint `play` and record what the endpoint `record`
/// outputs over the same time, from slightly before the first frame.
pub fn play_and_record(play: &str, record: &str, frames: &[[f32; 2]]) -> Result<Take> {
    let lb = Loopback::open(record).context("open loopback")?;
    let fmt = lb.format();
    if fmt.rate != RATE as u32 || fmt.channels < 2 {
        bail!("the recorded endpoint runs at {} Hz with {} channels; 48 kHz stereo is needed", fmt.rate, fmt.channels);
    }
    let out = Render::open_channels(play, 2, RENDER_BUFFER).context("open render")?;
    let flat: Vec<f32> = frames.iter().flatten().copied().collect();
    let mut raw = Vec::new();
    // Let loopback run first so the recording starts in silence.
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(300) {
        lb.read(&mut raw)?;
        std::thread::sleep(Duration::from_millis(5));
    }
    raw.clear();
    let mut glitched = false;
    let mut pos = out.write(&flat)?;
    out.start()?;
    let started = Instant::now();
    let mut done_at = None;
    let silence = vec![0.0f32; 2 * RATE / 10];
    loop {
        if pos < flat.len() {
            pos += out.write(&flat[pos..])?;
        } else {
            out.write(&silence)?;
            done_at.get_or_insert_with(Instant::now);
        }
        glitched |= lb.read(&mut raw)?;
        if done_at.is_some_and(|t| t.elapsed() > TAIL) {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let seconds = started.elapsed().as_secs_f64();
    eprintln!("  recorded {:.1} s", frames_len(&raw, fmt.channels) as f64 / RATE as f64);
    let ch = fmt.channels;
    let frames: Vec<[f32; 2]> = raw.chunks_exact(ch).map(|f| [f[0], f[1]]).collect();
    // Loopback delivers nothing while the endpoint plays nothing, which would
    // silently shorten the recording.
    let expected = seconds * RATE as f64;
    if (frames.len() as f64) < expected * 0.98 {
        glitched = true;
    }
    Ok(Take { frames, glitched })
}

fn frames_len(raw: &[f32], channels: usize) -> usize {
    raw.len() / channels
}

pub fn write_wav(path: &Path, frames: &[[f32; 2]]) -> Result<()> {
    let data = frames.len() * 8;
    let mut w = std::io::BufWriter::new(std::fs::File::create(path)?);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data as u32).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&3u16.to_le_bytes())?; // IEEE float
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&(RATE as u32).to_le_bytes())?;
    w.write_all(&(RATE as u32 * 8).to_le_bytes())?;
    w.write_all(&8u16.to_le_bytes())?;
    w.write_all(&32u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&(data as u32).to_le_bytes())?;
    for f in frames {
        w.write_all(&f[0].to_le_bytes())?;
        w.write_all(&f[1].to_le_bytes())?;
    }
    Ok(())
}

/// Read a PCM (16/24/32-bit) or float WAV as stereo at 48 kHz. Mono is
/// doubled; past two channels only the front pair is kept.
pub fn read_wav(path: &Path) -> Result<Vec<[f32; 2]>> {
    let mut b = Vec::new();
    std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?.read_to_end(&mut b)?;
    if b.len() < 12 || &b[..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        bail!("{} is not a WAV file", path.display());
    }
    let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut i = 12;
    let mut fmt = None;
    while i + 8 <= b.len() {
        let (id, len) = (&b[i..i + 4], u32_at(i + 4) as usize);
        let body = i + 8;
        if id == b"fmt " {
            let mut tag = u16_at(body);
            if tag == 0xFFFE {
                tag = u16_at(body + 24);
            }
            fmt = Some((tag, u16_at(body + 2) as usize, u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (tag, ch, rate, bits) = fmt.context("WAV data before its format")?;
            if rate != RATE as u32 {
                bail!("{} is {rate} Hz; convert it to 48 kHz first (ffmpeg -i in -ar 48000 out.wav)", path.display());
            }
            let data = &b[body..(body + len).min(b.len())];
            let bytes = bits as usize / 8;
            let sample = |s: &[u8]| -> f32 {
                match (tag, bits) {
                    (3, 32) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                    (1, 16) => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                    (1, 24) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
                    (1, 32) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                    _ => 0.0,
                }
            };
            if !matches!((tag, bits), (3, 32) | (1, 16) | (1, 24) | (1, 32)) {
                bail!("unsupported WAV sample format (tag {tag}, {bits} bit)");
            }
            return Ok(data
                .chunks_exact(bytes * ch)
                .map(|f| {
                    let l = sample(&f[..bytes]);
                    let r = if ch > 1 { sample(&f[bytes..2 * bytes]) } else { l };
                    [l, r]
                })
                .collect());
        }
        i = body + len + (len & 1);
    }
    bail!("{} has no audio data", path.display())
}
