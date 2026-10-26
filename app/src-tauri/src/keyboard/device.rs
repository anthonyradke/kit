//! Background thread that owns the keyboard: the NK65 over VIA's raw HID
//! interface, or the PRO X TKL over HID++.
//!
//! The NK65 keeps its keymap, macros and lighting in its own EEPROM, so on
//! connect the worker reads them and only writes lighting values that differ.
//! kit's own NK65 effects (`anim`) stream color and brightness over a solid
//! base that is what gets stored.
//! The PRO X TKL's lighting is driven by kit while it runs: kit takes the LEDs
//! from the onboard profile, paints them, turns them off when the keyboard
//! reports idle (it runs on a battery) and hands them back on Quit.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::anim::{self, Anim};
use super::config::{parse_color, to_hue_sat, Model, Preset};
use crate::mouse::input;
use super::state::Shared;

pub enum Cmd {
    /// The keyboard chosen in setup (or none).
    Model(Option<Model>),
    /// Lighting to show; `save` stores it on an NK65 (not while dragging a slider).
    Apply { preset: Box<Preset>, idle_off: u16, save: bool },
    SetKey { layer: u8, row: u8, col: u8, code: u16 },
    /// Onboard macro slots kit owns, already encoded. Other slots are kept.
    Macros(BTreeMap<u8, Vec<u8>>),
    /// kit is quitting: give the PRO X TKL's lights back to its onboard profile.
    Release(std::sync::mpsc::Sender<()>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub present: bool,
    pub connected: bool,
    pub name: Option<String>,
    pub battery: Option<u8>,
    pub charging: bool,
    /// "wilba", "rgbMatrix" (NK65) or "lightsync" (PRO X TKL).
    pub lighting: Option<&'static str>,
    /// NK65: VIA protocol, layers, matrix and every keycode (layer, row, col order).
    pub protocol: Option<u16>,
    pub layers: u8,
    pub rows: u8,
    pub cols: u8,
    pub keymap: Vec<u16>,
    /// NK65: the onboard macros as they are on the board.
    pub macros: Vec<Vec<MacroStep>>,
    pub macro_space: usize,
    /// PRO X TKL: per-key zone ids and the effect ids it offers.
    pub zones: Vec<u8>,
    pub effects: Vec<u16>,
    /// The lights are off because the keyboard is idle.
    pub lights_asleep: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MacroStep {
    Text { text: String },
    Tap { code: u16 },
    Down { code: u16 },
    Up { code: u16 },
    Delay { ms: u32 },
}

impl From<via::macros::Action> for MacroStep {
    fn from(a: via::macros::Action) -> Self {
        use via::macros::Action as A;
        match a {
            A::Text(text) => MacroStep::Text { text },
            A::Tap(code) => MacroStep::Tap { code },
            A::Down(code) => MacroStep::Down { code },
            A::Up(code) => MacroStep::Up { code },
            A::Delay(ms) => MacroStep::Delay { ms },
        }
    }
}

const OPEN_RETRY: Duration = Duration::from_secs(2);
const VIA_POLL: Duration = Duration::from_secs(2);
const BATTERY_EVERY: Duration = Duration::from_secs(60);
const APPLY_RETRY: Duration = Duration::from_secs(2);
const EVENT_WAIT_MS: i32 = 20;
const EVENT_WAIT_ASLEEP_MS: i32 = 100;
/// The default output can change; the sound meter is reopened this often.
const METER_REOPEN: Duration = Duration::from_secs(5);

pub fn spawn(app: AppHandle, rx: Receiver<Cmd>, model: Option<Model>) {
    std::thread::Builder::new()
        .name("keyboard".into())
        .spawn(move || Worker::new(app, rx, model).run())
        .expect("spawn keyboard thread");
}

struct Logi {
    dev: hidpp::Device,
    zones: Vec<hidpp::RgbZone>,
    rgb_index: Option<u8>,
    wireless_index: Option<u8>,
}

enum Dev {
    Via(via::Board),
    Logi(Box<Logi>),
}

struct Worker {
    app: AppHandle,
    rx: Receiver<Cmd>,
    model: Option<Model>,
    dev: Option<Dev>,
    desired: Option<(Preset, u16)>,
    /// The desired lighting still has to be sent.
    dirty: bool,
    /// NK65: store it in EEPROM when sent.
    save: bool,
    /// NK65: lighting values last read from or written to the board.
    via_values: BTreeMap<u8, [u8; 2]>,
    /// NK65: the raw macro buffer as on the board.
    macro_buffer: Vec<u8>,
    /// NK65: a kit effect playing, the key press count it last saw and the
    /// sound meter (Music).
    anim: Option<Anim>,
    presses: u32,
    meter: Option<sysaudio::PeakMeter>,
    next_meter: Instant,
    next_frame: Instant,
    next_open: Instant,
    next_poll: Instant,
    next_battery: Instant,
    next_apply: Instant,
    status: Status,
}

impl Worker {
    fn new(app: AppHandle, rx: Receiver<Cmd>, model: Option<Model>) -> Self {
        let now = Instant::now();
        Worker {
            app,
            rx,
            model,
            dev: None,
            desired: None,
            dirty: false,
            save: false,
            via_values: BTreeMap::new(),
            macro_buffer: vec![],
            anim: None,
            presses: 0,
            meter: None,
            next_meter: now,
            next_frame: now,
            next_open: now,
            next_poll: now,
            next_battery: now,
            next_apply: now,
            status: Status::default(),
        }
    }

    fn run(mut self) {
        let mut shown = self.status.clone();
        loop {
            loop {
                match self.rx.try_recv() {
                    Ok(cmd) => self.handle(cmd),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }
            if self.dev.is_none() && self.model.is_some() && Instant::now() >= self.next_open {
                self.open();
            }
            if self.dirty && Instant::now() >= self.next_apply {
                self.apply();
            }
            if self.status != shown {
                self.publish();
                shown = self.status.clone();
            }
            match &self.dev {
                Some(Dev::Logi(_)) => self.service_logi(),
                Some(Dev::Via(_)) => {
                    if Instant::now() >= self.next_poll {
                        self.poll_via();
                    }
                    if self.anim.is_some() {
                        if Instant::now() >= self.next_frame {
                            self.animate();
                        }
                        self.wait(self.next_poll.min(self.next_frame));
                    } else {
                        self.wait(self.next_poll);
                    }
                }
                None => self.wait(self.next_open),
            }
        }
    }

    /// Sleep until `until` or the next command.
    fn wait(&mut self, until: Instant) {
        // Pending lighting only matters once a keyboard is open (opening re-sends it).
        let until = if self.dirty && self.dev.is_some() { until.min(self.next_apply) } else { until };
        let wait = if self.model.is_none() { Duration::from_secs(3600) } else { until.saturating_duration_since(Instant::now()) };
        match self.rx.recv_timeout(wait) {
            Ok(cmd) => self.handle(cmd),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Model(model) => {
                if model != self.model {
                    self.release();
                    self.drop_dev(None);
                    self.model = model;
                    self.next_open = Instant::now();
                }
            }
            Cmd::Apply { preset, idle_off, save } => {
                self.desired = Some((*preset, idle_off));
                self.dirty = true;
                self.save |= save;
                self.next_apply = Instant::now();
            }
            Cmd::SetKey { layer, row, col, code } => {
                let ok = self.with_via(|b| b.set_keycode(layer, row, col, code)).is_some();
                if ok {
                    let s = &mut self.status;
                    let i = (layer as usize * s.rows as usize + row as usize) * s.cols as usize + col as usize;
                    if let Some(k) = s.keymap.get_mut(i) {
                        *k = code;
                    }
                }
            }
            Cmd::Macros(owned) => self.write_macros(owned),
            Cmd::Release(done) => {
                self.release();
                let _ = done.send(());
            }
        }
    }

    fn open(&mut self) {
        match self.model {
            Some(Model::Nk65) => self.open_via(),
            Some(Model::ProXTkl) => self.open_logi(),
            None => {}
        }
        if self.dev.is_none() {
            self.next_open = Instant::now() + OPEN_RETRY;
        }
    }

    fn drop_dev(&mut self, error: Option<String>) {
        self.set_anim(None);
        self.dev = None;
        self.via_values.clear();
        self.status = Status { error, ..Status::default() };
        self.next_open = Instant::now() + OPEN_RETRY;
        if self.desired.is_some() {
            self.dirty = true;
        }
    }

    fn publish(&self) {
        let shared = self.app.state::<Shared>();
        *shared.status.lock().unwrap() = self.status.clone();
        let _ = self.app.emit("keyboard", &self.status);
        crate::tray::refresh_tooltip(&self.app);
    }

    fn apply(&mut self) {
        match &self.dev {
            Some(Dev::Via(_)) => self.apply_via(),
            Some(Dev::Logi(_)) => self.apply_logi(),
            None => {}
        }
    }

    // ---------- NK65 ----------

    fn open_via(&mut self) {
        let mut board = match via::Board::open() {
            Ok(b) => b,
            Err(e) => {
                self.status = Status { error: Some(e.to_string()), ..Status::default() };
                return;
            }
        };
        let info = board.info;
        let read = (|| -> via::Result<_> {
            let layers = board.layer_count()?;
            let keymap = board.keymap(layers)?;
            Ok((layers, keymap))
        })();
        let (layers, keymap) = match read {
            Ok(v) => v,
            Err(e) => {
                self.status = Status { present: true, error: Some(e.to_string()), ..Status::default() };
                return;
            }
        };
        self.status = Status {
            present: true,
            connected: true,
            name: Some(info.name.to_string()),
            lighting: Some(match info.lighting {
                via::Lighting::Wilba => "wilba",
                via::Lighting::RgbMatrix => "rgbMatrix",
            }),
            protocol: Some(board.protocol),
            layers,
            rows: info.rows,
            cols: info.cols,
            keymap,
            ..Status::default()
        };
        self.dev = Some(Dev::Via(board));
        self.read_macros();
        self.read_via_lighting();
        // First time: keep what VIA set (state sends it back as the active preset).
        // Otherwise put the active preset on the board and store it there.
        let kind = info.lighting;
        if super::state::adopt_board_lighting(&self.app, kind, &self.via_values) {
            self.desired = None;
        }
        self.dirty = self.desired.is_some();
        self.save = true;
        self.next_poll = Instant::now() + VIA_POLL;
    }

    fn with_via<T>(&mut self, f: impl FnOnce(&mut via::Board) -> via::Result<T>) -> Option<T> {
        let Some(Dev::Via(board)) = self.dev.as_mut() else { return None };
        match f(board) {
            Ok(v) => Some(v),
            Err(via::Error::Hid(e)) => {
                self.drop_dev(Some(e.to_string()));
                None
            }
            Err(e) => {
                self.status.error = Some(e.to_string());
                None
            }
        }
    }

    /// A cheap request so an unplugged board is noticed.
    fn poll_via(&mut self) {
        self.next_poll = Instant::now() + VIA_POLL;
        if let Some(Dev::Via(board)) = self.dev.as_mut() {
            match board.layer_count() {
                Ok(_) => {}
                Err(via::Error::Hid(e)) => self.drop_dev(Some(e.to_string())),
                Err(via::Error::Timeout) => self.drop_dev(Some("the keyboard stopped answering".into())),
                Err(_) => {}
            }
        }
    }

    fn read_macros(&mut self) {
        let Some((count, buffer)) = self.with_via(|b| Ok((b.macro_count()?, b.macro_buffer()?))) else { return };
        self.status.macro_space = buffer.len();
        self.status.macros =
            via::macros::split(&buffer, count as usize).iter().map(|m| via::macros::decode(m).into_iter().map(Into::into).collect()).collect();
        self.macro_buffer = buffer;
    }

    fn write_macros(&mut self, owned: BTreeMap<u8, Vec<u8>>) {
        if !matches!(self.dev, Some(Dev::Via(_))) || self.macro_buffer.is_empty() {
            return;
        }
        let count = self.status.macros.len();
        let mut slots = via::macros::split(&self.macro_buffer, count);
        for (slot, bytes) in owned {
            if let Some(s) = slots.get_mut(slot as usize) {
                *s = bytes;
            }
        }
        let Some(buffer) = via::macros::join(&slots, self.macro_buffer.len()) else {
            self.status.error = Some("Those macros don't fit in the keyboard's macro memory.".into());
            return;
        };
        if buffer == self.macro_buffer {
            return;
        }
        if self.with_via(|b| b.set_macro_buffer(&buffer)).is_some() {
            self.read_macros();
        }
    }

    fn lighting_kind(&self) -> Option<via::Lighting> {
        match &self.dev {
            Some(Dev::Via(b)) => Some(b.info.lighting),
            _ => None,
        }
    }

    fn read_via_lighting(&mut self) {
        let Some(kind) = self.lighting_kind() else { return };
        for value in via_values(kind) {
            if let Some(v) = self.with_via(|b| b.get_value(value)) {
                self.via_values.insert(value as u8, v);
            }
        }
    }

    fn apply_via(&mut self) {
        self.dirty = false;
        let Some((p, _)) = self.desired.clone() else { return };
        let wrote = self.write_via(&p);
        let kit = anim::is_kit(p.effect);
        // A kit effect's base may already be on the board while EEPROM holds another preset.
        if (wrote || kit) && self.save {
            self.save = false;
            let _ = self.with_via(|b| b.save_lighting());
        }
        self.set_anim(kit.then(|| Anim::new(&p, Instant::now())));
    }

    /// Write the values `p` needs that differ from the board's. True if any did.
    fn write_via(&mut self, p: &Preset) -> bool {
        let Some(kind) = self.lighting_kind() else { return false };
        let mut wrote = false;
        for (value, data) in via_target(kind, p) {
            if self.via_values.get(&(value as u8)) == Some(&data) {
                continue;
            }
            // Single-byte values only take the first byte.
            let len = if matches!(value, via::Value::Brightness | via::Value::Effect | via::Value::Speed) { 1 } else { 2 };
            if self.with_via(|b| b.set_value(value, &data[..len])).is_some() {
                self.via_values.insert(value as u8, data);
                wrote = true;
            }
        }
        wrote
    }

    /// Start or stop a kit effect, with the key counting and sound meter it needs.
    fn set_anim(&mut self, next: Option<Anim>) {
        let effect = next.as_ref().map(|a| a.effect);
        let keys = effect.is_some_and(anim::wants_keys);
        input::set_key_watch(keys);
        self.presses = input::key_presses();
        if !effect.is_some_and(anim::wants_sound) {
            self.meter = None;
        }
        self.anim = next;
        self.next_frame = Instant::now();
    }

    /// One frame of the kit effect: only values that changed go to the board.
    fn animate(&mut self) {
        let now = Instant::now();
        self.next_frame = now + anim::FRAME;
        let Some(effect) = self.anim.as_ref().map(|a| a.effect) else { return };
        let count = input::key_presses();
        let presses = count.wrapping_sub(self.presses);
        self.presses = count;
        let mut peak = 0.0;
        if anim::wants_sound(effect) {
            if now >= self.next_meter {
                self.next_meter = now + METER_REOPEN;
                self.meter = sysaudio::PeakMeter::open().ok();
            }
            peak = self.meter.as_ref().and_then(|m| m.peak().ok()).unwrap_or(0.0);
        }
        let Some(a) = self.anim.as_mut() else { return };
        let frame = a.frame(now, presses, peak);
        for (value, data) in [(via::Value::Brightness, [frame.brightness, 0]), (via::Value::Color1, frame.hue_sat)] {
            if self.via_values.get(&(value as u8)) == Some(&data) {
                continue;
            }
            let len = if value == via::Value::Brightness { 1 } else { 2 };
            if self.with_via(|b| b.set_value(value, &data[..len])).is_none() {
                return;
            }
            self.via_values.insert(value as u8, data);
        }
    }

    // ---------- PRO X TKL ----------

    fn open_logi(&mut self) {
        let dev = match hidpp::Device::open(hidpp::Kind::Keyboard) {
            Ok(d) => d,
            Err(e) => {
                let present = !matches!(e, hidpp::Error::NoInterface);
                self.status = Status { present, error: Some(e.to_string()), ..Status::default() };
                return;
            }
        };
        let mut logi = Box::new(Logi { dev, zones: vec![], rgb_index: None, wireless_index: None });
        logi.wireless_index = logi.dev.feature_index(hidpp::feature::WIRELESS_STATUS).ok();
        logi.rgb_index = logi.dev.feature_index(hidpp::feature::RGB_EFFECTS).ok();
        let name = logi.dev.name().ok();
        self.status = Status { present: true, connected: true, name, lighting: Some("lightsync"), ..Status::default() };
        self.dev = Some(Dev::Logi(logi));
        self.on_wake();
    }

    /// The keyboard (re)connected: take the lights and paint them again.
    fn on_wake(&mut self) {
        let Some(Dev::Logi(l)) = self.dev.as_mut() else { return };
        if l.zones.is_empty() {
            if let Ok(zones) = l.dev.rgb_zones() {
                l.zones = zones;
            }
            let mut effects: Vec<u16> = l.zones.iter().flat_map(|z| z.effects.iter().map(|e| e.id)).collect();
            effects.sort_unstable();
            effects.dedup();
            self.status.effects = effects;
            if let Ok(zones) = l.dev.per_key_zones() {
                self.status.zones = zones;
            }
        }
        self.status.lights_asleep = false;
        self.dirty = self.desired.is_some();
        self.next_apply = Instant::now();
        self.next_battery = Instant::now();
    }

    fn with_logi<T>(&mut self, f: impl FnOnce(&mut Logi) -> hidpp::Result<T>) -> Option<T> {
        let Some(Dev::Logi(l)) = self.dev.as_mut() else { return None };
        match f(l) {
            Ok(v) => {
                if !self.status.connected {
                    self.status.connected = true;
                    self.on_wake();
                }
                self.status.error = None;
                Some(v)
            }
            Err(hidpp::Error::Hid(e)) => {
                self.drop_dev(Some(e.to_string()));
                None
            }
            Err(e @ hidpp::Error::Timeout) => {
                self.status.connected = false;
                self.status.error = Some(e.to_string());
                None
            }
            Err(e) => {
                self.status.error = Some(e.to_string());
                None
            }
        }
    }

    fn service_logi(&mut self) {
        if Instant::now() >= self.next_battery && self.status.connected {
            self.next_battery = Instant::now() + BATTERY_EVERY;
            if let Some((pct, charging)) = self.with_logi(|l| l.dev.battery()) {
                self.status.battery = Some(pct);
                self.status.charging = charging;
            }
        }
        let wait = if self.status.connected { EVENT_WAIT_MS } else { EVENT_WAIT_ASLEEP_MS };
        let Some(Dev::Logi(l)) = self.dev.as_mut() else { return };
        let (rgb, wireless) = (l.rgb_index, l.wireless_index);
        match l.dev.read_event(wait) {
            Ok(Some((index, function, params))) if Some(index) == rgb && function >> 4 == 1 => {
                // onUserActivity: 0 = idle, anything else = typing again.
                self.set_lights_awake(params[0] != 0);
            }
            Ok(Some((index, _, _))) if Some(index) == wireless => {
                self.status.connected = true;
                self.on_wake();
            }
            Ok(_) => {}
            Err(e) => self.drop_dev(Some(e.to_string())),
        }
        // Commands are drained at the top of the loop.
        std::thread::yield_now();
    }

    fn set_lights_awake(&mut self, awake: bool) {
        if awake == !self.status.lights_asleep {
            return;
        }
        if awake {
            self.status.lights_asleep = false;
            let _ = self.with_logi(|l| {
                l.dev.set_rgb_power(true)?;
                l.dev.set_sw_control(hidpp::sw_control::ACTIVE)
            });
            self.dirty = true;
            self.next_apply = Instant::now();
        } else {
            self.status.lights_asleep = true;
            let _ = self.with_logi(|l| {
                l.dev.set_sw_control(hidpp::sw_control::IDLE)?;
                l.dev.set_rgb_power(false)
            });
        }
    }

    fn apply_logi(&mut self) {
        self.dirty = false;
        if self.status.lights_asleep || !self.status.connected {
            // Painted again on wake.
            self.dirty = !self.status.connected;
            self.next_apply = Instant::now() + APPLY_RETRY;
            return;
        }
        let Some((p, idle_off)) = self.desired.clone() else { return };
        let per_key = self.status.zones.clone();
        let result = self.with_logi(|l| {
            let _ = l.dev.ensure_host_mode();
            l.dev.set_sw_control(hidpp::sw_control::ACTIVE)?;
            let _ = l.dev.set_rgb_timeouts(idle_off, idle_off);
            let base = scaled(&p.color1, p.brightness);
            let wanted = if p.brightness == 0 { 0 } else { p.effect };
            for zone in &l.zones {
                let (index, id) = pick_effect(zone, wanted);
                l.dev.set_rgb_effect(zone.index, index, logi_params(id, &p, base))?;
            }
            if wanted == 1 && !p.keys.is_empty() && !per_key.is_empty() {
                let keys: Vec<(u8, [u8; 3])> = per_key
                    .iter()
                    .map(|&z| (z, p.keys.get(&z).map(|c| scaled(c, p.brightness)).unwrap_or(base)))
                    .collect();
                l.dev.set_per_key(&keys)?;
            }
            Ok(())
        });
        if result.is_none() {
            self.dirty = true;
            self.next_apply = Instant::now() + APPLY_RETRY;
        }
    }

    fn release(&mut self) {
        if let Some(Dev::Via(_)) = self.dev {
            // Leave the board on the stored base, not wherever the effect was.
            if self.anim.is_some() {
                self.set_anim(None);
                if let Some((p, _)) = self.desired.clone() {
                    self.write_via(&p);
                }
            }
            return;
        }
        if !matches!(self.dev, Some(Dev::Logi(_))) {
            return;
        }
        let _ = self.with_logi(|l| {
            l.dev.set_sw_control(hidpp::sw_control::RELEASE)?;
            // Back to the onboard profile, which owns the lights from here.
            let f = l.dev.feature_index(hidpp::feature::ONBOARD_PROFILES)?;
            l.dev.request(f, 1, &[0x01])?;
            Ok(())
        });
    }
}

fn via_values(kind: via::Lighting) -> Vec<via::Value> {
    use via::Value::*;
    match kind {
        via::Lighting::Wilba => vec![Brightness, Effect, Speed, Color1, Color2, CapsOn, CapsColor, Layer1On, Layer1Color, Layer2On, Layer2Color, Layer3On, Layer3Color],
        via::Lighting::RgbMatrix => vec![Brightness, Effect, Speed, Color1],
    }
}

/// The values an NK65 should hold for `p`.
fn via_target(kind: via::Lighting, p: &Preset) -> Vec<(via::Value, [u8; 2])> {
    use via::Value::*;
    let hs = |c: &str| to_hue_sat(parse_color(c).unwrap_or([255, 255, 255]));
    let brightness = (p.brightness as u16 * 255 / 100) as u8;
    // kit's own effects play over Solid (effect 1 on both firmwares).
    let effect = if anim::is_kit(p.effect) { 1 } else { p.effect.min(255) as u8 };
    let mut out = vec![(Brightness, [brightness, 0]), (Effect, [effect, 0]), (Color1, hs(&p.color1))];
    match kind {
        via::Lighting::Wilba => {
            out.push((Speed, [(p.speed as u16 * 3 / 100) as u8, 0]));
            out.push((Color2, hs(&p.color2)));
            let slots = [(CapsOn, CapsColor), (Layer1On, Layer1Color), (Layer2On, Layer2Color), (Layer3On, Layer3Color)];
            for (i, (on, color)) in slots.into_iter().enumerate() {
                match &p.indicators[i] {
                    Some(c) => {
                        out.push((on, [254, 0]));
                        out.push((color, hs(c)));
                    }
                    None => out.push((on, [255, 0])),
                }
            }
        }
        via::Lighting::RgbMatrix => out.push((Speed, [(p.speed as u16 * 255 / 100) as u8, 0])),
    }
    out
}

fn scaled(color: &str, brightness: u8) -> [u8; 3] {
    parse_color(color).unwrap_or([255, 255, 255]).map(|c| (c as u16 * brightness as u16 / 100) as u8)
}

/// The zone's effect closest to `wanted` (cycle, wave and ripple come in two
/// generations), falling back to static. Returns (index, id).
fn pick_effect(zone: &hidpp::RgbZone, wanted: u16) -> (u8, u16) {
    let family: &[u16] = match wanted {
        0 => &[0x00],
        3 => &[0x03, 0x15],
        4 => &[0x04, 0x16],
        10 => &[0x0A, 0x02],
        11 => &[0x0B, 0x17],
        _ => &[0x01],
    };
    family
        .iter()
        .find_map(|id| zone.effects.iter().find(|e| e.id == *id))
        .or_else(|| zone.effects.iter().find(|e| e.id == 0x01))
        .map(|e| (e.index, e.id))
        .unwrap_or((0, 0x01))
}

/// The 10 parameter bytes of a 0x8071 effect (layouts from Solaar's LEDEffects).
fn logi_params(id: u16, p: &Preset, rgb: [u8; 3]) -> [u8; 10] {
    let mut b = [0u8; 10];
    // Slow (20 s) to fast (1 s) for animations that take a period in ms.
    let period = (20_000 - p.speed as u32 * 190) as u16;
    let [ph, pl] = period.to_be_bytes();
    let intensity = p.brightness;
    match id {
        0x00 => {}
        0x02 => {
            b[..3].copy_from_slice(&rgb);
            b[3] = 100 - p.speed.min(99);
        }
        0x03 => {
            b[5..7].copy_from_slice(&[ph, pl]);
            b[7] = intensity;
        }
        0x04 => {
            b[6..8].copy_from_slice(&[ph, pl]);
            b[9] = 1;
        }
        0x0A => {
            b[..3].copy_from_slice(&rgb);
            b[3..5].copy_from_slice(&[ph, pl]);
            b[6] = intensity;
        }
        0x0B | 0x17 => {
            b[..3].copy_from_slice(&rgb);
            let ripple = (200 - p.speed as u16 * 198 / 100).to_be_bytes();
            if id == 0x0B {
                b[4..6].copy_from_slice(&ripple);
            } else {
                b[3] = 255;
                b[6..8].copy_from_slice(&ripple);
            }
        }
        0x15 | 0x16 => {
            b[1] = 255;
            b[6..8].copy_from_slice(&[ph, pl]);
            b[8] = intensity;
            if id == 0x16 {
                b[9] = 1;
            }
        }
        _ => b[..3].copy_from_slice(&rgb),
    }
    b
}
