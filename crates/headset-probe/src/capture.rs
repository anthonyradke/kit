//! `headset-probe capture`: measure what Sonar does to sound on its way to
//! the headset, by playing the test program into Sonar's virtual devices and
//! recording the real Nova 5 endpoint with loopback. `direct` plays straight
//! to the Nova 5 as a control: it must come back unchanged.
//!
//! Loopback records the digital stream the endpoint is sent, so this
//! measures software processing only. The headset's own EQ runs after USB
//! and never shows up here.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sysaudio::Flow;

use crate::analyze::{self, at_freq, grid, Report};
use crate::fit;
use crate::measure;
use crate::model;
use crate::signals::Program;

pub const PATHS: [(&str, &str); 3] =
    [("game", "SteelSeries Sonar - Gaming"), ("chat", "SteelSeries Sonar - Chat"), ("direct", "")];
const TABLE_FREQS: [f64; 29] = [
    20.0, 30.0, 40.0, 55.0, 63.0, 80.0, 100.0, 150.0, 200.0, 300.0, 400.0, 500.0, 600.0, 700.0, 800.0, 1000.0, 1500.0,
    2000.0, 3000.0, 4000.0, 5000.0, 6000.0, 8000.0, 10000.0, 12000.0, 14000.0, 16000.0, 18000.0, 20000.0,
];

pub fn nova() -> Result<sysaudio::Endpoint> {
    sysaudio::find_arctis(Flow::Output)?.context("Nova 5 headphones not found (is the dongle plugged in?)")
}

/// An output endpoint by the start of its name.
pub fn endpoint_named(prefix: &str) -> Result<sysaudio::Endpoint> {
    sysaudio::endpoints(Flow::Output)?
        .into_iter()
        .find(|e| e.name.starts_with(prefix))
        .with_context(|| format!("no output device named \"{prefix}...\" (is Sonar running?)"))
}

/// Mutes an endpoint, and puts its mute back as it was when dropped, also
/// when a panic unwinds past it (a muted headset is easy to miss).
pub struct Muted {
    id: String,
    was: bool,
}

impl Muted {
    pub fn new(id: &str) -> Result<Self> {
        let was = sysaudio::endpoint_volume(id).map(|v| v.1).unwrap_or(false);
        sysaudio::set_endpoint_mute(id, true)?;
        Ok(Muted { id: id.to_string(), was })
    }
}

impl Drop for Muted {
    fn drop(&mut self) {
        let _ = sysaudio::set_endpoint_mute(&self.id, self.was);
    }
}

fn volume_line(e: &sysaudio::Endpoint) -> String {
    match sysaudio::endpoint_volume(&e.id) {
        Ok((db, mute)) => format!("{}: volume {db:.1} dB{}", e.name, if mute { ", MUTED" } else { "" }),
        Err(err) => format!("{}: volume unknown ({err})", e.name),
    }
}

pub fn run(dir: &Path, which: &[String]) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let nova = nova()?;
    let prog = Program::standard();
    measure::write_wav(&dir.join("program.wav"), &prog.frames)?;
    let mut out = String::new();
    writeln!(out, "{}", volume_line(&nova))?;
    // The program reaches 0 dBFS with Sonar's boosts on top: keep it out of
    // anyone's ears. Loopback records before the endpoint's mute, which the
    // direct take confirms (it comes back silent otherwise).
    let muted = Muted::new(&nova.id)?;
    let takes = record_all(dir, &nova, &prog, which, &mut out);
    drop(muted);
    takes?;
    print!("{out}");
    std::fs::write(dir.join("volumes.txt"), &out)?;
    analyze_dir(dir)
}

fn record_all(dir: &Path, nova: &sysaudio::Endpoint, prog: &Program, which: &[String], out: &mut String) -> Result<()> {
    for (name, prefix) in PATHS {
        if !which.is_empty() && !which.iter().any(|w| w == name) {
            continue;
        }
        let play = if prefix.is_empty() { nova.clone() } else { endpoint_named(prefix)? };
        if !prefix.is_empty() {
            writeln!(out, "{}", volume_line(&play))?;
        }
        eprintln!("{name}: playing {:.0} s into {} ...", prog.frames.len() as f64 / 48000.0, play.name);
        let take = measure::play_and_record(&play.id, &nova.id, &prog.frames)?;
        if take.glitched {
            eprintln!("  warning: the recording had a gap; rerun if the results look off");
        }
        measure::write_wav(&dir.join(format!("{name}.wav")), &take.frames)?;
        std::fs::write(dir.join(format!("{name}.txt")), format!("{}\n{}\n", play.name, nova.name))?;
    }
    Ok(())
}

