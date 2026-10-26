//! Lighting effects kit plays on the NK65 itself. The stock firmware only
//! takes one color and one brightness for the whole board, so kit sets a solid
//! base and then streams those two values (about 30 times a second, never
//! stored in the board's EEPROM). Some effects react to typing (the input hook
//! counts key presses) or to whatever sound is playing.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use super::config::{parse_color, to_hue_sat, Preset};

pub const KEYSTRIKE: u16 = 200;
pub const HEAT: u16 = 201;
pub const HUE_STEP: u16 = 202;
pub const MUSIC: u16 = 203;
pub const HEARTBEAT: u16 = 204;
pub const CANDLE: u16 = 205;
pub const AURORA: u16 = 206;

pub const FRAME: Duration = Duration::from_millis(33);

/// Effect numbers kit plays itself (clear of both firmwares' effect numbers).
pub fn is_kit(effect: u16) -> bool {
    (KEYSTRIKE..=AURORA).contains(&effect)
}

pub fn wants_keys(effect: u16) -> bool {
    matches!(effect, KEYSTRIKE | HEAT | HUE_STEP)
}

pub fn wants_sound(effect: u16) -> bool {
    effect == MUSIC
}

/// What the board should show this frame: brightness 0-255 and a color as
/// QMK hue and saturation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub brightness: u8,
    pub hue_sat: [u8; 2],
}

pub struct Anim {
    pub effect: u16,
    c1: [f32; 3],
    c2: [f32; 3],
    /// Resting brightness, 0-1 (the preset's brightness).
    base: f32,
    /// 0-1
    speed: f32,
    start: Instant,
    last: Instant,
    /// Envelope 0-1: a key flash, typing heat, loudness or flicker level.
    level: f32,
    /// Hue step: shown and target hue (0-1, the target runs past 1).
    hue: f32,
    target_hue: f32,
    sat: f32,
    /// Candle: where the flicker is heading and when it picks a new spot.
    flicker_to: f32,
    flicker_at: f32,
    rng: u32,
}

impl Anim {
    pub fn new(p: &Preset, now: Instant) -> Self {
        let rgb = |c: &str| parse_color(c).unwrap_or([255, 255, 255]).map(|v| v as f32 / 255.0);
        let [h, s] = to_hue_sat(parse_color(&p.color1).unwrap_or([255, 0, 0]));
        let hue = h as f32 / 255.0;
        Anim {
            effect: p.effect,
            c1: rgb(&p.color1),
            c2: rgb(&p.color2),
            base: p.brightness as f32 / 100.0,
            speed: p.speed as f32 / 100.0,
            start: now,
            last: now,
            level: if p.effect == CANDLE { 1.0 } else { 0.0 },
            hue,
            target_hue: hue,
            sat: if s < 40 { 1.0 } else { s as f32 / 255.0 },
            flicker_to: 1.0,
            flicker_at: 0.0,
            rng: 0x9E37_79B9,
        }
    }

