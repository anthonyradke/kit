//! Minimal HID++ 2.0 client for Logitech mice and keyboards (Lightspeed receiver or USB cable).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub const LOGITECH_VID: u16 = 0x046D;
const VENDOR_USAGE_PAGE: u16 = 0xFF00;
const LONG_USAGE: u16 = 0x0002;
const REPORT_LONG: u8 = 0x11;
const LONG_LEN: usize = 20;
/// Software ids so replies are told apart from other apps' (and from each
/// other: the mouse and keyboard workers can share one receiver).
const SW_ID_MOUSE: u8 = 0x0A;
const SW_ID_KEYBOARD: u8 = 0x0B;
/// An idle wireless mouse is in power-saving mode and can take over a second
/// to answer, so this is deliberately generous.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(2500);
/// Timeout for slots other than receiver slot 1 while searching. A cabled
/// mouse is awake and answers fast, and the spare receiver slots are usually
/// empty, so waiting the full timeout on each made opening very slow.
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);
/// Notifications kept while a request waits for its reply.
const MAX_PENDING: usize = 32;
/// Device index used when the mouse is plugged in by cable.
pub const WIRED_INDEX: u8 = 0xFF;

pub mod feature {
    pub const ROOT: u16 = 0x0000;
    pub const FEATURE_SET: u16 = 0x0001;
    pub const DEVICE_NAME: u16 = 0x0005;
    pub const WIRELESS_STATUS: u16 = 0x1D4B;
    pub const BATTERY_STATUS: u16 = 0x1000;
    pub const UNIFIED_BATTERY: u16 = 0x1004;
    pub const ADJUSTABLE_DPI: u16 = 0x2201;
    pub const REPORT_RATE: u16 = 0x8060;
    pub const ONBOARD_PROFILES: u16 = 0x8100;
    pub const RGB_EFFECTS: u16 = 0x8071;
    pub const PER_KEY_LIGHTING_V2: u16 = 0x8081;
}

/// What kind of device answered (feature 0x0005, getDeviceType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Keyboard,
    Mouse,
}

impl Kind {
    fn matches(self, device_type: u8) -> bool {
        match self {
            // Keyboard, or numpad (some combo boards report it).
            Kind::Keyboard => matches!(device_type, 0 | 2),
            // Mouse, trackpad, trackball.
            Kind::Mouse => matches!(device_type, 3..=5),
        }
    }

