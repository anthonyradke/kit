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

/// Flat, the user's two Sonar game presets converted to headset bands, and a
/// few purpose-built ones. Boosts stay near +5 dB: the headset has no preamp.
fn seed_presets() -> Vec<Preset> {
    let pad = |mut bands: Vec<Band>| {
        bands.resize(arctis::BANDS, Band::default());
        bands
    };
    vec![
        Preset { id: "flat".into(), name: "Flat".into(), bands: flat_bands() },
        Preset {
            id: "main".into(),
            name: "Main".into(),
            bands: pad(vec![
                peak(55, 4.0, 0.707),
                peak(100, 3.0, 0.707),
                peak(600, 4.0, 4.0),
                peak(5000, 2.0, 0.707),
                peak(8000, 3.0, 0.707),
                peak(12000, 3.0, 0.707),
                // Sonar's "bass boost" at 1.5.
                Band { enabled: true, filter: Filter::LowShelf, freq: 80, gain: 1.5, q: 0.707 },
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
                peak(2000, 2.0, 1.2),
                peak(3500, 4.0, 1.4),
                peak(6000, 2.0, 1.4),
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
            // Music: a gentle bass shelf, otherwise close to neutral.
            id: "warm".into(),
            name: "Warm".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 105, 4.5),
                peak(400, -1.0, 1.0),
                peak(2500, 1.0, 1.4),
                peak(6000, -1.5, 2.0),
                shelf(Filter::HighShelf, 10000, 1.0),
            ]),
        },
        Preset {
            // Music with much more low end than Warm (about +8 dB from 25-100 Hz),
            // with the low mids cut so it stays clean and a vocal lift so the mix
            // isn't buried.
            id: "bass-music".into(),
            name: "Bass Music".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 100, 6.5),
                peak(50, 2.5, 1.2),
                peak(300, -2.0, 1.0),
                peak(2500, 1.5, 1.2),
                peak(6500, -1.0, 2.0),
                shelf(Filter::HighShelf, 10000, 1.5),
            ]),
        },
        Preset {
            // Movies and story games: sub-bass rumble plus a dialogue lift.
            id: "cinema".into(),
            name: "Cinema".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 70, 5.0),
                peak(250, -1.5, 1.0),
                peak(2000, 2.5, 1.0),
                shelf(Filter::HighShelf, 9000, 2.0),
            ]),
        },
        Preset {
            // Quiet listening: ears lose bass and treble at low volume, so lift both.
            id: "late-night".into(),
            name: "Late night".into(),
            bands: pad(vec![
                shelf(Filter::LowShelf, 100, 5.0),
                peak(1000, -1.0, 0.707),
                shelf(Filter::HighShelf, 8000, 3.5),
            ]),
        },
    ]
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
        config
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

/// Corner and slope of the bass slider's shelf.
pub const BASS_FREQ: u16 = 100;
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
