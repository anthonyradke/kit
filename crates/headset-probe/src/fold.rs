//! `headset-probe fold <dir>`: how Sonar's Game device folds a multichannel
//! game mix (7.1) down to the headset's stereo.
//!
//! `capture` plays stereo, which only ever reaches the Game device's front
//! pair. Games see a 7.1 device and fill every channel, so this plays a sweep
//! on each of the device's channels in turn, in the device's own speaker
//! layout, and records the Nova 5 with loopback (muted meanwhile, as in
//! `capture`). For each input channel it reports the response into the
//! headset's left and right, the delay against front left and the polarity.
//! Run it with Sonar's EQ flat, or the EQ shows up on every channel.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::analyze::{best_lag, db, grid, ir_response, linear_ir, raw_ir, rms};
use crate::capture::{endpoint_named, nova, volume_line, Muted};
use crate::measure;
use crate::signals::{db_to_amp, sweep, SWEEP_LOW_DB, RATE};

const GAME: &str = "SteelSeries Sonar - Gaming";
const LEAD: usize = RATE;
const GAP: usize = RATE;
const TABLE_FREQS: [f64; 12] = [20.0, 30.0, 45.0, 60.0, 80.0, 120.0, 200.0, 500.0, 1000.0, 3000.0, 8000.0, 16000.0];
/// Below this a path counts as not connected.
const SILENT_DB: f64 = -60.0;

/// Speaker names for the bits of `dwChannelMask`, lowest first.
const SPEAKERS: [&str; 18] =
    ["FL", "FR", "FC", "LFE", "BL", "BR", "FLC", "FRC", "BC", "SL", "SR", "TC", "TFL", "TFC", "TFR", "TBL", "TBC", "TBR"];

/// The speaker of each channel, in interleaved order.
fn names(channels: usize, mask: u32) -> Vec<String> {
    let mut out: Vec<String> = (0..SPEAKERS.len()).filter(|b| mask & (1 << b) != 0).map(|b| SPEAKERS[b].to_string()).collect();
    out.truncate(channels);
    while out.len() < channels {
        out.push(format!("ch{}", out.len() + 1));
    }
    out
}

/// The program: a lead of silence, then the sweep on each channel in turn.
/// Returns the interleaved samples and where each channel's sweep starts.
fn program(channels: usize) -> (Vec<f32>, Vec<usize>, usize) {
    let s = sweep(db_to_amp(SWEEP_LOW_DB));
    let mut out = vec![0.0f32; LEAD * channels];
    let mut starts = Vec::new();
    for ch in 0..channels {
        starts.push(out.len() / channels);
        for &v in &s {
            let mut frame = vec![0.0f32; channels];
            frame[ch] = v;
            out.extend(frame);
        }
        out.extend(std::iter::repeat_n(0.0, GAP * channels));
    }
    (out, starts, s.len())
}

pub fn run(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let nova = nova()?;
    let game = endpoint_named(GAME)?;
    let mix = sysaudio::mix_format(&game.id).context("read the Game device's format")?;
    eprintln!("{}: {} channels, mask 0x{:X}, {} Hz", game.name, mix.channels, mix.mask, mix.rate);
    if mix.channels < 2 {
        bail!("the Game device has {} channel(s)", mix.channels);
    }
    // A software volume below 100% on the Game device shifts every number by the same amount.
    let volumes = format!("{}; {}", volume_line(&game), volume_line(&nova));
    eprintln!("{volumes}");
    let (samples, _, _) = program(mix.channels);
    eprintln!("playing {:.0} s, one sweep per channel ...", samples.len() as f64 / mix.channels as f64 / RATE as f64);
    let muted = Muted::new(&nova.id)?;
    let take = measure::play_and_record_layout(&game.id, &nova.id, &samples, mix.channels as u16, mix.mask);
    drop(muted);
    let take = take?;
    if take.glitched {
        eprintln!("  warning: the recording had a gap; rerun if the results look off");
    }
    measure::write_wav(&dir.join("fold.wav"), &take.frames)?;
    std::fs::write(
        dir.join("fold.txt"),
        format!("{} {}\n{} ({} Hz)\n{}\n{volumes}\n", mix.channels, mix.mask, game.name, mix.rate, nova.name),
    )?;
    analyze_dir(dir)
}

