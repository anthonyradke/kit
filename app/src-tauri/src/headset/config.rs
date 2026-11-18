//! EQ presets and headset settings, persisted as JSON in the app config dir.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::mouse::config::add_items;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub presets: Vec<Preset>,
    /// Id of the preset on the headset.
    pub active: String,
    pub headset: HeadsetSettings,
    pub mic: MicSettings,
    /// Make the Nova 5 the Windows default output and input whenever it
    /// (re)appears, so a reconnect never leaves sound on the wrong device.
    pub keep_default: bool,
    /// `SEED_VERSION` the built-in presets in this file were last upgraded to.
    #[serde(default)]
    pub seeds: u32,
}

/// Settings stored on the headset. Defaults match what SteelSeries GG had set.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HeadsetSettings {
    /// 0..=10
    pub sidetone: u8,
    /// 0..=15
    pub mic_volume: u8,
    /// 0 off, 1 low, 2 medium, 3 high
    pub mic_led: u8,
    pub volume_limiter: bool,
    /// Minutes of inactivity before the headset turns off; 0 = never.
    pub auto_off: u8,
    /// kit 0.9's bass slider, a 60 Hz low shelf on every preset. Only read,
    /// to move it onto each preset's Bass (see `Config::normalize`).
    #[serde(skip_serializing)]
    pub bass: f32,
    /// How kit's EQ makes room for its boosts.
    pub preamp: Preamp,
}

/// The gain in front of kit's EQ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preamp {
    /// Turned down by the curve's biggest boost, so nothing is ever limited.
    #[default]
    Auto,
    /// None, like Sonar: as loud as Sonar, and the limiter catches peaks
    /// the boosts push past full scale.
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MicSettings {
    pub denoise: bool,
    /// 0..=100
    pub strength: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// Always `arctis::BANDS` long.
    pub bands: Vec<Band>,
    /// Sonar's Bass, Voice and Treble sliders, on top of the bands.
    pub tone: Tone,
}

/// Sonar's three tone sliders, in dB, in Sonar's steps (0.1 dB) and range
/// ([`TONE_MAX`]). Each is one peak filter with the slider's value as its
/// gain, at [`TONE_FILTERS`]. On the headset's own EQ a slider is rounded to
/// its 0.5 dB steps and ±10 dB.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tone {
    pub bass: f32,
    pub voice: f32,
    pub treble: f32,
}

impl Tone {
    /// The sliders as (frequency, Q, gain), Bass first.
    fn filters(&self) -> [(u16, f32, f32); 3] {
        let [b, v, t] = TONE_FILTERS;
        [(b.0, b.1, self.bass), (v.0, v.1, self.voice), (t.0, t.1, self.treble)]
    }
}