/// Analyze the takes saved in `dir` by an earlier `capture`.
pub fn analyze_dir(dir: &Path) -> Result<()> {
    let prog = Program::standard();
    let mut out = String::new();
    let (mut game, mut kit) = (None, None);
    // `kit` is kit's EQ APO run offline by `kit-dll`.
    for name in PATHS.iter().map(|p| p.0).chain(["kit"]) {
        let wav = dir.join(format!("{name}.wav"));
        if !wav.exists() {
            continue;
        }
        let rec = measure::read_wav(&wav)?;
        let label = std::fs::read_to_string(dir.join(format!("{name}.txt"))).unwrap_or_default().replace('\n', " -> ");
        let Some(r) = analyze::analyze(&prog, &rec) else {
            writeln!(out, "== {name}: nothing was recorded (no sound reached the headset endpoint)\n")?;
            continue;
        };
        report(&mut out, name, label.trim_end_matches(" -> "), &r)?;
        write_csv(&dir.join(format!("{name}.csv")), &r)?;
        match name {
            "game" => game = Some(r),
            "kit" => kit = Some(r),
            _ => {}
        }
    }
    if let Some(g) = game {
        compare(&mut out, dir, &g, kit.as_ref())?;
    } else if out.is_empty() {
        bail!("no takes in {}", dir.display());
    }
    print!("{out}");
    std::fs::write(dir.join("report.txt"), &out)?;
    Ok(())
}

fn write_csv(path: &PathBuf, r: &Report) -> Result<()> {
    let mut s = String::from("freq_hz,sweep_l_db,sweep_r_db,pink_l_db,pink_r_db,coherence\n");
    for (i, f) in grid().iter().enumerate() {
        writeln!(s, "{f:.2},{:.3},{:.3},{:.3},{:.3},{:.4}", r.sweep[0][i], r.sweep[1][i], r.pink[0][i], r.pink[1][i], r.coherence[i])?;
    }
    std::fs::write(path, s)?;
    Ok(())
}

fn report(out: &mut String, name: &str, label: &str, r: &Report) -> Result<()> {
    writeln!(out, "== {name}: {label}")?;
    writeln!(out, "noise floor {:.1} dBFS; delay {:.1} ms, drifting {} samples over the take", r.noise_floor_db, r.delay_ms, r.delay_spread)?;
    writeln!(
        out,
        "gain: broadband pink {:+.2} dB; at 1 kHz L {:+.2} R {:+.2} dB; L->R crosstalk max {:.1} dB",
        r.pink_gain_db,
        at_freq(&r.sweep[0], 1000.0),
        at_freq(&r.sweep[1], 1000.0),
        r.crosstalk_db
    )?;
    let lr = r.sweep[0].iter().zip(&r.sweep[1]).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
    let sp = r.sweep[0].iter().zip(&r.pink[0]).zip(&r.coherence).filter(|(_, c)| **c > 0.99).map(|((a, b), _)| (a - b).abs()).fold(0.0, f64::max);
    writeln!(out, "L vs R differ by at most {lr:.2} dB; sweep vs pink by at most {sp:.2} dB where coherent")?;
    writeln!(out, "  freq    sweep L  sweep R   pink L   coh")?;
    for f in TABLE_FREQS {
        writeln!(
            out,
            "{f:>6.0}  {:>+8.2} {:>+8.2} {:>+8.2}  {:.3}",
            at_freq(&r.sweep[0], f),
            at_freq(&r.sweep[1], f),
            at_freq(&r.pink[0], f),
            at_freq(&r.coherence, f)
        )?;
    }
    writeln!(out, "level: sine steps (gain is out/in; THD+N relative to the tone)")?;
    for (f, rows) in &r.steps {
        writeln!(out, "  {f:.0} Hz   in dBFS:  gain    out peak   THD+N")?;
        for row in rows {
            writeln!(out, "        {:>+6.1}:  {:>+6.2}  {:>+7.2}  {:>7.1}", row.level_db, row.gain_db, row.out_peak_db, row.thd_db)?;
        }
    }
    let worst = r.full_deviation.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_default();
    writeln!(
        out,
        "0 dBFS sweep: output peak {:+.2} dBFS, {} samples at/over full scale; gain vs the -20 dBFS sweep worst {:+.2} dB at {:.0} Hz",
        r.full_peak_db, r.full_clipped, worst.1, worst.0
    )?;
    let mut line = String::from("  ");
    for f in [30.0, 63.0, 100.0, 300.0, 600.0, 1000.0, 3000.0, 8000.0, 12000.0] {
        if let Some(d) = r.full_deviation.iter().min_by(|a, b| (a.0 / f).ln().abs().total_cmp(&(b.0 / f).ln().abs())) {
            write!(line, "{f:.0} Hz {:+.2}  ", d.1)?;
        }
    }
    writeln!(out, "{line}")?;
    writeln!(out, "jump -24 -> 0 -> -24 dBFS at 63 Hz: gain change vs low level (ms after the jump up; down at 1000 ms)")?;
    let mut line = String::from("  ");
    for (t, g) in &r.jump {
        let t0 = if *t >= 1000.0 { t - 1000.0 } else { *t };
        if [-50.0, 20.0, 50.0, 100.0, 250.0, 500.0, 950.0].iter().any(|m| (t0 - m).abs() < 10.0) || (t0 > -1.0 && t0 < 40.0) {
            write!(line, "{t:.0}:{g:+.2}  ")?;
        }
    }
    writeln!(out, "{line}\n")?;
    Ok(())
}

