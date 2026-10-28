//! Lighting presets and key bindings, persisted as JSON in the app config dir.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::mouse::config::{new_id, Action};

/// Kit-run macros on the NK65 go out as F13..F24, which the input hook catches.
pub const KIT_KEYS: usize = 12;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Model {
    #[default]
    Nk65,
    ProXTkl,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    /// Which keyboard the presets were made for; they are reseeded on a change.
    pub model: Model,
    pub presets: Vec<Preset>,
    pub active: String,
    /// PRO X TKL: seconds without typing before the lights go off (battery).
    pub idle_off: u16,
    /// Keys handled by kit's input hook, by HID usage (PRO X TKL).
    pub bindings: Vec<Binding>,
    /// NK65: macro played by kit when the board sends F13 + index.
    pub kit_keys: Vec<Option<String>>,
    /// NK65: onboard macro slots kit writes, slot -> macro id.
    pub onboard: BTreeMap<u8, String>,
    /// NK65: the lighting already on the board was taken in as a preset
    /// (done once, so kit's first run doesn't overwrite it).
    pub adopted: bool,
    /// Effects starred in the effect picker, by the picker's key ("q1", "k6", "a203", ...).
    pub favorites: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// NK65: the firmware's effect number. PRO X TKL: the HID++ effect id
    /// (0 off, 1 static, 3 cycle, 4 wave, 10 breathe, 11 ripple).
    pub effect: u16,
    /// "#rrggbb"
    pub color1: String,
    pub color2: String,
    /// 0..=100
    pub speed: u8,
    /// 0..=100
    pub brightness: u8,
    /// NK65 kit effects (firmware 4 on): how lit the board stays between
    /// presses or in an effect's dark parts, 0..=100.
    pub background: u8,
    /// PRO X TKL per-key paint over Static, zone id -> "#rrggbb".
    pub keys: BTreeMap<u8, String>,
    /// NK65 (original PCB) indicator lights: caps lock, layers 1-3. `None` = off.
    pub indicators: [Option<String>; 4],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub usage: u8,
    pub action: Action,
}

impl Default for Config {
    fn default() -> Self {
        Config::for_model(Model::Nk65)
    }
}

impl Default for Preset {
    fn default() -> Self {
        Preset {
            id: new_id(),
            name: "New lighting".into(),
            effect: 1,
            color1: "#ffffff".into(),
            color2: "#ffffff".into(),
            speed: 50,
            brightness: 80,
            background: 0,
            keys: BTreeMap::new(),
            indicators: Default::default(),
        }
    }
}

fn preset(name: &str, effect: u16, color1: &str, color2: &str) -> Preset {
    Preset { name: name.into(), effect, color1: color1.into(), color2: color2.into(), ..Preset::default() }
}

impl Config {
    pub fn for_model(model: Model) -> Self {
        let presets = match model {
            // Effect numbers of the original NK65's firmware; on rev B they
            // name RGB-matrix modes (1 solid, 2 alphas/mods, 12 cycle all), close enough.
            Model::Nk65 => vec![
                preset("Milkshake", 2, "#ffb3d1", "#9fe8cf"),
                preset("Vanilla", 1, "#fff1dc", "#fff1dc"),
                preset("Rainbow", 5, "#ffffff", "#ffffff"),
                Preset { brightness: 0, ..preset("Dark", 0, "#ffffff", "#ffffff") },
            ],
            Model::ProXTkl => vec![
                preset("White", 1, "#ffffff", "#ffffff"),
                preset("Breathe", 10, "#4aa8ff", "#4aa8ff"),
                preset("Cycle", 3, "#ffffff", "#ffffff"),
                Preset { brightness: 0, ..preset("Dark", 0, "#ffffff", "#ffffff") },
            ],
        };
        Config {
            model,
            active: presets[0].id.clone(),
            presets,
            idle_off: 300,
            bindings: vec![],
            kit_keys: vec![None; KIT_KEYS],
            onboard: BTreeMap::new(),
            adopted: false,
            favorites: vec![],
        }
    }