pub fn analyze_dir(dir: &Path) -> Result<()> {
    let meta = std::fs::read_to_string(dir.join("fold.txt")).context("no fold.txt: run `fold` first")?;
    let mut first = meta.lines().next().unwrap_or_default().split_whitespace().map(|w| w.parse::<u32>());
    let (Some(Ok(channels)), Some(Ok(mask))) = (first.next(), first.next()) else { bail!("fold.txt is malformed") };
    let channels = channels as usize;
    let rec = measure::read_wav(&dir.join("fold.wav"))?;
    let (samples, starts, len) = program(channels);
    let names = names(channels, mask);

    // Coarse: the whole program (summed) against the recording; the search
    // stays well inside one segment's length, so it can't slip a segment.
    let frames = samples.len() / channels;
    let played: Vec<f64> = (0..frames).map(|i| samples[i * channels..(i + 1) * channels].iter().map(|&v| v as f64).sum()).collect();
    let recorded: Vec<f64> = rec.iter().map(|f| (f[0] + f[1]) as f64).collect();
    if rms(&recorded) < 1e-7 {
        bail!("the recording is silent: did the Game device play to the Nova 5?");
    }
    let coarse = best_lag(&played, &recorded, 0, 3 * RATE);
    // Exact: the peak of front left's response into the left ear.
    let x0: Vec<f64> = sweep(db_to_amp(SWEEP_LOW_DB)).iter().map(|&v| v as f64).collect();
    let window = RATE as isize / 20;
    let chan = |from: isize, n: usize, ch: usize| -> Vec<f64> {
        (0..n as isize).map(|i| usize::try_from(from + i).ok().and_then(|j| rec.get(j)).map_or(0.0, |f| f[ch] as f64)).collect()
    };
    let ir0 = raw_ir(&x0, &chan(starts[0] as isize + coarse - window, len + RATE / 2, 0));
    let k = (0..2 * window as usize).max_by(|&a, &b| ir0[a].abs().total_cmp(&ir0[b].abs())).unwrap_or(window as usize);
    let delay = coarse - window + k as isize;

    let noise = db(rms(&chan(delay, LEAD - RATE / 10, 0)));
    let mut out = String::new();
    let lines: Vec<&str> = meta.lines().collect();
    writeln!(out, "{}", lines.get(1..3).map(|l| l.join(" -> ")).unwrap_or_default())?;
    if let Some(v) = lines.get(3) {
        writeln!(out, "{v}")?;
    }
    writeln!(out, "layout: {}  (mask 0x{mask:X})", names.join(" "))?;
    writeln!(out, "path delay {:.1} ms, noise floor {noise:.1} dBFS", delay as f64 / RATE as f64 * 1000.0)?;
    writeln!(out, "\nResponse in dB (0 = passed at full level), delay vs FL->L, polarity:")?;
    write!(out, "{:>9}", "")?;
    for f in TABLE_FREQS {
        write!(out, "{:>7}", if f >= 1000.0 { format!("{}k", f / 1000.0) } else { format!("{f}") })?;
    }
    writeln!(out, "{:>9} {:>4}", "delay", "pol")?;
    let g = grid();
    let mut csv_cols = Vec::new();
    for (ch, name) in names.iter().enumerate() {
        for (o, ear) in ["L", "R"].iter().enumerate() {
            let y = chan(starts[ch] as isize + delay, len + RATE / 2, o);
            let ir = linear_ir(&x0, &y);
            let resp = ir_response(&ir);
            let label = format!("{name}->{ear}");
            write!(out, "{label:>9}")?;
            if resp.iter().cloned().fold(f64::MIN, f64::max) < SILENT_DB {
                writeln!(out, "  (nothing)")?;
            } else {
                for f in TABLE_FREQS {
                    let i = g.iter().position(|&x| x >= f * 0.999).unwrap_or(g.len() - 1);
                    write!(out, "{:>+7.1}", resp[i])?;
                }
                // `linear_ir` starts 512 frames early.
                let (pk, v) = ir.iter().enumerate().fold((0, 0.0f64), |m, (i, &v)| if v.abs() > m.1.abs() { (i, v) } else { m });
                writeln!(out, "{:>7.2}ms {:>4}", (pk as f64 - 512.0) / RATE as f64 * 1000.0, if v < 0.0 { "-" } else { "+" })?;
            }
            csv_cols.push((label, resp));
        }
    }
    let mut csv = String::from("freq");
    for (label, _) in &csv_cols {
        write!(csv, ",{label}")?;
    }
    csv.push('\n');
    for (i, f) in g.iter().enumerate() {
        write!(csv, "{f:.2}")?;
        for (_, r) in &csv_cols {
            write!(csv, ",{:.3}", r[i])?;
        }
        csv.push('\n');
    }
    std::fs::write(dir.join("fold.csv"), csv)?;
    std::fs::write(dir.join("fold-report.txt"), &out)?;
    print!("{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_follow_the_mask() {
        assert_eq!(names(8, sysaudio::MASK_7_1), ["FL", "FR", "FC", "LFE", "BL", "BR", "SL", "SR"]);
        assert_eq!(names(2, 0), ["ch1", "ch2"]);
    }

    #[test]
    fn program_puts_each_sweep_on_its_own_channel() {
        let (s, starts, len) = program(3);
        for (ch, &start) in starts.iter().enumerate() {
            let energy: Vec<f64> = (0..3).map(|c| (start..start + len).map(|i| (s[i * 3 + c] as f64).powi(2)).sum()).collect();
            for (c, e) in energy.iter().enumerate() {
                assert_eq!(*e > 0.0, c == ch, "segment {ch}, channel {c}");
            }
        }
    }
}
