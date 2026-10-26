//! Client for QMK keyboards running VIA firmware (the NovelKeys NK65 family):
//! the keymap, the onboard macro buffer and lighting, over VIA's raw HID
//! interface (usage page 0xFF60, 32-byte reports).
//!
//! Lighting differs by board: the original NK65 (PID 0x4E4B) runs wilba's
//! RGB backlight code (two colors, indicator lights), the NK65 rev B
//! (0x4E4F) QMK's RGB matrix. Firmware before VIA protocol 11 addresses
//! lighting values without a channel byte.

pub mod keycodes;
pub mod macros;
pub mod usage;

#[cfg(feature = "hid")]
mod board;
#[cfg(feature = "hid")]
pub use board::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lighting {
    /// wilba_tech RGB backlight: effects 0-10, speed 0-3, two colors, indicators.
    Wilba,
    /// QMK RGB matrix: effect = matrix mode, speed 0-255, one color.
    RgbMatrix,
}

pub struct Known {
    pub vid: u16,
    pub pid: u16,
    pub name: &'static str,
    pub lighting: Lighting,
    pub rows: u8,
    pub cols: u8,
}

pub const KNOWN: &[Known] = &[
    Known { vid: 0x8968, pid: 0x4E4B, name: "NK65", lighting: Lighting::Wilba, rows: 5, cols: 15 },
    Known { vid: 0x8968, pid: 0x4E4F, name: "NK65 rev B", lighting: Lighting::RgbMatrix, rows: 5, cols: 15 },
];

/// Lighting values kit reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Value {
    /// 0-255
    Brightness,
    Effect,
    Speed,
    /// Hue, saturation (0-255 each)
    Color1,
    Color2,
    /// Indicator color (hue, sat) and whether it is on (row byte 254 = on, 255 = off).
    CapsColor,
    CapsOn,
    Layer1Color,
    Layer1On,
    Layer2Color,
    Layer2On,
    Layer3Color,
    Layer3On,
}

impl Value {
    /// (channel, value id) for protocol 11+, or `None` if the board lacks it.
    pub fn address(self, lighting: Lighting) -> Option<(u8, u8)> {
        use Value::*;
        match lighting {
            Lighting::Wilba => Some((
                0,
                match self {
                    Brightness => 0x09,
                    Effect => 0x0A,
                    Speed => 0x0B,
                    Color1 => 0x0C,
                    Color2 => 0x0D,
                    CapsColor => 0x0E,
                    CapsOn => 0x0F,
                    Layer1Color => 0x10,
                    Layer1On => 0x11,
                    Layer2Color => 0x12,
                    Layer2On => 0x13,
                    Layer3Color => 0x14,
                    Layer3On => 0x15,
                },
            )),
            Lighting::RgbMatrix => match self {
                Brightness => Some((3, 1)),
                Effect => Some((3, 2)),
                Speed => Some((3, 3)),
                Color1 => Some((3, 4)),
                _ => None,
            },
        }
    }

    /// Value id for firmware older than protocol 11 (no channel byte).
    pub fn legacy_id(self, lighting: Lighting) -> Option<u8> {
        match lighting {
            Lighting::Wilba => self.address(lighting).map(|a| a.1),
            Lighting::RgbMatrix => self.address(lighting).map(|a| 0x7F + a.1),
        }
    }
}
