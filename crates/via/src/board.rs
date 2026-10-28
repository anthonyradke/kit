use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

use crate::{Known, Lighting, Value, KNOWN};

pub const USAGE_PAGE: u16 = 0xFF60;
pub const USAGE: u16 = 0x61;
const LEN: usize = 32;
const TIMEOUT: Duration = Duration::from_millis(600);
/// Payload bytes per buffer read/write (32 minus the 4-byte header).
const CHUNK: usize = 28;
/// First protocol whose lighting commands carry a channel byte.
const CHANNELS_FROM: u16 = 11;

mod cmd {
    pub const PROTOCOL: u8 = 0x01;
    pub const SET_KEYCODE: u8 = 0x05;
    pub const SET_VALUE: u8 = 0x07;
    pub const GET_VALUE: u8 = 0x08;
    pub const SAVE: u8 = 0x09;
    pub const BOOTLOADER: u8 = 0x0B;
    pub const MACRO_COUNT: u8 = 0x0C;
    pub const MACRO_SIZE: u8 = 0x0D;
    pub const MACRO_GET: u8 = 0x0E;
    pub const MACRO_SET: u8 = 0x0F;
    pub const LAYER_COUNT: u8 = 0x11;
    pub const KEYMAP_GET: u8 = 0x12;
    pub const UNHANDLED: u8 = 0xFF;
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hid: {0}")]
    Hid(#[from] hidapi::HidError),
    #[error("NK65 not found (is it plugged in?)")]
    NotFound,
    #[error("timed out waiting for the keyboard")]
    Timeout,
    #[error("the keyboard's firmware doesn't support this")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct Board {
    dev: HidDevice,
    pub info: &'static Known,
    pub protocol: u16,
    /// The lighting code the firmware runs. The original NK65 ships with
    /// wilba's, but current QMK (and kit's firmware) use RGB matrix on the same USB id.
    pub lighting: Lighting,
    /// kit's own firmware: (version, number of kit effects, first kit effect number).
    pub kit: Option<KitFirmware>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KitFirmware {
    pub version: u8,
    pub effects: u8,
    pub first_effect: u8,
}

/// kit's values on VIA's custom channel (see firmware/nk65/keymaps/kit/keymap.c).
const KIT_CHANNEL: u8 = 0;
const KIT_INFO: u8 = 0x6B;
const KIT_BOOTLOADER: u8 = 0x62;
const KIT_COLORS: u8 = 0x63;

/// The first known board that is plugged in, without opening it.
pub fn find() -> Option<&'static Known> {
    let api = HidApi::new().ok()?;
    let found = api.device_list().find_map(|d| {
        if d.usage_page() != USAGE_PAGE || d.usage() != USAGE {
            return None;
        }
        KNOWN.iter().find(|k| k.vid == d.vendor_id() && k.pid == d.product_id())
    });
    found
}

impl Board {
    pub fn open() -> Result<Self> {
        let api = HidApi::new()?;
        for d in api.device_list() {
            if d.usage_page() != USAGE_PAGE || d.usage() != USAGE {
                continue;
            }
            let Some(info) = KNOWN.iter().find(|k| k.vid == d.vendor_id() && k.pid == d.product_id()) else {
                continue;
            };
            let mut board = Board { dev: api.open_path(d.path())?, info, protocol: 0, lighting: info.lighting, kit: None };
            let r = board.command(&[cmd::PROTOCOL])?;
            board.protocol = u16::from_be_bytes([r[1], r[2]]);
            if board.protocol >= CHANNELS_FROM {
                let (channel, id) = Value::Brightness.address(Lighting::RgbMatrix).unwrap_or((3, 1));
                if board.command(&[cmd::GET_VALUE, channel, id]).is_ok() {
                    board.lighting = Lighting::RgbMatrix;
                }
                if let Ok(r) = board.command(&[cmd::GET_VALUE, KIT_CHANNEL, KIT_INFO]) {
                    board.kit = Some(KitFirmware { version: r[3], effects: r[4], first_effect: r[5] });
                }
            }
            return Ok(board);
        }
        Err(Error::NotFound)
    }

    /// Send one report and wait for the keyboard to echo its command byte.
    fn command(&mut self, data: &[u8]) -> Result<[u8; LEN]> {
        let mut buf = [0u8; LEN + 1];
        buf[1..1 + data.len().min(LEN)].copy_from_slice(&data[..data.len().min(LEN)]);
        self.dev.write(&buf)?;
        let deadline = Instant::now() + TIMEOUT;
        let mut resp = [0u8; LEN];
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let n = self.dev.read_timeout(&mut resp, left.as_millis().max(1) as i32)?;
            if n == 0 {
                continue;
            }
            if resp[0] == cmd::UNHANDLED {
                return Err(Error::Unsupported);
            }
            if resp[0] == data[0] {
                return Ok(resp);
            }
        }
        Err(Error::Timeout)
    }

    /// kit's firmware (version 2 on): which PCB's column pins have seen a key
    /// press since it started (bit 0 v1, bit 1 v1.4). 0 until a key in 7-9 / U-O / J-L / N-, is pressed.
    pub fn kit_pcb(&mut self) -> Result<u8> {
        Ok(self.command(&[cmd::GET_VALUE, KIT_CHANNEL, KIT_INFO])?[6])
    }