    fn sw_id(self) -> u8 {
        match self {
            Kind::Keyboard => SW_ID_KEYBOARD,
            Kind::Mouse => SW_ID_MOUSE,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("no Logitech HID++ interface found (is G HUB closed and the receiver plugged in?)")]
    NoInterface,
    #[error("no responding HID++ 2.0 device of that kind found (is it switched on?)")]
    NoDevice,
    #[error("timed out waiting for reply")]
    Timeout,
    #[error("device returned HID++ error 0x{0:02X}")]
    Device(u8),
    #[error("feature 0x{0:04X} not supported by this device")]
    Unsupported(u16),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Device {
    dev: HidDevice,
    index: u8,
    sw_id: u8,
    timeout: Duration,
    /// Feature id -> runtime index (0 = not supported). Indexes never change
    /// for a device, so each is looked up once.
    features: HashMap<u16, u8>,
    /// Notifications that arrived while waiting for a reply, for `read_event`.
    pending: VecDeque<(u8, u8, [u8; 16])>,
    pub product: String,
}

/// The mouse worker's name for a device; every request works on any kind.
pub type Mouse = Device;

impl Device {
    /// Find the first Logitech HID++ device of `kind` that answers. Receivers
    /// and cabled devices are tried in turn, so a keyboard on its own receiver
    /// is never mistaken for the mouse (or the other way round).
    pub fn open(kind: Kind) -> Result<Self> {
        let api = HidApi::new()?;
        let mut found_interface = false;
        for info in api.device_list() {
            if info.vendor_id() != LOGITECH_VID
                || info.usage_page() != VENDOR_USAGE_PAGE
                || info.usage() != LONG_USAGE
            {
                continue;
            }
            found_interface = true;
            let product = info.product_string().unwrap_or("?").to_string();
            let dev = api.open_path(info.path())?;
            let mut mouse = Device {
                dev,
                index: 0,
                sw_id: kind.sw_id(),
                timeout: REQUEST_TIMEOUT,
                features: HashMap::new(),
                pending: VecDeque::new(),
                product,
            };
            // Receiver slot 1 is the usual one and a cabled mouse answers on 0xFF;
            // try those before the rarely used slots 2..=6.
            for index in [1u8, WIRED_INDEX, 2, 3, 4, 5, 6] {
                mouse.index = index;
                mouse.timeout = if index == 1 { REQUEST_TIMEOUT } else { PROBE_TIMEOUT };
                if mouse.ping().is_ok() {
                    mouse.timeout = REQUEST_TIMEOUT;
                    mouse.pending.clear();
                    if mouse.is_kind(kind) {
                        return Ok(mouse);
                    }
                    // Another kind of device; feature indexes are per device.
                    mouse.features.clear();
                }
            }
        }
        Err(if found_interface { Error::NoDevice } else { Error::NoInterface })
    }

    pub fn device_index(&self) -> u8 {
        self.index
    }

    /// Send a request and wait for the matching reply's 16 parameter bytes.
    pub fn request(&mut self, feat_index: u8, function: u8, params: &[u8]) -> Result<[u8; 16]> {
        let mut buf = [0u8; LONG_LEN];
        buf[0] = REPORT_LONG;
        buf[1] = self.index;
        buf[2] = feat_index;
        buf[3] = (function << 4) | self.sw_id;
        let params = &params[..params.len().min(LONG_LEN - 4)];
        buf[4..4 + params.len()].copy_from_slice(params);
        self.dev.write(&buf)?;

        let deadline = Instant::now() + self.timeout;
        let mut resp = [0u8; LONG_LEN];
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let n = self.dev.read_timeout(&mut resp, left.as_millis().max(1) as i32)?;
            if n < 5 || resp[1] != self.index {
                continue;
            }
            if resp[2] == 0xFF && resp[3] == feat_index && resp[4] == buf[3] {
                return Err(Error::Device(resp[5]));
            }
            if resp[2] == feat_index && resp[3] == buf[3] {
                let mut out = [0u8; 16];
                out.copy_from_slice(&resp[4..20]);
                return Ok(out);
            }
            // Anything else is an unrelated notification (such as the mouse
            // reconnecting): keep it for `read_event` and keep waiting.
            if resp[0] == REPORT_LONG && self.pending.len() < MAX_PENDING {
                let mut params = [0u8; 16];
                params.copy_from_slice(&resp[4..20]);
                self.pending.push_back((resp[2], resp[3], params));
            }
        }
        Err(Error::Timeout)
    }

    /// Returns (major, minor) HID++ protocol version.
    pub fn ping(&mut self) -> Result<(u8, u8)> {
        let r = self.request(0, 1, &[0, 0, 0x5A])?;
        Ok((r[0], r[1]))
    }

    /// Look up the runtime index of a feature, if the device has it.
    pub fn feature_index(&mut self, id: u16) -> Result<u8> {
        let index = match self.features.get(&id) {
            Some(&index) => index,
            None => {
                let index = self.request(0, 0, &id.to_be_bytes())?[0];
                self.features.insert(id, index);
                index
            }
        };
        if index == 0 && id != feature::ROOT {
            return Err(Error::Unsupported(id));
        }
        Ok(index)
    }

    /// All (index, feature id) pairs the device exposes.
    pub fn features(&mut self) -> Result<Vec<(u8, u16)>> {
        let fs = self.feature_index(feature::FEATURE_SET)?;
        let count = self.request(fs, 0, &[])?[0];
        let mut out = vec![(0, feature::ROOT)];
        for i in 1..=count {
            let r = self.request(fs, 1, &[i])?;
            out.push((i, u16::from_be_bytes([r[0], r[1]])));
        }
        Ok(out)
    }

    /// Whether this is a device of `kind`. One without the device-type
    /// function is taken at its word (older mice).
    fn is_kind(&mut self, kind: Kind) -> bool {
        let Ok(f) = self.feature_index(feature::DEVICE_NAME) else { return kind == Kind::Mouse };
        match self.request(f, 2, &[]) {
            Ok(r) => kind.matches(r[0]),
            Err(Error::Device(_)) => kind == Kind::Mouse,
            Err(_) => false,
        }
    }

    pub fn name(&mut self) -> Result<String> {
        let f = self.feature_index(feature::DEVICE_NAME)?;
        let len = self.request(f, 0, &[])?[0] as usize;
        let mut name = Vec::with_capacity(len);
        while name.len() < len {
            let r = self.request(f, 1, &[name.len() as u8])?;
            let take = (len - name.len()).min(16);
            name.extend_from_slice(&r[..take]);
        }
        Ok(String::from_utf8_lossy(&name).trim_end_matches('\0').to_string())
    }

    /// Battery percentage and whether it is charging.
    pub fn battery(&mut self) -> Result<(u8, bool)> {
        if let Ok(f) = self.feature_index(feature::UNIFIED_BATTERY) {
            let r = self.request(f, 1, &[])?;
            return Ok((r[0], r[2] == 1 || r[2] == 2));
        }
        let f = self.feature_index(feature::BATTERY_STATUS)?;
        let r = self.request(f, 0, &[])?;
        Ok((r[0], r[2] == 1 || r[2] == 2))
    }

    pub fn dpi(&mut self) -> Result<u16> {
        let f = self.feature_index(feature::ADJUSTABLE_DPI)?;
        let r = self.request(f, 2, &[0])?;
        Ok(u16::from_be_bytes([r[1], r[2]]))
    }

    pub fn set_dpi(&mut self, dpi: u16) -> Result<u16> {
        let f = self.feature_index(feature::ADJUSTABLE_DPI)?;
        let [hi, lo] = dpi.to_be_bytes();
        let r = self.request(f, 3, &[0, hi, lo])?;
        Ok(u16::from_be_bytes([r[1], r[2]]))
    }

    /// Report rate in Hz.
    pub fn report_rate(&mut self) -> Result<u16> {
        let f = self.feature_index(feature::REPORT_RATE)?;
        let ms = self.request(f, 1, &[])?[0].max(1);
        Ok(1000 / ms as u16)
    }

    /// Set report rate in Hz (125, 250, 500 or 1000).
    pub fn set_report_rate(&mut self, hz: u16) -> Result<()> {
        let f = self.feature_index(feature::REPORT_RATE)?;
        let ms = (1000 / hz.clamp(125, 1000)) as u8;
        self.request(f, 2, &[ms])?;
        Ok(())
    }

    /// Put the mouse in host mode so software controls it. In the default
    /// onboard mode the stored profile owns the report rate and rejects changes.
    /// The mouse falls back to onboard mode when it power-cycles.
    pub fn ensure_host_mode(&mut self) -> Result<()> {
        const ONBOARD: u8 = 1;
        const HOST: u8 = 2;
        let f = match self.feature_index(feature::ONBOARD_PROFILES) {
            Ok(f) => f,
            Err(Error::Unsupported(_)) => return Ok(()),
            Err(e) => return Err(e),
        };
        if self.request(f, 2, &[])?[0] == ONBOARD {
            self.request(f, 1, &[HOST])?;
        }
        Ok(())
    }

    /// Wait up to `timeout_ms` for an unsolicited report addressed to this device.
    /// Returns (feature index, function/event byte, params).
    pub fn read_event(&mut self, timeout_ms: i32) -> Result<Option<(u8, u8, [u8; 16])>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let mut resp = [0u8; LONG_LEN];
        let n = self.dev.read_timeout(&mut resp, timeout_ms)?;
        if n < 5 || resp[0] != REPORT_LONG || resp[1] != self.index {
            return Ok(None);
        }
        let mut params = [0u8; 16];
        params.copy_from_slice(&resp[4..20]);
        Ok(Some((resp[2], resp[3], params)))
    }
}

