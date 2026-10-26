//! QMK renumbered most keycodes beyond the basics in 0.19 (VIA protocol 12).
//! A keymap read from older firmware has to be translated before it goes onto
//! a board running newer firmware (kit's NK65 firmware, for one).

/// Lighting keys of wilba's RGB backlight (old custom keycodes 0x5F80..),
/// mapped to their RGB matrix keycodes; color 2 has no equivalent.
const WILBA_TO_RM: [u16; 14] = [
    RM_VALU, RM_VALD, RM_NEXT, RM_PREV, RM_SPDU, RM_SPDD, RM_HUEU, RM_HUED, TRNS, TRNS, RM_SATU, RM_SATD, TRNS, TRNS,
];
/// Old RGB keycodes 0x5CC2.. (toggle, mode +/-, hue, sat, val, speed +/-).
const OLD_RGB_TO_RM: [u16; 11] = [RM_TOGG, RM_NEXT, RM_PREV, RM_HUEU, RM_HUED, RM_SATU, RM_SATD, RM_VALU, RM_VALD, RM_SPDU, RM_SPDD];

const TRNS: u16 = 0x0001;
const RM_TOGG: u16 = 0x7842;
const RM_NEXT: u16 = 0x7843;
const RM_PREV: u16 = 0x7844;
const RM_HUEU: u16 = 0x7845;
const RM_HUED: u16 = 0x7846;
const RM_SATU: u16 = 0x7847;
const RM_SATD: u16 = 0x7848;
const RM_VALU: u16 = 0x7849;
const RM_VALD: u16 = 0x784A;
const RM_SPDU: u16 = 0x784B;
const RM_SPDD: u16 = 0x784C;

/// The protocol-12 keycode for a keycode from firmware before protocol 12,
/// or `None` if it has no equivalent (the caller keeps that key's default).
pub fn upgrade(old: u16) -> Option<u16> {
    let in_range = |base: u16, len: u16| old.checked_sub(base).filter(|&n| n < len);
    Some(match old {
        // Basic keys, keys with modifiers and layer-tap are unchanged.
        0x0000..=0x1FFF | 0x4000..=0x4FFF => old,
        // Mod-tap moved from 0x6000 to 0x2000, same layout.
        0x6000..=0x7FFF => old - 0x4000,
        _ => {
            if let Some(n) = in_range(0x5010, 0x10) {
                0x5200 + n // TO
            } else if let Some(n) = in_range(0x5100, 0x20) {
                0x5220 + n // MO
            } else if let Some(n) = in_range(0x5200, 0x20) {
                0x5240 + n // DF
            } else if let Some(n) = in_range(0x5300, 0x20) {
                0x5260 + n // TG
            } else if let Some(n) = in_range(0x5400, 0x20) {
                0x5280 + n // OSL
            } else if let Some(n) = in_range(0x5500, 0x20) {
                0x52A0 + n // OSM (5 mod bits)
            } else if let Some(n) = in_range(0x5800, 0x20) {
                0x52C0 + n // TT
            } else if let Some(n) = in_range(0x5900, 0x100) {
                // LM: layer in the high nibble, 4 mod bits in the low one.
                0x5000 | ((n >> 4) << 5) | (n & 0x0F)
            } else if old == 0x5C00 {
                0x7C00 // QK_BOOT
            } else if let Some(n) = in_range(0x5CC2, OLD_RGB_TO_RM.len() as u16) {
                OLD_RGB_TO_RM[n as usize]
            } else if let Some(n) = in_range(0x5F12, 0x20) {
                0x7700 + n // macros
            } else {
                WILBA_TO_RM[in_range(0x5F80, WILBA_TO_RM.len() as u16)? as usize]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycodes_upgrade() {
        assert_eq!(upgrade(0x0004), Some(0x0004)); // A
        assert_eq!(upgrade(0x0001), Some(0x0001)); // transparent
        assert_eq!(upgrade(0x0204), Some(0x0204)); // Shift+A
        assert_eq!(upgrade(0x4104), Some(0x4104)); // LT(1, A)
        assert_eq!(upgrade(0x6229), Some(0x2229)); // MT(Shift, Esc)
        assert_eq!(upgrade(0x5101), Some(0x5221)); // MO(1)
        assert_eq!(upgrade(0x5302), Some(0x5262)); // TG(2)
        assert_eq!(upgrade(0x5013), Some(0x5203)); // TO(3)
        assert_eq!(upgrade(0x5923), Some(0x5000 | (2 << 5) | 3)); // LM(2, Ctrl+Shift)
        assert_eq!(upgrade(0x5C00), Some(0x7C00)); // reset -> boot
        assert_eq!(upgrade(0x5F12), Some(0x7700)); // M0
        assert_eq!(upgrade(0x5F80), Some(RM_VALU)); // wilba brightness +
        assert_eq!(upgrade(0x5F88), Some(TRNS)); // wilba color 2 hue +
        assert_eq!(upgrade(0x5CC2), Some(RM_TOGG));
        assert_eq!(upgrade(0x5D00), None);
    }
}