    /// kit's firmware (version 4 on): its effects' second color (hue,
    /// saturation) and background level.
    pub fn kit_colors(&mut self) -> Result<[u8; 3]> {
        let r = self.command(&[cmd::GET_VALUE, KIT_CHANNEL, KIT_COLORS])?;
        Ok([r[3], r[4], r[5]])
    }

    pub fn set_kit_colors(&mut self, v: [u8; 3]) -> Result<()> {
        self.command(&[cmd::SET_VALUE, KIT_CHANNEL, KIT_COLORS, v[0], v[1], v[2]])?;
        Ok(())
    }

    /// Store the kit colors in the board's EEPROM.
    pub fn save_kit_colors(&mut self) -> Result<()> {
        self.command(&[cmd::SAVE, KIT_CHANNEL, KIT_COLORS])?;
        Ok(())
    }

    pub fn layer_count(&mut self) -> Result<u8> {
        Ok(self.command(&[cmd::LAYER_COUNT])?[1])
    }

    /// Every keycode, layer by layer, row by row.
    pub fn keymap(&mut self, layers: u8) -> Result<Vec<u16>> {
        let total = layers as usize * self.info.rows as usize * self.info.cols as usize * 2;
        let bytes = self.read_buffer(cmd::KEYMAP_GET, total)?;
        Ok(bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_be_bytes(c)).collect())
    }

    pub fn set_keycode(&mut self, layer: u8, row: u8, col: u8, code: u16) -> Result<()> {
        let [hi, lo] = code.to_be_bytes();
        self.command(&[cmd::SET_KEYCODE, layer, row, col, hi, lo])?;
        Ok(())
    }

    pub fn macro_count(&mut self) -> Result<u8> {
        Ok(self.command(&[cmd::MACRO_COUNT])?[1])
    }

    pub fn macro_buffer(&mut self) -> Result<Vec<u8>> {
        let r = self.command(&[cmd::MACRO_SIZE])?;
        let size = u16::from_be_bytes([r[1], r[2]]) as usize;
        self.read_buffer(cmd::MACRO_GET, size)
    }

    /// Write the whole macro buffer (it lives in EEPROM, so only on change).
    pub fn set_macro_buffer(&mut self, data: &[u8]) -> Result<()> {
        for (i, chunk) in data.chunks(CHUNK).enumerate() {
            let [hi, lo] = ((i * CHUNK) as u16).to_be_bytes();
            let mut req = vec![cmd::MACRO_SET, hi, lo, chunk.len() as u8];
            req.extend_from_slice(chunk);
            self.command(&req)?;
        }
        Ok(())
    }

    fn read_buffer(&mut self, command: u8, total: usize) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(total);
        while out.len() < total {
            let size = (total - out.len()).min(CHUNK);
            let [hi, lo] = (out.len() as u16).to_be_bytes();
            let r = self.command(&[command, hi, lo, size as u8])?;
            out.extend_from_slice(&r[4..4 + size]);
        }
        Ok(out)
    }

    fn lighting(&self) -> Lighting {
        self.lighting
    }

    /// Restart the board into its bootloader (STM32 DFU) for flashing. kit's
    /// firmware does it on request; older VIA firmware may answer VIA's own
    /// bootloader command. Either way the board drops off USB, so a timeout or
    /// a HID error afterwards means it worked.
    pub fn enter_bootloader(&mut self) -> Result<()> {
        let result = if self.kit.is_some() {
            self.command(&[cmd::SET_VALUE, KIT_CHANNEL, KIT_BOOTLOADER])
        } else {
            self.command(&[cmd::BOOTLOADER])
        };
        match result {
            Ok(_) | Err(Error::Timeout) | Err(Error::Hid(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Header of a lighting get/set for this firmware: command, then channel and id.
    fn value_header(&self, command: u8, value: Value) -> Result<Vec<u8>> {
        if self.protocol >= CHANNELS_FROM {
            let (channel, id) = value.address(self.lighting()).ok_or(Error::Unsupported)?;
            Ok(vec![command, channel, id])
        } else {
            Ok(vec![command, value.legacy_id(self.lighting()).ok_or(Error::Unsupported)?])
        }
    }

    pub fn get_value(&mut self, value: Value) -> Result<[u8; 2]> {
        let req = self.value_header(cmd::GET_VALUE, value)?;
        let r = self.command(&req)?;
        let at = req.len();
        Ok([r[at], r[at + 1]])
    }

    pub fn set_value(&mut self, value: Value, data: &[u8]) -> Result<()> {
        let mut req = self.value_header(cmd::SET_VALUE, value)?;
        req.extend_from_slice(data);
        self.command(&req)?;
        Ok(())
    }

    /// Store the current lighting in EEPROM so it survives unplugging.
    pub fn save_lighting(&mut self) -> Result<()> {
        if self.protocol >= CHANNELS_FROM {
            let channel = Value::Brightness.address(self.lighting()).map(|a| a.0).unwrap_or(0);
            self.command(&[cmd::SAVE, channel])?;
        } else {
            self.command(&[cmd::SAVE])?;
        }
        Ok(())
    }
}
