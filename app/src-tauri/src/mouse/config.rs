//! Profiles, macros and settings, persisted as JSON in the app config dir.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const DPI_MIN: u16 = 100;
pub const DPI_MAX: u16 = 25600;
pub const DPI_STEP: u16 = 50;
pub const MAX_STAGES: usize = 5;
pub const RATES: [u16; 4] = [125, 250, 500, 1000];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub active: String,
    pub profiles: Vec<Profile>,
    pub macros: Vec<Macro>,
    pub settings: Settings,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub next_profile_hotkey: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub dpi_stages: Vec<u16>,
    /// Stage used when the profile is activated.
    pub dpi_index: usize,
    pub report_rate: u16,
    pub back: Action,
    pub forward: Action,
    pub hotkey: Option<String>,
}

/// What a side button does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    /// Normal Back / Forward behaviour.
    #[default]
    Default,
    Disabled,
    /// Hold a key combination while the button is held.
    Keys { keys: Vec<KeyRef> },
    Macro { id: String },
    /// Temporarily switch to this DPI while held (sniper button).
    DpiShift { dpi: u16 },
    DpiCycle,
    NextProfile,
    Profile { id: String },
    /// The NK65's profiles (kit-run keys on the board).
    NextKeyProfile,
    KeyProfile { id: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRef {
    pub vk: u16,
    pub scan: u16,
    pub ext: bool,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Step {
    Key { key: KeyRef, down: bool },
    Mouse { button: MouseButton, down: bool },
    Delay { ms: u32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayMode {
    /// Play `repeat` times per press.
    #[default]
    Once,
    /// Loop while the button is held.
    WhileHeld,
    /// Press to start looping, press again to stop.
    Toggle,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Macro {
    pub id: String,
    pub name: String,
    pub mode: PlayMode,
    pub repeat: u32,
    pub steps: Vec<Step>,
}

impl Default for Macro {
    fn default() -> Self {
        Self { id: new_id(), name: "New macro".into(), mode: PlayMode::Once, repeat: 1, steps: vec![] }
    }
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "Default".into(),
            dpi_stages: vec![400, 800, 1600],
            dpi_index: 1,
            report_rate: 1000,
            back: Action::Default,
            forward: Action::Default,
            hotkey: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let profile = Profile::default();
        Self { active: profile.id.clone(), profiles: vec![profile], macros: vec![], settings: Settings::default() }
    }
}

pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}{:x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

pub fn clamp_dpi(dpi: u16) -> u16 {
    let dpi = dpi.clamp(DPI_MIN, DPI_MAX);
    (dpi + DPI_STEP / 2) / DPI_STEP * DPI_STEP
}

impl Config {
    pub fn load(path: &Path) -> Self {
        let mut config = match std::fs::read_to_string(path) {
            Err(_) => Config::default(),
            Ok(text) => match serde_json::from_str::<Config>(text.trim_start_matches('\u{feff}')) {
                Ok(c) => c,
                Err(e) => {
                    // Keep the unreadable file instead of overwriting the user's profiles.
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
        if self.profiles.is_empty() {
            self.profiles.push(Profile::default());
        }
        for p in &mut self.profiles {
            if p.id.is_empty() {
                p.id = new_id();
            }
            p.dpi_stages.iter_mut().for_each(|d| *d = clamp_dpi(*d));
            p.dpi_stages.truncate(MAX_STAGES);
            if p.dpi_stages.is_empty() {
                p.dpi_stages.push(800);
            }
            p.dpi_index = p.dpi_index.min(p.dpi_stages.len() - 1);
            if !RATES.contains(&p.report_rate) {
                p.report_rate = 1000;
            }
            for action in [&mut p.back, &mut p.forward] {
                if let Action::DpiShift { dpi } = action {
                    *dpi = clamp_dpi(*dpi);
                }
            }
            if p.hotkey.as_deref().is_some_and(str::is_empty) {
                p.hotkey = None;
            }
        }
        for m in &mut self.macros {
            if m.id.is_empty() {
                m.id = new_id();
            }
            m.repeat = m.repeat.clamp(1, 1000);
        }
        // Bindings to a deleted profile or macro would silently do nothing.
        // (An empty macro id is "Macro" chosen before any macro exists.)
        let profile_ids: Vec<String> = self.profiles.iter().map(|p| p.id.clone()).collect();
        let macro_ids: Vec<String> = self.macros.iter().map(|m| m.id.clone()).collect();
        for p in &mut self.profiles {
            for action in [&mut p.back, &mut p.forward] {
                let stale = match action {
                    Action::Profile { id } => !profile_ids.contains(id),
                    Action::Macro { id } => !id.is_empty() && !macro_ids.contains(id),
                    _ => false,
                };
                if stale {
                    *action = Action::Default;
                }
            }
        }
        if !self.profiles.iter().any(|p| p.id == self.active) {
            self.active = self.profiles[0].id.clone();
        }
    }

    pub fn active_profile(&self) -> &Profile {
        self.profiles.iter().find(|p| p.id == self.active).unwrap_or(&self.profiles[0])
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn macro_by_id(&self, id: &str) -> Option<&Macro> {
        self.macros.iter().find(|m| m.id == id)
    }
}
