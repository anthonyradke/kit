//! Client for the SteelSeries Arctis Nova 5 / 5X base station (USB dongle).
//!
//! Every command is one 64-byte output report on the vendor interface
//! (usage page 0xFFC0). Settings commands are followed by a "save" so the
//! headset keeps them across power cycles. The equalizer runs on the headset
//! itself, so it applies to everything it plays, with no software in the path.
//!
//! Protocol details come from HeadsetControl (`steelseries_arctis_nova_5.hpp`)
//! plus the settings read-back (`0x20`) found by probing: the reply holds the
//! 10 EQ bands (6 bytes each) followed by mic volume, sidetone and the volume
//! limiter. The mute LED and auto-off settings aren't in it. The base station
//! answers it even while the headset is off.

use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

pub const STEELSERIES_VID: u16 = 0x1038;
/// Nova 5 and Nova 5X base stations.
pub const PRODUCT_IDS: [u16; 2] = [0x2232, 0x2253];
pub const VENDOR_USAGE_PAGE: u16 = 0xFFC0;
const MSG_LEN: usize = 64;
const READ_TIMEOUT: Duration = Duration::from_millis(500);

pub const BANDS: usize = 10;
pub const GAIN_MIN: f32 = -10.0;
pub const GAIN_MAX: f32 = 10.0;
pub const Q_MIN: f32 = 0.2;
pub const Q_MAX: f32 = 10.0;
pub const FREQ_MIN: u16 = 20;
pub const FREQ_MAX: u16 = 20000;
/// Frequency value the headset uses for an unused band.
const FREQ_DISABLED: u16 = 20001;
pub const SIDETONE_MAX: u8 = 10;
pub const MIC_VOLUME_MAX: u8 = 15;

mod op {
    pub const STATUS: u8 = 0xB0;
    pub const GET_SETTINGS: u8 = 0x20;
    pub const SET_EQ: u8 = 0x33;
    pub const VOLUME_LIMITER: u8 = 0x27;
    pub const SAVE_A: u8 = 0x09;
    pub const SAVE_B: u8 = 0x35;
    pub const MIC_VOLUME: u8 = 0x37;
    pub const SIDETONE: u8 = 0x39;
    pub const INACTIVE_TIME: u8 = 0xA3;
    pub const MIC_LED: u8 = 0xAE;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("Arctis Nova 5 base station not found (is the dongle plugged in?)")]
    NotFound,
    #[error("timed out waiting for the base station")]
    Timeout,
    #[error("unexpected reply from the base station")]
    BadReply,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// False when the dongle is plugged in but the headset is off or out of range.
    pub connected: bool,
    pub battery: u8,
    pub charging: bool,
    /// Game / chat volume from the headset's ChatMix (0-100 each).
    pub chatmix: (u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Peaking,
    LowPass,
    HighPass,
    LowShelf,
    HighShelf,
}

impl Filter {
    fn byte(self) -> u8 {
        match self {
            Filter::Peaking => 1,
            Filter::LowPass => 2,
            Filter::HighPass => 3,
            Filter::LowShelf => 4,
            Filter::HighShelf => 5,
        }
    }

    fn from_byte(b: u8) -> Self {
        match b {
            2 => Filter::LowPass,
            3 => Filter::HighPass,
            4 => Filter::LowShelf,
            5 => Filter::HighShelf,
            _ => Filter::Peaking,
        }
    }
}

/// One band of the headset's parametric EQ. `None` slots are unused.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub filter: Filter,
    pub freq: u16,
    /// dB, applied in 0.5 dB steps.
    pub gain: f32,
    pub q: f32,
}

pub type Eq = [Option<Band>; BANDS];

/// Settings the base station reports back.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub eq: Eq,
    pub mic_volume: u8,
    pub sidetone: u8,
    pub volume_limiter: bool,
}

/// The EQ as the headset stores it. Two EQs that encode the same are the
/// same to the headset (gain is in 0.5 dB steps, Q in thousandths).
pub fn encode_eq(eq: &Eq) -> [u8; 6 * BANDS] {
    let mut out = [0u8; 6 * BANDS];
    for (i, slot) in eq.iter().enumerate() {
        let (filter, freq, gain, q) = match slot {
            Some(b) => (b.filter, b.freq.clamp(FREQ_MIN, FREQ_MAX), b.gain, b.q),
            None => (Filter::Peaking, FREQ_DISABLED, 0.0, 1.414),
        };
        let gain = ((gain.clamp(GAIN_MIN, GAIN_MAX) - GAIN_MIN) * 2.0).round() as u8;
        let q = (q.clamp(Q_MIN, Q_MAX) * 1000.0).round() as u16;
        let b = &mut out[6 * i..6 * i + 6];
        b[..2].copy_from_slice(&freq.to_le_bytes());
        b[2] = filter.byte();
        b[3] = gain;
        b[4..].copy_from_slice(&q.to_le_bytes());
    }
    out
}

