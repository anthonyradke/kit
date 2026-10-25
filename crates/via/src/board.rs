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
}

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
            let mut board = Board { dev: api.open_path(d.path())?, info, protocol: 0 };
            let r = board.command(&[cmd::PROTOCOL])?;
            board.protocol = u16::from_be_bytes([r[1], r[2]]);
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
        self.info.lighting
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