    /// Advance to `now`, given the key presses since the last frame and how
    /// loud the sound output is (peak, 0-1).
    pub fn frame(&mut self, now: Instant, presses: u32, peak: f32) -> Frame {
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.25);
        let t = now.saturating_duration_since(self.start).as_secs_f32();
        self.last = now;
        let s = self.speed;
        // Below 1% counts as resting, so nothing more goes to the board once a flash is over.
        let decay = |level: f32, half: f32| {
            let l = level * 0.5f32.powf(dt / half);
            if l < 0.01 { 0.0 } else { l }
        };
        // Most effects rest at the preset's brightness and rise to full with the envelope.
        let (bright, rgb) = match self.effect {
            KEYSTRIKE => {
                // Each press flashes color 2 at full brightness, then it fades back.
                self.level = if presses > 0 { 1.0 } else { decay(self.level, 0.6 - 0.52 * s) };
                (self.lift(self.level), mix(self.c1, self.c2, self.level))
            }
            HEAT => {
                // Settles near 1 at about 8 presses a second; speed sets how fast it reacts and cools.
                let tau = 3.0 - 2.5 * s;
                self.level = (decay(self.level, tau * std::f32::consts::LN_2) + presses as f32 / (8.0 * tau)).min(1.0);
                (self.lift(self.level), mix(self.c1, self.c2, self.level))
            }
            HUE_STEP => {
                // Each press moves the hue along; it glides there and stays.
                self.target_hue += presses as f32 * (0.01 + 0.07 * s);
                self.hue += (self.target_hue - self.hue) * (1.0 - (-dt / 0.12).exp());
                if (self.target_hue - self.hue).abs() < 0.001 {
                    self.hue = self.target_hue;
                }
                let turns = self.hue.floor();
                self.hue -= turns;
                self.target_hue -= turns;
                self.level = if presses > 0 { 1.0 } else { decay(self.level, 0.15) };
                (self.lift(0.4 * self.level), hue_rgb(self.hue, self.sat))
            }
            MUSIC => {
                // Jumps up with the sound, falls back more slowly.
                let loud = peak.clamp(0.0, 1.0).sqrt();
                self.level = loud.max(decay(self.level, 0.35 - 0.28 * s));
                (self.lift(self.level), mix(self.c1, self.c2, self.level))
            }
            HEARTBEAT => {
                // Two beats (lub-dub), then a rest.
                let beat = (t % (1.5 - 0.9 * s)) - 0.1;
                let pulse = |c: f32| (-((beat - c) / 0.07).powi(2) / 2.0).exp();
                self.level = (pulse(0.0) + 0.6 * pulse(0.22)).min(1.0);
                (self.lift(self.level), mix(self.c1, self.c2, self.level))
            }
            CANDLE => {
                // Wanders between dim and full, now and then dipping low; dimmer means more color 2.
                if t >= self.flicker_at {
                    let r = self.random();
                    self.flicker_to = if r < 0.08 { 0.35 } else { 0.55 + 0.45 * self.random() };
                    self.flicker_at = t + (0.25 - 0.18 * s) * (0.5 + self.random());
                }
                self.level += (self.flicker_to - self.level) * (1.0 - (-dt / 0.06).exp());
                (self.base * self.level, mix(self.c2, self.c1, self.level))
            }
            AURORA => {
                // Two slow waves that never quite line up, drifting between the colors.
                let x = t / (30.0 - 26.0 * s);
                let blend = 0.5 + 0.5 * (0.6 * (TAU * x).sin() + 0.4 * (TAU * x * 2.3 + 1.3).sin());
                let swell = 0.7 + 0.3 * (0.5 + 0.5 * (TAU * x * 1.7 + 0.5).sin());
                (self.base * swell, mix(self.c1, self.c2, blend))
            }
            _ => (self.base, self.c1),
        };
        let rgb = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        Frame { brightness: (bright.clamp(0.0, 1.0) * 255.0).round() as u8, hue_sat: to_hue_sat(rgb) }
    }

    /// From the resting brightness (0) up to full (1).
    fn lift(&self, x: f32) -> f32 {
        self.base + (1.0 - self.base) * x
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1 << 24) as f32
    }
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

fn hue_rgb(h: f32, s: f32) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(effect: u16) -> Preset {
        Preset { effect, color1: "#ff0000".into(), color2: "#0000ff".into(), brightness: 20, speed: 50, ..Preset::default() }
    }

    #[test]
    fn keystrike_flashes_and_fades() {
        let t0 = Instant::now();
        let mut a = Anim::new(&preset(KEYSTRIKE), t0);
        let rest = a.frame(t0, 0, 0.0);
        assert_eq!(rest.brightness, 51);
        let hit = a.frame(t0 + FRAME, 1, 0.0);
        assert_eq!(hit.brightness, 255);
        assert_eq!(hit.hue_sat, to_hue_sat([0, 0, 255]));
        let mut f = hit;
        for i in 2..120 {
            f = a.frame(t0 + FRAME * i, 0, 0.0);
        }
        assert_eq!(f, rest);
    }

    #[test]
    fn heat_rises_with_typing() {
        let t0 = Instant::now();
        let mut a = Anim::new(&preset(HEAT), t0);
        let mut f = a.frame(t0, 0, 0.0);
        // About 10 presses a second for 5 seconds.
        for i in 1..150 {
            f = a.frame(t0 + FRAME * i, (i % 3 == 0) as u32, 0.0);
        }
        assert!(f.brightness > 200, "only reached {}", f.brightness);
    }

    #[test]
    fn ambient_effects_move_on_their_own() {
        let t0 = Instant::now();
        for effect in [HEARTBEAT, CANDLE, AURORA] {
            let mut a = Anim::new(&Preset { speed: 100, ..preset(effect) }, t0);
            let mut seen: Vec<u8> = (0..300).map(|i| a.frame(t0 + FRAME * i, 0, 0.0).brightness).collect();
            seen.sort_unstable();
            seen.dedup();
            assert!(seen.len() > 5, "effect {effect} barely moves: {seen:?}");
        }
        assert!(is_kit(AURORA) && !is_kit(44) && !is_kit(1));
    }
}