/// (frequency, Q) of the Bass, Voice and Treble peaks. Measured on Sonar's
/// Game output (2026-11-17): with the sliders at +2, +1.5 and +2.5 on top of
/// its Deep Bass preset, these three peaks account for the difference to
/// 0.03 dB rms. Sonar's older "Bass Boost" is the same 80 Hz peak.
/// A second measurement (Main's bands with +3.3, +1.8 and +2.3) agreed to
/// 0.09 dB rms, so the gain is the slider's value at any setting.
pub const TONE_FILTERS: [(u16, f32); 3] = [(80, 0.707), (2000, 0.5), (8000, 0.707)];
/// Sonar's slider range, in dB either way.
pub const TONE_MAX: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Band {
    pub enabled: bool,
    pub filter: Filter,
    pub freq: u16,
    pub gain: f32,
    pub q: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Filter {
    #[default]
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

impl Default for Config {
    fn default() -> Self {
        let presets = seed_presets();
        Config {
            active: presets[1].id.clone(),
            presets,
            headset: HeadsetSettings::default(),
            mic: MicSettings::default(),
            keep_default: true,
            seeds: SEED_VERSION,
        }
    }
}

impl Default for HeadsetSettings {
    fn default() -> Self {
        HeadsetSettings {
            sidetone: 1,
            mic_volume: 15,
            mic_led: 3,
            volume_limiter: true,
            auto_off: 30,
            bass: 0.0,
            preamp: Preamp::Auto,
        }
    }
}

impl Default for MicSettings {
    fn default() -> Self {
        // Sonar had ClearCast noise cancellation at 90%.
        MicSettings { denoise: true, strength: 90 }
    }
}

impl Default for Preset {
    fn default() -> Self {
        Preset { id: new_id(), name: "New preset".into(), bands: flat_bands(), tone: Tone::default() }
    }
}

impl Default for Band {
    fn default() -> Self {
        Band { enabled: false, filter: Filter::Peaking, freq: 1000, gain: 0.0, q: 0.707 }
    }
}

const STANDARD_FREQS: [u16; arctis::BANDS] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

fn flat_bands() -> Vec<Band> {
    STANDARD_FREQS.iter().map(|&freq| Band { enabled: true, freq, q: 1.414, ..Band::default() }).collect()
}

fn peak(freq: u16, gain: f32, q: f32) -> Band {
    Band { enabled: true, filter: Filter::Peaking, freq, gain, q }
}

fn shelf(filter: Filter, freq: u16, gain: f32) -> Band {
    Band { enabled: true, filter, freq, gain, q: 0.707 }
}

/// Bumped whenever the built-in presets change; see `Config::upgrade_presets`.
const SEED_VERSION: u32 = 5;

/// Flat and the user's two Sonar Game presets, as Sonar stores them and as
/// measured on its output (docs/sonar-eq.md). Tony Preset is the user's own:
/// the bands of their earlier Main with Bass +3.3, Voice +1.8 and Treble
/// +2.3. Deep Bass is Sonar's built-in "Music: Deep Bass" with the sliders
/// the user set. Neither has a preamp, as in Sonar: kit's preamp setting
/// decides.
fn seed_presets() -> Vec<Preset> {
    let pad = |mut bands: Vec<Band>| {
        bands.resize(arctis::BANDS, Band::default());
        bands
    };
    let deep_bass =
        [(31, 0.0), (60, 6.0), (150, 3.0), (350, -2.5), (500, 0.0), (1000, 0.0), (2000, 0.0), (4000, 0.0), (11000, 3.0), (16000, 0.0)];
    vec![
        Preset { id: "flat".into(), name: "Flat".into(), bands: flat_bands(), tone: Tone::default() },
        Preset {
            id: "tony".into(),
            name: "Tony Preset".into(),
            bands: pad(vec![
                peak(55, 4.0, 0.707),
                peak(100, 3.0, 0.707),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 3.0, 0.707),
            ]),
            tone: Tone { bass: 3.3, voice: 1.8, treble: 2.3 },
        },
        Preset {
            id: "deep-bass".into(),
            name: "Deep Bass".into(),
            bands: deep_bass.iter().map(|&(f, g)| peak(f, g, 0.707)).collect(),
            tone: Tone { bass: 2.0, voice: 1.5, treble: 2.5 },
        },
    ]
}

/// The `SEED_VERSION` that first shipped a built-in, so an upgrade adds the
/// new ones without bringing back one the user deleted.
fn seed_added_in(id: &str) -> u32 {
    match id {
        "tony" | "deep-bass" => 5,
        _ => 0,
    }
}

/// Built-in names earlier releases used, as (id, old name).
const PAST_SEED_NAMES: [(&str, &str); 1] = [("bass-music", "Bass Music")];

/// Every set of bands earlier releases shipped for a built-in id, including
/// the ones no longer shipped. A preset whose bands are in here is untouched.
fn past_seed_bands(id: &str) -> Vec<Vec<Band>> {
    let hp = |freq| Band { enabled: true, filter: Filter::HighPass, freq, gain: 0.0, q: 0.707 };
    let versions = match id {
        "flat" => vec![flat_bands()],
        "main" => vec![
            vec![
                peak(55, 4.0, 0.707),
                peak(100, 3.0, 0.707),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 3.0, 0.707),
                shelf(Filter::LowShelf, 80, 1.5),
            ],
            vec![
                shelf(Filter::LowShelf, 55, 3.0),
                peak(90, -2.0, 1.0),
                peak(600, 2.5, 4.0),
                peak(5000, 1.5, 0.707),
                shelf(Filter::HighShelf, 9000, 2.0),
            ],
            vec![
                shelf(Filter::LowShelf, 55, 5.0),
                peak(90, -2.5, 1.0),
                peak(300, 1.0, 1.0),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 2.5, 0.707),
            ],
            vec![
                peak(55, 4.0, 0.707),
                peak(100, 3.0, 0.707),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 3.0, 0.707),
                peak(80, 1.5, 0.707),
            ],
        ],
        "balanced" => vec![
            vec![shelf(Filter::LowShelf, 45, 2.5), peak(85, -3.0, 1.0), peak(300, 1.5, 1.0)],
            vec![
                shelf(Filter::LowShelf, 45, 3.5),
                peak(85, -2.5, 1.0),
                peak(300, 1.5, 1.0),
                peak(3000, 1.5, 1.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ],
        ],
        "opus" => vec![vec![
            peak(50, 2.5, 0.707),
            peak(120, 0.0, 1.0),
            peak(300, -1.5, 1.2),
            peak(600, -0.5, 1.4),
            peak(1000, 0.0, 1.0),
            peak(2500, 1.5, 1.4),
            peak(4981, -6.5, 0.707),
            peak(3000, 1.5, 0.707),
        ]],
        "footsteps" => vec![
            vec![
                shelf(Filter::LowShelf, 120, -4.0),
                peak(250, -2.0, 1.0),
                peak(2000, 2.0, 1.2),
                peak(3500, 4.0, 1.4),
                peak(6000, 2.0, 1.4),
            ],
            vec![
                shelf(Filter::LowShelf, 120, -4.0),
                peak(250, -2.0, 1.0),
                peak(2000, 1.5, 1.2),
                peak(3500, 3.0, 1.4),
                peak(6000, 1.5, 1.4),
            ],
        ],
        "voice" => vec![vec![hp(90), peak(200, -2.0, 1.0), peak(1500, 1.5, 1.0), peak(3000, 3.0, 1.2), peak(7000, -2.0, 2.0)]],
        "warm" => vec![
            vec![
                shelf(Filter::LowShelf, 105, 4.5),
                peak(400, -1.0, 1.0),
                peak(2500, 1.0, 1.4),
                peak(6000, -1.5, 2.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ],
            vec![
                shelf(Filter::LowShelf, 60, 2.0),
                peak(85, -1.5, 1.2),
                peak(250, 1.5, 0.9),
                peak(6000, -1.5, 2.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ],
        ],
        "bass-music" => vec![
            vec![
                shelf(Filter::LowShelf, 100, 6.5),
                peak(50, 2.5, 1.2),
                peak(300, -2.0, 1.0),
                peak(2500, 1.5, 1.2),
                peak(6500, -1.0, 2.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ],
            vec![
                shelf(Filter::LowShelf, 45, 3.0),
                peak(90, -1.5, 1.2),
                shelf(Filter::HighShelf, 160, -4.0),
                peak(350, 1.0, 1.0),
                peak(3000, 1.0, 1.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ],
            vec![
                shelf(Filter::LowShelf, 50, 6.0),
                peak(95, -2.0, 1.0),
                peak(300, 1.0, 1.0),
                peak(3000, 1.5, 1.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ],
        ],
        "cinema" => vec![
            vec![
                shelf(Filter::LowShelf, 70, 5.0),
                peak(250, -1.5, 1.0),
                peak(2000, 2.5, 1.0),
                shelf(Filter::HighShelf, 9000, 2.0),
            ],
            vec![
                shelf(Filter::LowShelf, 45, 3.0),
                peak(90, -1.5, 1.0),
                peak(2000, 2.0, 1.0),
                shelf(Filter::HighShelf, 9000, 1.5),
            ],
        ],
        "late-night" => vec![
            vec![shelf(Filter::LowShelf, 100, 5.0), peak(1000, -1.0, 0.707), shelf(Filter::HighShelf, 8000, 3.5)],
            vec![
                shelf(Filter::LowShelf, 60, 3.0),
                peak(90, -1.0, 1.0),
                peak(1000, -1.0, 0.707),
                shelf(Filter::HighShelf, 8000, 3.0),
            ],
        ],
        _ => vec![],
    };
    versions
        .into_iter()
        .map(|mut bands| {
            bands.resize(arctis::BANDS, Band::default());
            bands
        })
        .collect()
}

pub fn new_id() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}", nanos)
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let mut config: Config = crate::store::load(path);
        config.normalize();
        if config.upgrade_presets() {
            let _ = config.save(path);
        }
        config
    }

    /// Bring built-in presets from an older release up to date. A built-in
    /// the user edited keeps their bands as a "(yours)" copy next to it. A
    /// built-in that is no longer shipped is removed unless it was edited or
    /// is the active preset; then it stays as the user's own.
    /// Returns whether anything changed.
    pub fn upgrade_presets(&mut self) -> bool {
        if self.seeds >= SEED_VERSION {
            return false;
        }
        let seeds = seed_presets();
        // Every earlier built-in had its sliders at zero.
        let untouched = |p: &Preset| p.tone == Tone::default() && past_seed_bands(&p.id).contains(&p.bands);
        let mut kept = Vec::new();
        let active = self.active.clone();
        let mut active_moved = None;
        self.presets.retain_mut(|p| {
            if let Some(seed) = seeds.iter().find(|s| s.id == p.id) {
                if (p.bands != seed.bands || p.tone != seed.tone) && !untouched(p) {
                    let copy = Preset { id: new_id() + &p.id, name: format!("{} (yours)", p.name), bands: p.bands.clone(), tone: p.tone };
                    // The headset keeps sounding the way the user made it.
                    if p.id == active {
                        active_moved = Some(copy.id.clone());
                    }
                    kept.push(copy);
                }
                p.bands = seed.bands.clone();
                p.tone = seed.tone;
                if PAST_SEED_NAMES.contains(&(p.id.as_str(), p.name.as_str())) {
                    p.name = seed.name.clone();
                }
                true
            } else {
                p.id == active || !untouched(p)
            }
        });
        self.presets.extend(kept);
        if let Some(id) = active_moved {
            self.active = id;
        }
        for (i, seed) in seeds.into_iter().enumerate() {
            if seed_added_in(&seed.id) > self.seeds && !self.presets.iter().any(|p| p.id == seed.id) {
                self.presets.insert(i.min(self.presets.len()), seed);
            }
        }
        self.seeds = SEED_VERSION;
        true
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::store::save(path, self)
    }

    /// Add the presets of `other` that this config lacks.
    pub fn add_from(&mut self, other: Config) {
        add_items(&mut self.presets, other.presets, |p| &mut p.id, |p| &mut p.name);
    }

    /// Repair anything the UI or a hand-edited file could get wrong.
    pub fn normalize(&mut self) {
        if self.presets.is_empty() {
            self.presets = seed_presets();
        }
        for p in &mut self.presets {
            if !crate::store::valid_id(&p.id) {
                p.id = new_id();
            }
            p.bands.resize(arctis::BANDS, Band::default());
            for b in &mut p.bands {
                b.freq = b.freq.clamp(arctis::FREQ_MIN, arctis::FREQ_MAX);
                // The headset works in 0.5 dB steps.
                b.gain = (b.gain.clamp(arctis::GAIN_MIN, arctis::GAIN_MAX) * 2.0).round() / 2.0;
                b.q = if b.q.is_finite() { b.q.clamp(arctis::Q_MIN, arctis::Q_MAX) } else { 0.707 };
                b.q = (b.q * 1000.0).round() / 1000.0;
            }
        }
        let step = |g: f32| if g.is_finite() { (g.clamp(-TONE_MAX, TONE_MAX) * 10.0).round() / 10.0 } else { 0.0 };
        // kit 0.9's bass slider applied to every preset: move it onto each
        // preset's Bass (an 80 Hz peak rather than a 60 Hz shelf, near enough).
        let old_bass = if self.headset.bass.is_finite() { self.headset.bass } else { 0.0 };
        self.headset.bass = 0.0;
        for p in &mut self.presets {
            p.tone = Tone { bass: step(p.tone.bass + old_bass), voice: step(p.tone.voice), treble: step(p.tone.treble) };
        }
        if !self.presets.iter().any(|p| p.id == self.active) {
            self.active = self.presets[0].id.clone();
        }
        let h = &mut self.headset;
        h.sidetone = h.sidetone.min(arctis::SIDETONE_MAX);
        h.mic_volume = h.mic_volume.min(arctis::MIC_VOLUME_MAX);
        h.mic_led = h.mic_led.min(3);
        self.mic.strength = self.mic.strength.min(100);
    }

    pub fn active_preset(&self) -> &Preset {
        self.presets.iter().find(|p| p.id == self.active).unwrap_or(&self.presets[0])
    }
}

/// The headset output's rate, which kit's EQ is designed for.
pub const EQ_RATE: f64 = 48000.0;

impl Preset {
    /// The bands and the tone sliders as kit's EQ APO runs them (in
    /// Equalizer APO's text format through `kit_eq::Curve`). Unlike the
    /// headset, it has room for every slider.
    pub fn to_curve(&self, preamp: Preamp) -> kit_eq::Curve {
        let bands = self.bands.iter().filter(|b| b.enabled).map(|b| {
            let kind = match b.filter {
                Filter::Peaking => kit_eq::Kind::Peaking,
                Filter::LowShelf => kit_eq::Kind::LowShelf,
                Filter::HighShelf => kit_eq::Kind::HighShelf,
                Filter::LowPass => kit_eq::Kind::LowPass,
                Filter::HighPass => kit_eq::Kind::HighPass,
            };
            kit_eq::Filter::new(kind, b.freq as f64, b.gain as f64, b.q as f64)
        });
        let tone = self
            .tone
            .filters()
            .into_iter()
            .filter(|t| t.2 != 0.0)
            .map(|(f, q, g)| kit_eq::Filter::new(kit_eq::Kind::Peaking, f as f64, g as f64, q as f64));
        let mut curve = kit_eq::Curve { preamp_db: 0.0, filters: bands.chain(tone).collect() };
        if preamp == Preamp::Auto {
            // The response the APO matches (see `kit_eq::Curve::design`).
            curve.preamp_db = -curve.max_boost_db(2.0 * EQ_RATE);
        }
        curve
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_survive_normalize() {
        let mut c = Config::default();
        let before = c.clone();
        c.normalize();
        assert_eq!(c, before);
        for p in &c.presets {
            assert_eq!(p.bands.len(), arctis::BANDS);
        }
        assert_eq!(c.active_preset().name, "Tony Preset");
    }

    fn seed(id: &str) -> Preset {
        seed_presets().into_iter().find(|p| p.id == id).unwrap()
    }

    /// kit's curve for `id` against Sonar's Game output, measured with
    /// loopback on 2026-11-17 (docs/sonar-eq.md), as (Hz, dB) pairs.
    fn matches_sonar(id: &str, measured: &[(f64, f64)], within: f64) {
        let curve = seed(id).to_curve(Preamp::Off);
        for &(f, db) in measured {
            let ours = curve.response_db(f, 2.0 * EQ_RATE);
            assert!((ours - db).abs() < within, "{id} at {f} Hz: {ours:.2} dB, Sonar {db:.2} dB");
        }
    }

    #[test]
    fn deep_bass_matches_what_sonar_played() {
        let measured = [
            (63.0, 8.62),
            (80.0, 8.40),
            (100.0, 7.48),
            (150.0, 5.08),
            (200.0, 2.96),
            (400.0, -0.88),
            (1000.0, 0.69),
            (2000.0, 1.89),
            (3000.0, 2.31),
            (5000.0, 3.45),
            (8000.0, 5.23),
            (10000.0, 5.39),
            (16000.0, 3.25),
            (20000.0, 2.04),
        ];
        matches_sonar("deep-bass", &measured, 0.1);
        // Auto takes off its biggest boost, about +8.7 dB near 70 Hz.
        let auto = seed("deep-bass").to_curve(Preamp::Auto).preamp_db;
        assert!((auto + 8.75).abs() < 0.1, "{auto}");
    }

    #[test]
    fn tony_matches_what_sonar_played() {
        // Only where nothing else was playing during the take (left and
        // right within 0.1 dB of each other, coherence 0.97 or more).
        let measured = [
            (25.0, 2.68),
            (30.0, 3.69),
            (35.0, 4.83),
            (90.0, 8.77),
            (300.0, 1.76),
            (600.0, 5.09),
            (3000.0, 4.56),
            (5000.0, 7.24),
            (8000.0, 9.14),
            (10000.0, 8.70),
            (12000.0, 7.56),
            (16000.0, 5.03),
            (20000.0, 3.17),
        ];
        matches_sonar("tony", &measured, 0.15);
    }

    fn old(id: &str, version: usize) -> Preset {
        Preset { id: id.into(), name: id.into(), bands: past_seed_bands(id)[version].clone(), tone: Tone::default() }
    }

    #[test]
    fn upgrade_from_first_release() {
        let mut bass = old("bass-music", 0);
        bass.name = "Bass Music".into();
        let mut edited = old("main", 0);
        edited.bands[0].gain = 2.0;
        let mut c = Config {
            presets: vec![old("flat", 0), edited.clone(), old("warm", 0), old("cinema", 0), bass],
            active: "cinema".into(),
            seeds: 0,
            ..Config::default()
        };
        c.normalize();
        assert!(c.upgrade_presets());
        let ids: Vec<_> = c.presets.iter().map(|p| p.id.as_str()).collect();
        // Warm and Bass are gone; the edited Main is the user's now; Cinema
        // stays because it's active; Tony Preset and Deep Bass are added.
        assert_eq!(ids, ["flat", "tony", "deep-bass", "main", "cinema"]);
        assert_eq!(c.presets[3], edited);
        assert!(!c.upgrade_presets());
    }

    #[test]
    fn an_untouched_active_preset_keeps_playing_after_an_upgrade() {
        let mut c = Config { presets: vec![old("flat", 0), old("main", 3)], active: "main".into(), seeds: 4, ..Config::default() };
        c.normalize();
        assert!(c.upgrade_presets());
        assert_eq!(c.active_preset().bands, past_seed_bands("main")[3]);
    }

    #[test]
    fn upgrade_from_review_tuned_presets() {
        // 0.9: Flat, Main, Balanced and Bass, on Balanced.
        let mut c = Config {
            presets: vec![old("flat", 0), old("main", 3), old("balanced", 1), old("bass-music", 2)],
            active: "balanced".into(),
            seeds: 4,
            ..Config::default()
        };
        c.normalize();
        assert!(c.upgrade_presets());
        let ids: Vec<_> = c.presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["flat", "tony", "deep-bass", "balanced"], "unused, untouched built-ins go");

        let mut fresh = Config::default();
        assert!(!fresh.upgrade_presets());
    }

    #[test]
    fn the_old_bass_slider_moves_onto_every_preset() {
        let mut c = Config::default();
        c.headset.bass = 2.0;
        c.normalize();
        assert_eq!(c.headset.bass, 0.0);
        let bass: Vec<f32> = c.presets.iter().map(|p| p.tone.bass).collect();
        assert_eq!(bass, [2.0, 5.3, 4.0]);
        // It is never written back, so it moves only once.
        let json = serde_json::to_value(&c).unwrap();
        assert!(json["headset"].get("bass").is_none());
    }

    #[test]
    fn kit_curve_has_the_bands_tone_and_headroom() {
        // All ten bands plus the three sliders, and the text round-trips.
        let curve = seed("deep-bass").to_curve(Preamp::Off);
        assert_eq!(curve.filters.len(), 13);
        assert_eq!(curve.preamp_db, 0.0);
        assert_eq!(kit_eq::Curve::parse(&curve.to_string()).unwrap().filters.len(), 13);
        // Flat stays flat: no filters with gain, no preamp.
        let flat = seed("flat").to_curve(Preamp::Auto);
        assert_eq!(flat.preamp_db, 0.0);
        assert!(flat.max_boost_db(96000.0).abs() < 1e-9);
    }

    #[test]
    fn tone_sliders_are_sonars_peaks() {
        let p = Preset { tone: Tone { bass: 4.0, voice: -2.0, treble: 0.0 }, ..seed("flat") };
        let filters = p.to_curve(Preamp::Off).filters;
        assert_eq!(filters.len(), 12, "a slider at 0 adds nothing");
        assert_eq!(filters[10], kit_eq::Filter::new(kit_eq::Kind::Peaking, 80.0, 4.0, 0.707f32 as f64));
        assert_eq!(filters[11], kit_eq::Filter::new(kit_eq::Kind::Peaking, 2000.0, -2.0, 0.5));
        // Sonar's steps and range.
        let mut c = Config { presets: vec![Preset { tone: Tone { bass: 3.33, voice: 20.0, treble: f32::NAN }, ..seed("flat") }], ..Config::default() };
        c.normalize();
        assert_eq!(c.presets[0].tone, Tone { bass: 3.3, voice: TONE_MAX, treble: 0.0 });
    }
}