/// Sonar's measured Game curve against what it stores and what was
/// identified, against kit's Main on the headset, and the 48 kHz biquads
/// that reproduce it for kit.
fn compare(out: &mut String, dir: &Path, g: &Report, kit_apo: Option<&Report>) -> Result<()> {
    let grid = grid();
    let measured: Vec<f64> = g.sweep[0].iter().zip(&g.sweep[1]).map(|(a, b)| (a + b) / 2.0).collect();
    let stored = model::sonar_main_stored();
    let sonar = model::sonar_main();
    let sonar_db = model::on_grid(&sonar, model::SONAR_RATE, &grid);
    let kit = model::headset_curve(&model::kit_main());
    let kit_db = model::on_grid(&kit, model::RATE, &grid);

    writeln!(out, "== Sonar Game vs the stored preset and kit's Main on the headset")?;
    writeln!(out, "measured = Sonar Game, mean of the L and R sweeps")?;
    writeln!(out, "sonar = the stored six filters + Bass Boost as PK 80 Hz +1.5 dB Q 0.707, RBJ at 96 kHz, no preamp")?;
    writeln!(out, "kit = kit's Main as the headset receives it (0.5 dB gain steps, Q in thousandths), RBJ at 48 kHz, no preamp")?;
    writeln!(out, "  freq  measured    sonar  meas-sonar     kit  kit-meas")?;
    for f in TABLE_FREQS {
        let (m, s, k) = (at_freq(&measured, f), at_freq(&sonar_db, f), at_freq(&kit_db, f));
        writeln!(out, "{f:>6.0}  {m:>+8.2} {s:>+8.2}   {:>+8.3}   {k:>+8.2}  {:>+8.2}", m - s, k - m)?;
    }
    let max = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max);
    // What kit's EQ APO runs for Sonar's Main: the matched design at 48 kHz.
    let design = sonar.design(model::RATE);
    let design_db: Vec<f64> = grid.iter().map(|&f| design.response_db(f, model::RATE)).collect();
    let diff = |a: &[f64], b: &[f64]| {
        let d: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
        let rms = (d.iter().map(|x| x * x).sum::<f64>() / d.len() as f64).sqrt();
        (rms, d.iter().fold(0.0f64, |m, x| m.max(x.abs())))
    };
    let (rms, mx) = diff(&design_db, &measured);
    writeln!(out, "kit's EQ APO design (Sonar's filters matched at 48 kHz) vs measured: rms {rms:.3} dB, worst {mx:.3} dB")?;
    if let Some(k) = kit_apo {
        let kit_measured: Vec<f64> = k.sweep[0].iter().zip(&k.sweep[1]).map(|(a, b)| (a + b) / 2.0).collect();
        let (rms, mx) = diff(&kit_measured, &measured);
        writeln!(out, "kit's EQ APO DLL, measured offline, vs Sonar measured: rms {rms:.3} dB, worst {mx:.3} dB")?;
        writeln!(out, "  freq     sonar      kit   kit-sonar")?;
        for f in TABLE_FREQS {
            let (m, k) = (at_freq(&measured, f), at_freq(&kit_measured, f));
            writeln!(out, "{f:>6.0}  {m:>+8.2} {k:>+8.2}   {:>+8.3}", k - m)?;
        }
    }
    let peak_f = |v: &[f64]| grid[v.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0];
    writeln!(out, "peak: measured {:+.2} dB at {:.0} Hz; kit on the headset {:+.2} dB at {:.0} Hz", max(&measured), peak_f(&measured), max(&kit_db), peak_f(&kit_db))?;
    for (label, curve, rate) in [
        ("stored filters alone at 48 kHz", &stored, model::RATE),
        ("stored filters alone at 96 kHz", &stored, model::SONAR_RATE),
        ("identified Sonar model at 96 kHz", &sonar, model::SONAR_RATE),
        ("kit's Main on the headset", &kit, model::RATE),
    ] {
        let (rms, mx) = fit::error(curve, rate, &grid, &measured);
        writeln!(out, "{label} vs measured: rms {rms:.3} dB, worst {mx:.3} dB")?;
    }

    // What kit runs: biquads at 48 kHz fitted to the measurement, starting
    // from the identified filters.
    let free = fit::fit(&sonar, &grid, &measured);
    writeln!(out, "
fit for kit at 48 kHz (every parameter free): rms {:.4} dB, worst {:.4} dB", free.rms_db, free.max_db)?;
    for line in free.curve.to_string().lines() {
        writeln!(out, "    {line}")?;
    }
    let mut csv = String::from("freq_hz,measured_db,sonar_model_db,kit_headset_db,kit_fit_db
");
    for (i, f) in grid.iter().enumerate() {
        writeln!(csv, "{f:.2},{:.3},{:.3},{:.3},{:.3}", measured[i], sonar_db[i], kit_db[i], free.curve.response_db(*f, model::RATE))?;
    }
    std::fs::write(dir.join("compare.csv"), csv)?;
    std::fs::write(dir.join("sonar-game.txt"), free.curve.to_string())?;
    writeln!(out, "
wrote the 48 kHz fit to {}", dir.join("sonar-game.txt").display())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kit_eq::{Filter, Kind, Processor};

    /// A take made by running the program through a known curve, delayed
    /// like a real path, must analyze back to that curve.
    #[test]
    fn analysis_recovers_a_known_curve() {
        let prog = Program::standard();
        let mut truth = model::sonar_main_stored();
        truth.filters.push(Filter::new(Kind::LowShelf, 90.0, 1.5, 0.707));
        truth.preamp_db = -2.0;
        let mut p = Processor::new(&truth.design_rbj(48000.0), 48000, 2, false);
        let mut flat: Vec<f32> = std::iter::repeat_n(0.0, 2 * 1234).chain(prog.frames.iter().flatten().copied()).collect();
        flat.extend(std::iter::repeat_n(0.0, 2 * 48000));
        p.process(&mut flat);
        let rec: Vec<[f32; 2]> = flat.as_chunks::<2>().0.to_vec();
        let r = analyze::analyze(&prog, &rec).unwrap();
        assert!((r.delay_ms - 1234.0 / 48.0).abs() < 0.05, "delay {}", r.delay_ms);
        let grid = grid();
        let (rms, max) = fit::error(&truth, model::RATE, &grid, &r.sweep[0]);
        assert!(max < 0.05, "sweep off by rms {rms} max {max}");
        let coherent: Vec<(f64, f64)> =
            grid.iter().zip(&r.pink[0]).zip(&r.coherence).filter(|(_, c)| **c > 0.99).map(|((f, d), _)| (*f, *d)).collect();
        for (f, d) in coherent {
            assert!((d - truth.response_db(f, 48000.0)).abs() < 0.3, "pink off at {f}");
        }
        // Linear: every level has the same gain and the full-scale sweep tracks the quiet one.
        for (_, rows) in &r.steps {
            assert!(rows.iter().all(|x| (x.gain_db - rows[0].gain_db).abs() < 0.01));
        }
        assert!(r.full_deviation.iter().all(|d| d.1.abs() < 0.05));
        assert!(r.jump.iter().all(|j| j.1.abs() < 0.05), "{:?}", r.jump);
        // The fit recovers it.
        assert!(r.coherence.iter().all(|c| *c > 0.99), "pink coherence");
        let mut start = model::sonar_main_stored();
        start.filters.push(Filter::new(Kind::LowShelf, 80.0, 1.0, 0.7));
        let f = fit::fit(&start, &grid, &r.sweep[0]);
        assert!(f.rms_db < 0.02, "fit rms {}", f.rms_db);
    }
}