/// One lighting zone of the RGB effects feature (0x8071) and the effects it
/// can run, in the device's own order (effects are set by their index).
#[derive(Clone, Debug, PartialEq)]
pub struct RgbZone {
    pub index: u8,
    pub location: u16,
    pub effects: Vec<RgbEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgbEffect {
    pub index: u8,
    /// Effect id: 0x00 off, 0x01 static, 0x03/0x15 cycle, 0x04/0x16 wave,
    /// 0x0A breathe, 0x0B/0x17 ripple.
    pub id: u16,
}

/// SetSWControl payloads (0x8071 fn 5), as G HUB and Solaar send them.
pub mod sw_control {
    /// Software owns the LEDs; the firmware still watches for idle.
    pub const ACTIVE: [u8; 3] = [0x01, 0x03, 0x04];
    /// Software owns the LEDs; the firmware reports activity again.
    pub const IDLE: [u8; 3] = [0x01, 0x03, 0x02];
    /// Hand the LEDs back to the onboard profile.
    pub const RELEASE: [u8; 3] = [0x01, 0x00, 0x00];
}

/// Keyboard lighting (LIGHTSYNC): zone effects, per-key colors and the idle
/// handshake. Protocol details follow Solaar's `RGBEffects`/`PerKeyLighting`.
impl Device {
    pub fn rgb_zones(&mut self) -> Result<Vec<RgbZone>> {
        let f = self.feature_index(feature::RGB_EFFECTS)?;
        let count = self.request(f, 0, &[0xFF, 0xFF, 0x00])?[2];
        let mut zones = Vec::with_capacity(count as usize);
        for z in 0..count {
            let r = self.request(f, 0, &[z, 0xFF, 0x00])?;
            let location = u16::from_be_bytes([r[2], r[3]]);
            let mut effects = Vec::with_capacity(r[4] as usize);
            for e in 0..r[4] {
                let r = self.request(f, 0, &[z, e, 0x00])?;
                effects.push(RgbEffect { index: r[1], id: u16::from_be_bytes([r[2], r[3]]) });
            }
            zones.push(RgbZone { index: z, location, effects });
        }
        Ok(zones)
    }

