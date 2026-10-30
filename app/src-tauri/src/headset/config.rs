//! EQ presets and headset settings, persisted as JSON in the app config dir.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

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
    /// dB added to every preset as a low shelf (see `Preset::to_eq`).
    pub bass: f32,
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
}

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
        HeadsetSettings { sidetone: 1, mic_volume: 15, mic_led: 3, volume_limiter: true, auto_off: 30, bass: 0.0 }
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
        Preset { id: new_id(), name: "New preset".into(), bands: flat_bands() }
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
const SEED_VERSION: u32 = 2;

/// Flat plus three tuned presets: the user's Sonar preset, an everyday
/// correction and a bass one.
///
/// The Nova 5 is already bass heavy: reviews measure a ~+4.5 dB high-bass
/// peak around 80 Hz with less below 60 Hz and a low-mid dip after it.
/// Boosting that peak again is what makes bass boomy and muffled, so these
/// cut 80-100 Hz and add their low end under 60 Hz instead, turning the peak
/// into a clean shelf. The headset has no preamp; the summed boost stays at
/// or under +6 dB, the most SteelSeries' own firmware presets use.
fn seed_presets() -> Vec<Preset> {
    let pad = |mut bands: Vec<Band>| {
        bands.resize(arctis::BANDS, Band::default());
        bands
    };
    vec![
        Preset { id: "flat".into(), name: "Flat".into(), bands: flat_bands() },
        Preset {
            // The user's Sonar preset (sub bass, a narrow 600 Hz lift, bright
            // treble) with its bass moved below the headset's own peak.
            id: "main".into(),
            name: "Main".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 55, 5.0),
                peak(90, -2.5, 1.0),
                peak(300, 1.0, 1.0),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 2.5, 0.707),
            ]),
        },
        Preset {
            // Everyday listening: the 80 Hz boom flattened into an even
            // +2.5 dB below 100 Hz, the low-mid dip filled, a little presence
            // and air.
            id: "balanced".into(),
            name: "Balanced".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 45, 3.5),
                peak(85, -2.5, 1.0),
                peak(300, 1.5, 1.0),
                peak(3000, 1.5, 1.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ]),
        },
        Preset {
            // Music and movies with a lot of clean low end: about +6 dB of
            // sub bass falling to +2 dB at 100 Hz, no boom peak.
            id: "bass-music".into(),
            name: "Bass".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 50, 6.0),
                peak(95, -2.0, 1.0),
                peak(300, 1.0, 1.0),
                peak(3000, 1.5, 1.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ]),
        },
    ]
}

