//! The Nova 5's ChatMix dial. Sonar splits sound into a Game and a Chat
//! device and the dial sets their volumes. kit has neither, so it sets the
//! volume of each app on the headset (and on VB-CABLE with surround on)
//! instead: the apps on the chat list follow the dial's chat side, every
//! other app its game side.
//!
//! The volumes are set outright, not scaled from what they were, so they
//! can't drift if kit ever ends without putting them back. Turning ChatMix
//! off, the headset going away and quitting all put every app kit changed
//! back to full volume.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// How often new apps are looked for while the dial stays put.
const SWEEP: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Mixer {
    /// The dial position last applied, game then chat, 0-100.
    applied: Option<[u8; 2]>,
    /// Endpoint id and process id of every app whose volume kit set.
    touched: HashSet<(String, u32)>,
    next_sweep: Option<Instant>,
}

impl Mixer {
    /// Follow the dial; `None` (switched off, or no headset) gives every app
    /// back its full volume.
    pub fn update(&mut self, dial: Option<[u8; 2]>, chat_apps: &[String], surround: bool) {
        let Some(dial) = dial else {
            self.restore();
            return;
        };
        let now = Instant::now();
        if self.applied == Some(dial) && self.next_sweep.is_some_and(|t| now < t) {
            return;
        }
        self.applied = Some(dial);
        self.next_sweep = Some(now + SWEEP);
        let names: HashMap<u32, String> = crate::setup::processes().into_iter().collect();
        let mut endpoints = Vec::new();
        if let Ok(Some(out)) = sysaudio::find_arctis(sysaudio::Flow::Output) {
            endpoints.push(out.id);
        }
        if surround {
            if let Ok(Some(cable)) = sysaudio::find_cable() {
                endpoints.push(cable.id);
            }
        }
        let touched = &mut self.touched;
        for id in endpoints {
            let _ = sysaudio::set_session_volumes(&id, |pid, volume| {
                let chat = names.get(&pid).is_some_and(|n| chat_apps.iter().any(|a| a == n));
                let level = f32::from(if chat { dial[1] } else { dial[0] }) / 100.0;
                touched.insert((id.clone(), pid));
                ((volume - level).abs() > 0.004).then_some(level)
            });
        }
    }

    /// Put every app kit changed back to full volume.
    pub fn restore(&mut self) {
        self.applied = None;
        self.next_sweep = None;
        if self.touched.is_empty() {
            return;
        }
        let endpoints: HashSet<String> = self.touched.iter().map(|(id, _)| id.clone()).collect();
        for id in endpoints {
            let _ = sysaudio::set_session_volumes(&id, |pid, volume| {
                (self.touched.contains(&(id.clone(), pid)) && volume < 1.0).then_some(1.0)
            });
        }
        self.touched.clear();
    }
}

/// Apps that count as chat unless the user changes the list.
pub fn default_chat_apps() -> Vec<String> {
    ["discord.exe", "discordptb.exe", "discordcanary.exe", "ts3client_win64.exe", "teamspeak.exe", "mumble.exe", "slack.exe", "ms-teams.exe", "teams.exe", "zoom.exe", "skype.exe", "whatsapp.exe", "telegram.exe", "signal.exe", "guilded.exe"]
        .map(String::from)
        .to_vec()
}

/// An app name as the list keeps it: the lowercase executable name.
pub fn normalize_app(name: &str) -> Option<String> {
    let name = name.trim().rsplit(['\\', '/']).next()?.to_lowercase();
    if name.is_empty() || name.len() > 64 || name.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) {
        return None;
    }
    Some(if name.ends_with(".exe") { name } else { format!("{name}.exe") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_are_executable_names() {
        assert_eq!(normalize_app(" Discord ").as_deref(), Some("discord.exe"));
        assert_eq!(normalize_app(r"C:\Program Files\Zoom\Zoom.exe").as_deref(), Some("zoom.exe"));
        assert_eq!(normalize_app(""), None);
        assert_eq!(normalize_app("a<b"), None);
    }
}