    /// Run effect number `effect_index` of `zone` with its 10 parameter bytes.
    pub fn set_rgb_effect(&mut self, zone: u8, effect_index: u8, params: [u8; 10]) -> Result<()> {
        let f = self.feature_index(feature::RGB_EFFECTS)?;
        let mut data = [0u8; 13];
        data[0] = zone;
        data[1] = effect_index;
        data[2..12].copy_from_slice(&params);
        data[12] = 0x01;
        self.request(f, 1, &data)?;
        Ok(())
    }

    pub fn set_sw_control(&mut self, payload: [u8; 3]) -> Result<()> {
        let f = self.feature_index(feature::RGB_EFFECTS)?;
        self.request(f, 5, &payload)?;
        Ok(())
    }

    /// Seconds without input before the firmware reports idle, and before it
    /// would sleep the LEDs on its own.
    pub fn set_rgb_timeouts(&mut self, idle_s: u16, sleep_s: u16) -> Result<()> {
        let f = self.feature_index(feature::RGB_EFFECTS)?;
        let [ih, il] = idle_s.to_be_bytes();
        let [sh, sl] = sleep_s.to_be_bytes();
        self.request(f, 7, &[0x01, 0x00, 0x00, ih, il, sh, sl])?;
        Ok(())
    }

    /// LEDs on (`true`) or faded off to sleep.
    pub fn set_rgb_power(&mut self, on: bool) -> Result<()> {
        let f = self.feature_index(feature::RGB_EFFECTS)?;
        self.request(f, 8, &[0x01, if on { 0x01 } else { 0x03 }, 0x00])?;
        Ok(())
    }

    /// Zone ids of the per-key feature (0x8081): HID usage - 3 for most keys,
    /// usage - 120 for the modifiers.
    pub fn per_key_zones(&mut self) -> Result<Vec<u8>> {
        let f = self.feature_index(feature::PER_KEY_LIGHTING_V2)?;
        let mut bitmap = Vec::with_capacity(42);
        for page in 0..3u8 {
            bitmap.extend_from_slice(&self.request(f, 0, &[0x00, page])?[2..]);
        }
        Ok((1..=255u8).filter(|&i| bitmap.get(i as usize / 8).is_some_and(|b| b >> (i % 8) & 1 == 1)).collect())
    }

    /// Paint keys and commit the frame. Keys sharing a color go out 13 at a
    /// time; the rest four per packet.
    pub fn set_per_key(&mut self, keys: &[(u8, [u8; 3])]) -> Result<()> {
        let f = self.feature_index(feature::PER_KEY_LIGHTING_V2)?;
        let mut by_color: Vec<([u8; 3], Vec<u8>)> = Vec::new();
        for &(zone, rgb) in keys {
            match by_color.iter_mut().find(|(c, _)| *c == rgb) {
                Some((_, zones)) => zones.push(zone),
                None => by_color.push((rgb, vec![zone])),
            }
        }
        let mut singles: Vec<u8> = Vec::with_capacity(16);
        for (rgb, zones) in &by_color {
            let mut rest: &[u8] = zones;
            while rest.len() > 3 {
                let (chunk, tail) = rest.split_at(rest.len().min(13));
                let mut data = rgb.to_vec();
                data.extend_from_slice(chunk);
                self.request(f, 6, &data)?;
                rest = tail;
            }
            for &zone in rest {
                singles.extend_from_slice(&[zone, rgb[0], rgb[1], rgb[2]]);
                if singles.len() == 16 {
                    self.request(f, 1, &singles)?;
                    singles.clear();
                }
            }
        }
        if !singles.is_empty() {
            self.request(f, 1, &singles)?;
        }
        self.request(f, 7, &[0x00])?;
        Ok(())
    }
}