/// The `SEED_VERSION` that first shipped a built-in, so an upgrade adds the
/// new ones without bringing back one the user deleted.
fn seed_added_in(id: &str) -> u32 {
    match id {
        "balanced" => 1,
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
        ],
        "balanced" => vec![vec![shelf(Filter::LowShelf, 45, 2.5), peak(85, -3.0, 1.0), peak(300, 1.5, 1.0)]],
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
        let mut config = match std::fs::read_to_string(path) {
            Err(_) => Config::default(),
            Ok(text) => match serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')) {
                Ok(c) => c,
                Err(e) => {
                    // Keep the unreadable file instead of overwriting the user's presets.
                    eprintln!("config unreadable ({e}); backing it up and starting fresh");
                    let _ = std::fs::copy(path, path.with_extension("json.bak"));
                    Config::default()
                }
            },
        };
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
        let untouched = |p: &Preset| past_seed_bands(&p.id).contains(&p.bands);
        let mut kept = Vec::new();
        let active = self.active.clone();
        self.presets.retain_mut(|p| {
            if let Some(seed) = seeds.iter().find(|s| s.id == p.id) {
                if p.bands != seed.bands && !untouched(p) {
                    kept.push(Preset { id: new_id() + &p.id, name: format!("{} (yours)", p.name), bands: p.bands.clone() });
                }
                p.bands = seed.bands.clone();
                if PAST_SEED_NAMES.contains(&(p.id.as_str(), p.name.as_str())) {
                    p.name = seed.name.clone();
                }
                true
            } else {
                p.id == active || !untouched(p)
            }
        });
        self.presets.extend(kept);
        for (i, seed) in seeds.into_iter().enumerate() {
            if seed_added_in(&seed.id) > self.seeds && !self.presets.iter().any(|p| p.id == seed.id) {
                self.presets.insert(i.min(self.presets.len()), seed);
            }
        }
        self.seeds = SEED_VERSION;
        true
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// Repair anything the UI or a hand-edited file could get wrong.
    pub fn normalize(&mut self) {
        if self.presets.is_empty() {
            self.presets = seed_presets();
        }
        for p in &mut self.presets {
            if p.id.is_empty() {
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
        if !self.presets.iter().any(|p| p.id == self.active) {
            self.active = self.presets[0].id.clone();
        }
        let h = &mut self.headset;
        h.sidetone = h.sidetone.min(arctis::SIDETONE_MAX);
        h.mic_volume = h.mic_volume.min(arctis::MIC_VOLUME_MAX);
        h.mic_led = h.mic_led.min(3);
        h.bass = if h.bass.is_finite() { (h.bass.clamp(arctis::GAIN_MIN, arctis::GAIN_MAX) * 2.0).round() / 2.0 } else { 0.0 };
        self.mic.strength = self.mic.strength.min(100);
    }

    pub fn active_preset(&self) -> &Preset {
        self.presets.iter().find(|p| p.id == self.active).unwrap_or(&self.presets[0])
    }
}

/// Corner and slope of the bass slider's shelf. Below the headset's own
/// 80 Hz peak, so more bass means more rumble rather than more boom.
pub const BASS_FREQ: u16 = 60;
const BASS_Q: f32 = 0.707;

impl Preset {
    /// The bands in the headset's format with the bass slider applied;
    /// disabled bands become unused slots.
    pub fn to_eq(&self, bass: f32) -> arctis::Eq {
        let mut eq = [None; arctis::BANDS];
        for (slot, b) in eq.iter_mut().zip(self.with_bass(bass)) {
            if b.enabled {
                *slot = Some(arctis::Band { filter: b.filter.into(), freq: b.freq, gain: b.gain, q: b.q });
            }
        }
        eq
    }

    /// The bands with the bass slider's low shelf added. It takes a band that
    /// is off or does nothing (0 dB); with none free it is added to the
    /// preset's own low shelf. A preset with neither gets no bass change.
    /// `headset.js` (`withBass`) mirrors this for the graph.
    pub fn with_bass(&self, bass: f32) -> Vec<Band> {
        let mut bands = self.bands.clone();
        if bass == 0.0 {
            return bands;
        }
        let idle = |b: &Band| {
            !b.enabled || (b.gain == 0.0 && matches!(b.filter, Filter::Peaking | Filter::LowShelf | Filter::HighShelf))
        };
        if let Some(b) = bands.iter_mut().find(|b| idle(b)) {
            *b = Band { enabled: true, filter: Filter::LowShelf, freq: BASS_FREQ, gain: bass, q: BASS_Q };
        } else if let Some(b) = bands.iter_mut().find(|b| b.filter == Filter::LowShelf) {
            b.gain = (b.gain + bass).clamp(arctis::GAIN_MIN, arctis::GAIN_MAX);
        }
        bands
    }
}

impl From<Filter> for arctis::Filter {
    fn from(f: Filter) -> Self {
        match f {
            Filter::Peaking => arctis::Filter::Peaking,
            Filter::LowShelf => arctis::Filter::LowShelf,
            Filter::HighShelf => arctis::Filter::HighShelf,
            Filter::LowPass => arctis::Filter::LowPass,
            Filter::HighPass => arctis::Filter::HighPass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_survive_normalize_and_fit_the_headset() {
        let mut c = Config::default();
        let before = c.clone();
        c.normalize();
        assert_eq!(c, before);
        for p in &c.presets {
            assert_eq!(p.bands.len(), arctis::BANDS);
        }
        assert_eq!(c.active_preset().name, "Main");
    }

    /// The summed response of `bands` in dB at `f`, with the same RBJ
    /// biquads the graph in `headset.js` draws.
    fn response_db(bands: &[Band], f: f64) -> f64 {
        let fs = 48000.0;
        bands
            .iter()
            .filter(|b| b.enabled)
            .map(|b| {
                let w = 2.0 * std::f64::consts::PI * b.freq as f64 / fs;
                let (cos, alpha, a) = (w.cos(), w.sin() / (2.0 * b.q as f64), 10f64.powf(b.gain as f64 / 40.0));
                let sa = 2.0 * a.sqrt() * alpha;
                let [b0, b1, b2, a0, a1, a2] = match b.filter {
                    Filter::Peaking => [1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a],
                    Filter::LowShelf => [
                        a * (a + 1.0 - (a - 1.0) * cos + sa),
                        2.0 * a * (a - 1.0 - (a + 1.0) * cos),
                        a * (a + 1.0 - (a - 1.0) * cos - sa),
                        a + 1.0 + (a - 1.0) * cos + sa,
                        -2.0 * (a - 1.0 + (a + 1.0) * cos),
                        a + 1.0 + (a - 1.0) * cos - sa,
                    ],
                    Filter::HighShelf => [
                        a * (a + 1.0 + (a - 1.0) * cos + sa),
                        -2.0 * a * (a - 1.0 + (a + 1.0) * cos),
                        a * (a + 1.0 + (a - 1.0) * cos - sa),
                        a + 1.0 - (a - 1.0) * cos + sa,
                        2.0 * (a - 1.0 - (a + 1.0) * cos),
                        a + 1.0 - (a - 1.0) * cos - sa,
                    ],
                    Filter::LowPass => [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
                    Filter::HighPass => [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha],
                };
                let wf = 2.0 * std::f64::consts::PI * f / fs;
                let (c1, s1, c2, s2) = (wf.cos(), wf.sin(), (2.0 * wf).cos(), (2.0 * wf).sin());
                let (nr, ni) = (b0 + b1 * c1 + b2 * c2, -(b1 * s1 + b2 * s2));
                let (dr, di) = (a0 + a1 * c1 + a2 * c2, -(a1 * s1 + a2 * s2));
                10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
            })
            .sum()
    }

    #[test]
    fn seeds_leave_headroom() {
        // Without a preamp a boost is level on top of full-scale music, and
        // overlapping bands add up.
        for p in seed_presets() {
            for k in 0..=300 {
                let f = 20.0 * 1000f64.powf(k as f64 / 300.0);
                let db = response_db(&p.bands, f);
                assert!(db <= 6.05, "{} boosts {db:.1} dB at {f:.0} Hz", p.name);
            }
        }
        // And none of them stacks on the headset's own 80 Hz peak.
        for p in seed_presets() {
            assert!(response_db(&p.bands, 80.0) <= 0.0, "{} boosts the 80 Hz peak", p.name);
        }
    }

    fn old(id: &str, version: usize) -> Preset {
        Preset { id: id.into(), name: id.into(), bands: past_seed_bands(id)[version].clone() }
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
        let seeds = seed_presets();
        let ids: Vec<_> = c.presets.iter().map(|p| p.id.as_str()).collect();
        // Warm is gone; Cinema stays because it's active; Balanced is added.
        assert_eq!(&ids[..5], ["flat", "main", "balanced", "cinema", "bass-music"]);
        assert_eq!(c.presets[1].bands, seeds[1].bands);
        assert_eq!(c.presets[4].bands, seeds[3].bands);
        assert_eq!(c.presets[4].name, "Bass");
        assert_eq!(c.presets[5].name, "main (yours)");
        assert_eq!(c.presets[5].bands, edited.bands);
        assert_eq!(c.presets.len(), 6);
        assert!(!c.upgrade_presets());
    }

    #[test]
    fn upgrade_from_second_release() {
        let mut footsteps = old("footsteps", 1);
        footsteps.bands[1].gain = -3.0;
        let mut c = Config {
            presets: vec![old("flat", 0), old("main", 1), old("opus", 0), footsteps.clone(), old("voice", 0)],
            active: "main".into(),
            seeds: 1,
            ..Config::default()
        };
        c.normalize();
        assert!(c.upgrade_presets());
        let ids: Vec<_> = c.presets.iter().map(|p| p.id.as_str()).collect();
        // The user deleted Balanced and Bass in this release: they stay deleted.
        // Opus and Voice are unused built-ins; the edited Footsteps is theirs now.
        assert_eq!(ids, ["flat", "main", "footsteps"]);
        assert_eq!(c.presets[1].bands, seed_presets()[1].bands);
        assert_eq!(c.presets[2], footsteps);

        let mut fresh = Config::default();
        assert!(!fresh.upgrade_presets());
    }

    #[test]
    fn bass_takes_a_free_band_or_joins_the_low_shelf() {
        let c = Config::default();
        let flat = &c.presets[0];
        assert_eq!(flat.with_bass(0.0), flat.bands);
        let bands = flat.with_bass(4.0);
        assert_eq!(bands[0], Band { enabled: true, filter: Filter::LowShelf, freq: BASS_FREQ, gain: 4.0, q: BASS_Q });
        assert_eq!(bands[1..], flat.bands[1..]);

        let full = Preset {
            bands: (0..arctis::BANDS as u16)
                .map(|i| if i == 3 { shelf(Filter::LowShelf, 80, 8.0) } else { peak(200 + i * 100, 1.0, 1.0) })
                .collect(),
            ..Preset::default()
        };
        assert_eq!(full.with_bass(4.0)[3].gain, arctis::GAIN_MAX);
        assert_eq!(full.with_bass(-4.0)[3].gain, 4.0);
    }
}
