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
const SEED_VERSION: u32 = 1;

/// Flat, the user's two Sonar game presets converted to headset bands, and a
/// few purpose-built ones.
///
/// Two facts shape every curve. The headset has no preamp, so a boost is
/// extra level on top of music that is already mastered near full scale: big
/// boosts clip or trip the volume limiter. Every preset here stays at or below
/// +3 dB and gets the rest of its tilt by cutting. And the Nova 5 is already
/// bass heavy: reviews measure a ~+4.5 dB high-bass peak around 80 Hz with
/// less below 60 Hz and a low-mid dip after it. Boosting that peak again is
/// what makes bass boomy and muffled, so the bass presets cut 80-100 Hz and
/// add their low end under 60 Hz instead, turning the peak into a clean shelf.
fn seed_presets() -> Vec<Preset> {
    let pad = |mut bands: Vec<Band>| {
        bands.resize(arctis::BANDS, Band::default());
        bands
    };
    vec![
        Preset { id: "flat".into(), name: "Flat".into(), bands: flat_bands() },
        Preset {
            // The user's Sonar preset: deep bass, a narrow 600 Hz lift and
            // extra air, trimmed to fit without a preamp.
            id: "main".into(),
            name: "Main".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 55, 3.0),
                peak(90, -2.0, 1.0),
                peak(600, 2.5, 4.0),
                peak(5000, 1.5, 0.707),
                shelf(Filter::HighShelf, 9000, 2.0),
            ]),
        },
        Preset {
            // Everyday listening: the 80 Hz boom flattened into an even +2 dB
            // below 100 Hz, the low-mid dip filled, nothing else touched.
            id: "balanced".into(),
            name: "Balanced".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 45, 2.5),
                peak(85, -3.0, 1.0),
                peak(300, 1.5, 1.0),
            ]),
        },
        Preset {
            id: "opus".into(),
            name: "opus5.5".into(),
            bands: pad(vec![
                peak(50, 2.5, 0.707),
                peak(120, 0.0, 1.0),
                peak(300, -1.5, 1.2),
                peak(600, -0.5, 1.4),
                peak(1000, 0.0, 1.0),
                peak(2500, 1.5, 1.4),
                peak(4981, -6.5, 0.707),
                peak(3000, 1.5, 0.707),
            ]),
        },
        Preset {
            // Competitive shooters: less rumble from explosions, more of the
            // 2-5 kHz range where footsteps and reloads sit.
            id: "footsteps".into(),
            name: "Footsteps".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 120, -4.0),
                peak(250, -2.0, 1.0),
                peak(2000, 1.5, 1.2),
                peak(3500, 3.0, 1.4),
                peak(6000, 1.5, 1.4),
            ]),
        },
        Preset {
            // Discord, podcasts, video: no rumble, less boxiness, clearer
            // consonants, softer "s" sounds.
            id: "voice".into(),
            name: "Voice".into(),
            bands: pad(vec![
                Band { enabled: true, filter: Filter::HighPass, freq: 90, gain: 0.0, q: 0.707 },
                peak(200, -2.0, 1.0),
                peak(1500, 1.5, 1.0),
                peak(3000, 3.0, 1.2),
                peak(7000, -2.0, 2.0),
            ]),
        },
        Preset {
            // Music: a little more body in the upper bass and low mids,
            // softer upper treble.
            id: "warm".into(),
            name: "Warm".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 60, 2.0),
                peak(85, -1.5, 1.2),
                peak(250, 1.5, 0.9),
                peak(6000, -1.5, 2.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ]),
        },
        Preset {
            // Music with a lot of clean low end: an even ~+7 dB shelf from 20
            // to 80 Hz over the mids with no boom peak. Most of it comes from
            // cutting everything above 160 Hz, so turn the volume up a notch.
            id: "bass-music".into(),
            name: "Bass Music".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 45, 3.0),
                peak(90, -1.5, 1.2),
                shelf(Filter::HighShelf, 160, -4.0),
                peak(350, 1.0, 1.0),
                peak(3000, 1.0, 1.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ]),
        },
        Preset {
            // Movies and story games: sub-bass rumble plus a dialogue lift.
            id: "cinema".into(),
            name: "Cinema".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 45, 3.0),
                peak(90, -1.5, 1.0),
                peak(2000, 2.0, 1.0),
                shelf(Filter::HighShelf, 9000, 1.5),
            ]),
        },
        Preset {
            // Quiet listening: ears lose bass and treble at low volume, so lift both.
            id: "late-night".into(),
            name: "Late night".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 60, 3.0),
                peak(90, -1.0, 1.0),
                peak(1000, -1.0, 0.707),
                shelf(Filter::HighShelf, 8000, 3.0),
            ]),
        },
    ]
}