pub struct Headset {
    dev: HidDevice,
    pub product_id: u16,
}

impl Headset {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        let info = api
            .device_list()
            .find(|d| {
                d.vendor_id() == STEELSERIES_VID
                    && PRODUCT_IDS.contains(&d.product_id())
                    && d.usage_page() == VENDOR_USAGE_PAGE
            })
            .ok_or(Error::NotFound)?;
        Ok(Headset { dev: api.open_path(info.path())?, product_id: info.product_id() })
    }

    /// Send one command (without the report id byte), zero padded to 64 bytes.
    fn send(&self, cmd: &[u8]) -> Result<()> {
        let mut buf = [0u8; MSG_LEN + 1];
        buf[1..1 + cmd.len()].copy_from_slice(cmd);
        self.dev.write(&buf)?;
        Ok(())
    }

    /// Send a command and wait for the reply that echoes its opcode.
    pub fn query(&self, cmd: &[u8]) -> Result<Vec<u8>> {
        // Drop anything unsolicited left in the queue.
        let mut r = [0u8; 128];
        while self.dev.read_timeout(&mut r, 0)? > 0 {}
        self.send(cmd)?;
        let until = Instant::now() + READ_TIMEOUT;
        while Instant::now() < until {
            let n = self.dev.read_timeout(&mut r, 50)?;
            if n > 0 && r[0] == cmd[0] {
                return Ok(r[..n].to_vec());
            }
        }
        Err(Error::Timeout)
    }

    /// Write a setting and make the headset persist it.
    fn set(&self, cmd: &[u8]) -> Result<()> {
        self.send(cmd)?;
        self.send(&[op::SAVE_A])?;
        self.send(&[op::SAVE_B, 0x01])
    }

    pub fn status(&self) -> Result<Status> {
        let r = self.query(&[op::STATUS])?;
        if r.len() < 7 {
            return Err(Error::BadReply);
        }
        Ok(Status {
            connected: r[1] != 0x02,
            battery: r[3].min(100),
            charging: r[4] == 0x01,
            chatmix: (r[5], r[6]),
        })
    }

    pub fn settings(&self) -> Result<Settings> {
        let r = self.query(&[op::GET_SETTINGS])?;
        if r.len() < 64 {
            return Err(Error::BadReply);
        }
        let mut eq = [None; BANDS];
        for (i, slot) in eq.iter_mut().enumerate() {
            let b = &r[1 + 6 * i..7 + 6 * i];
            let freq = u16::from_le_bytes([b[0], b[1]]);
            if freq == FREQ_DISABLED {
                continue;
            }
            *slot = Some(Band {
                filter: Filter::from_byte(b[2]),
                freq,
                gain: b[3] as f32 / 2.0 + GAIN_MIN,
                q: u16::from_le_bytes([b[4], b[5]]) as f32 / 1000.0,
            });
        }
        Ok(Settings { eq, mic_volume: r[61], sidetone: r[62], volume_limiter: r[63] != 0 })
    }

    /// Replace the whole EQ. Values are clamped to what the headset accepts.
    /// With `persist` false the change is live but not saved, for previews
    /// while dragging (saving writes the headset's flash).
    pub fn set_eq(&self, eq: &Eq, persist: bool) -> Result<()> {
        let mut cmd = [0u8; 1 + 6 * BANDS];
        cmd[0] = op::SET_EQ;
        cmd[1..].copy_from_slice(&encode_eq(eq));
        if persist {
            self.set(&cmd)
        } else {
            self.send(&cmd)
        }
    }

    /// How loudly the mic is fed back into the headphones, 0..=10.
    pub fn set_sidetone(&self, level: u8) -> Result<()> {
        self.set(&[op::SIDETONE, level.min(SIDETONE_MAX)])
    }

    /// Hardware mic gain, 0..=15.
    pub fn set_mic_volume(&self, level: u8) -> Result<()> {
        self.set(&[op::MIC_VOLUME, level.min(MIC_VOLUME_MAX)])
    }

    /// Mute LED brightness: 0 off, 1 low, 2 medium, 3 high.
    pub fn set_mic_led(&self, level: u8) -> Result<()> {
        let raw = match level {
            0 => 0x00,
            1 => 0x01,
            2 => 0x04,
            _ => 0x0A,
        };
        self.set(&[op::MIC_LED, raw])
    }

    pub fn set_volume_limiter(&self, on: bool) -> Result<()> {
        self.set(&[op::VOLUME_LIMITER, on as u8])
    }

    /// Minutes of silence before the headset turns itself off (0 = never).
    pub fn set_inactive_time(&self, minutes: u8) -> Result<()> {
        self.send(&[op::INACTIVE_TIME, minutes])
    }
}