    pub fn load(path: &Path) -> Self {
        let mut config = match std::fs::read_to_string(path) {
            Err(_) => Config::default(),
            Ok(text) => serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')).unwrap_or_else(|e| {
                eprintln!("keyboard config unreadable ({e}); backing it up and starting fresh");
                let _ = std::fs::copy(path, path.with_extension("json.bak"));
                Config::default()
            }),
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
            let fresh = Config::for_model(self.model);
            self.presets = fresh.presets;
        }
        for p in &mut self.presets {
            if p.id.is_empty() {
                p.id = new_id();
            }
            p.speed = p.speed.min(100);
            p.brightness = p.brightness.min(100);
            for c in [&mut p.color1, &mut p.color2] {
                if parse_color(c).is_none() {
                    *c = "#ffffff".into();
                }
            }
            p.keys.retain(|_, c| parse_color(c).is_some());
            for slot in &mut p.indicators {
                if slot.as_deref().is_some_and(|c| parse_color(c).is_none()) {
                    *slot = None;
                }
            }
        }
        if !self.presets.iter().any(|p| p.id == self.active) {
            self.active = self.presets[0].id.clone();
        }
        self.idle_off = self.idle_off.clamp(30, 3600);
        self.kit_keys.resize(KIT_KEYS, None);
        self.bindings.retain(|b| b.action != Action::Default);
        self.bindings.dedup_by_key(|b| b.usage);
    }

    /// Forget macros that were deleted.
    pub fn drop_missing_macros(&mut self, exists: impl Fn(&str) -> bool) {
        for k in &mut self.kit_keys {
            if k.as_deref().is_some_and(|id| !exists(id)) {
                *k = None;
            }
        }
        self.onboard.retain(|_, id| exists(id));
        self.bindings.retain(|b| !matches!(&b.action, Action::Macro { id } if !id.is_empty() && !exists(id)));
    }

    pub fn active_preset(&self) -> &Preset {
        self.presets.iter().find(|p| p.id == self.active).unwrap_or(&self.presets[0])
    }
}

pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// "#rrggbb" for a QMK hue and saturation at full value.
pub fn from_hue_sat([h, s]: [u8; 2]) -> String {
    let (h, s) = (h as f32 / 255.0 * 6.0, s as f32 / 255.0);
    let f = |n: f32| {
        let k = (n + h) % 6.0;
        1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    let [r, g, b] = [f(5.0), f(3.0), f(1.0)].map(|v| (v * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Hue and saturation on QMK's 0-255 scale (VIA colors carry no value).
pub fn to_hue_sat([r, g, b]: [u8; 3]) -> [u8; 2] {
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let sat = if max > 0.0 { d / max } else { 0.0 };
    let hue = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    [(hue * 255.0).round() as u8, (sat * 255.0).round() as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_sat() {
        assert_eq!(to_hue_sat([255, 0, 0]), [0, 255]);
        assert_eq!(to_hue_sat([0, 255, 0]), [85, 255]);
        assert_eq!(to_hue_sat([255, 255, 255]), [0, 0]);
        assert_eq!(parse_color("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_color("ff8000"), None);
        assert_eq!(from_hue_sat([0, 255]), "#ff0000");
        assert_eq!(from_hue_sat([0, 0]), "#ffffff");
        for c in ["#ff0000", "#00ff00", "#ffb3d1", "#4a6cff"] {
            let back = from_hue_sat(to_hue_sat(parse_color(c).unwrap()));
            let [a, b] = [parse_color(c).unwrap(), parse_color(&back).unwrap()];
            // Only hue and saturation survive; a full-value color comes back close.
            assert!(a.iter().zip(b).all(|(x, y)| (*x as i16 - y as i16).abs() <= 3), "{c} -> {back}");
        }
    }
}
