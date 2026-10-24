//! Minimal HID++ 2.0 client for Logitech mice (Lightspeed receiver or USB cable).

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub const LOGITECH_VID: u16 = 0x046D;
const VENDOR_USAGE_PAGE: u16 = 0xFF00;
const LONG_USAGE: u16 = 0x0002;
const REPORT_LONG: u8 = 0x11;
const LONG_LEN: usize = 20;
/// Arbitrary software id so we can tell our replies apart from other apps'.
const SW_ID: u8 = 0x0A;
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
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("no Logitech HID++ interface found (is G HUB closed and the receiver plugged in?)")]
    NoInterface,
    #[error("no responding HID++ 2.0 device found (is the mouse switched on?)")]
    NoDevice,
    #[error("timed out waiting for reply")]
    Timeout,
    #[error("device returned HID++ error 0x{0:02X}")]
    Device(u8),
    #[error("feature 0x{0:04X} not supported by this device")]
    Unsupported(u16),
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Mouse {
    dev: HidDevice,
    index: u8,
    timeout: Duration,
    /// Feature id -> runtime index (0 = not supported). Indexes never change
    /// for a device, so each is looked up once.
    features: HashMap<u16, u8>,
    /// Notifications that arrived while waiting for a reply, for `read_event`.
    pending: VecDeque<(u8, u8, [u8; 16])>,
    pub product: String,
}

impl Mouse {
    /// Find the first Logitech HID++ interface with a responding device on it.
    pub fn open() -> Result<Self> {
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
            let mut mouse = Mouse {
                dev,
                index: 0,
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
                    return Ok(mouse);
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
        buf[3] = (function << 4) | SW_ID;
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