/// The built-in presets as `SEED_VERSION` 0 shipped them, for the ones that
/// changed since. Those boosted up to +8.6 dB and stacked on the headset's
/// own bass peak.
fn seed_bands_v0(id: &str) -> Option<Vec<Band>> {
    let bands = match id {
        "main" => vec![
            peak(55, 4.0, 0.707),
            peak(100, 3.0, 0.707),
            peak(600, 4.0, 4.0),
            peak(5000, 2.0, 0.707),
            peak(8000, 3.0, 0.707),
            peak(12000, 3.0, 0.707),
            shelf(Filter::LowShelf, 80, 1.5),
        ],
        "footsteps" => vec![
            shelf(Filter::LowShelf, 120, -4.0),
            peak(250, -2.0, 1.0),
            peak(2000, 2.0, 1.2),
            peak(3500, 4.0, 1.4),
            peak(6000, 2.0, 1.4),
        ],
        "warm" => vec![
            shelf(Filter::LowShelf, 105, 4.5),
            peak(400, -1.0, 1.0),
            peak(2500, 1.0, 1.4),
            peak(6000, -1.5, 2.0),
            shelf(Filter::HighShelf, 10000, 1.0),
        ],
        "bass-music" => vec![
            shelf(Filter::LowShelf, 100, 6.5),
            peak(50, 2.5, 1.2),
            peak(300, -2.0, 1.0),
            peak(2500, 1.5, 1.2),
            peak(6500, -1.0, 2.0),
            shelf(Filter::HighShelf, 10000, 1.5),
        ],
        "cinema" => vec![
            shelf(Filter::LowShelf, 70, 5.0),
            peak(250, -1.5, 1.0),
            peak(2000, 2.5, 1.0),
            shelf(Filter::HighShelf, 9000, 2.0),
        ],
        "late-night" => vec![
            shelf(Filter::LowShelf, 100, 5.0),
            peak(1000, -1.0, 0.707),
            shelf(Filter::HighShelf, 8000, 3.5),
        ],
        _ => return None,
    };
    let mut bands = bands;
    bands.resize(arctis::BANDS, Band::default());
    Some(bands)
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
    /// the user edited keeps their bands as a "(yours)" copy next to it.
    /// Returns whether anything changed.
    pub fn upgrade_presets(&mut self) -> bool {
        if self.seeds >= SEED_VERSION {
            return false;
        }
        let seeds = seed_presets();
        let mut kept = Vec::new();
        for p in &mut self.presets {
            let (Some(seed), Some(old)) = (seeds.iter().find(|s| s.id == p.id), seed_bands_v0(&p.id)) else {
                continue;
            };
            if p.bands != old {
                kept.push(Preset { id: new_id() + &p.id, name: format!("{} (yours)", p.name), bands: p.bands.clone() });
            }
            p.bands = seed.bands.clone();
        }
        self.presets.extend(kept);
        // Balanced is new in version 1; it goes after Main like in the seeds.
        if !self.presets.iter().any(|p| p.id == "balanced") {
            let at = self.presets.iter().position(|p| p.id == "main").map_or(self.presets.len(), |i| i + 1);
            self.presets.insert(at, seeds.into_iter().find(|s| s.id == "balanced").unwrap());
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

    #[test]
    fn seeds_leave_headroom() {
        // Without a preamp a boost is level on top of full-scale music.
        for p in seed_presets() {
            for b in p.bands.iter().filter(|b| b.enabled) {
                assert!(b.gain <= 3.0, "{} boosts {} dB at {} Hz", p.name, b.gain, b.freq);
            }
        }
    }

    #[test]
    fn upgrade_replaces_old_seeds_and_keeps_edits() {
        let old = |id: &str, name: &str| Preset { id: id.into(), name: name.into(), bands: seed_bands_v0(id).unwrap() };
        let mut edited = old("warm", "Warm");
        edited.bands[0].gain = 2.0;
        let mut c = Config {
            presets: vec![old("main", "Main"), edited.clone(), old("cinema", "Cinema")],
            active: "main".into(),
            seeds: 0,
            ..Config::default()
        };
        c.normalize();
        assert!(c.upgrade_presets());
        let seeds = seed_presets();
        let seed = |id: &str| seeds.iter().find(|s| s.id == id).unwrap().bands.clone();
        let ids: Vec<_> = c.presets.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(&ids[..4], ["main", "balanced", "warm", "cinema"]);
        assert_eq!(c.presets[0].bands, seed("main"));
        assert_eq!(c.presets[2].bands, seed("warm"));
        assert_eq!(c.presets[4].name, "Warm (yours)");
        assert_eq!(c.presets[4].bands, edited.bands);
        assert!(!c.upgrade_presets());

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
