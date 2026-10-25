//! QMK dynamic-keymap macros: the buffer VIA edits. Macros are stored one
//! after another, each ended by a NUL. Inside one, plain ASCII is typed as
//! text and `0x01` starts a code: `01 01 kc` tap, `01 02 kc` press,
//! `01 03 kc` release (basic keycodes), `01 04 <digits> |` wait in ms, and
//! `01 05..07 lo hi` the same with 16-bit keycodes (newer firmware).

const PREFIX: u8 = 0x01;
const TAP: u8 = 0x01;
const DOWN: u8 = 0x02;
const UP: u8 = 0x03;
const DELAY: u8 = 0x04;
const TAP16: u8 = 0x05;
const UP16: u8 = 0x07;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Text(String),
    Tap(u16),
    Down(u16),
    Up(u16),
    Delay(u32),
}

/// Split a macro buffer into `count` raw macros (missing ones are empty).
pub fn split(buffer: &[u8], count: usize) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = buffer.split(|&b| b == 0).take(count).map(<[u8]>::to_vec).collect();
    out.resize(count, Vec::new());
    out
}

/// Join raw macros back into a buffer of `size` bytes. `None` if they don't fit.
pub fn join(macros: &[Vec<u8>], size: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(size);
    for m in macros {
        buf.extend_from_slice(m);
        buf.push(0);
    }
    if buf.len() > size {
        return None;
    }
    buf.resize(size, 0);
    Some(buf)
}

pub fn decode(raw: &[u8]) -> Vec<Action> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != PREFIX {
            let start = i;
            while i < raw.len() && raw[i] != PREFIX {
                i += 1;
            }
            out.push(Action::Text(String::from_utf8_lossy(&raw[start..i]).into_owned()));
            continue;
        }
        let Some(&code) = raw.get(i + 1) else { break };
        match code {
            TAP | DOWN | UP => {
                let Some(&kc) = raw.get(i + 2) else { break };
                let kc = kc as u16;
                out.push(match code {
                    TAP => Action::Tap(kc),
                    DOWN => Action::Down(kc),
                    _ => Action::Up(kc),
                });
                i += 3;
            }
            DELAY => {
                let digits: String = raw[i + 2..].iter().take_while(|&&b| b != b'|').map(|&b| b as char).collect();
                out.push(Action::Delay(digits.parse().unwrap_or(0)));
                i += 2 + digits.len() + 1;
            }
            TAP16..=UP16 => {
                let (Some(&lo), Some(&hi)) = (raw.get(i + 2), raw.get(i + 3)) else { break };
                let kc = u16::from_le_bytes([lo, hi]);
                out.push(match code {
                    TAP16 => Action::Tap(kc),
                    UP16 => Action::Up(kc),
                    _ => Action::Down(kc),
                });
                i += 4;
            }
            _ => i += 2,
        }
    }
    out
}

/// Encode actions. Keycodes above 0xFF don't fit the basic form and are skipped.
pub fn encode(actions: &[Action]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in actions {
        match a {
            Action::Text(t) => out.extend(t.bytes().filter(|&b| b != 0 && b != PREFIX)),
            Action::Tap(kc) | Action::Down(kc) | Action::Up(kc) if *kc <= 0xFF => {
                let code = match a {
                    Action::Tap(_) => TAP,
                    Action::Down(_) => DOWN,
                    _ => UP,
                };
                out.extend_from_slice(&[PREFIX, code, *kc as u8]);
            }
            Action::Delay(ms) if *ms > 0 => {
                out.extend_from_slice(&[PREFIX, DELAY]);
                out.extend_from_slice(ms.to_string().as_bytes());
                out.push(b'|');
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let actions = vec![
            Action::Down(0xE1),
            Action::Tap(0x04),
            Action::Delay(120),
            Action::Up(0xE1),
            Action::Text("gg".into()),
        ];
        assert_eq!(decode(&encode(&actions)), actions);
    }

    #[test]
    fn split_and_join_keep_other_slots() {
        let buf = join(&[b"hi".to_vec(), vec![], encode(&[Action::Tap(0x2C)])], 16).unwrap();
        assert_eq!(buf.len(), 16);
        let slots = split(&buf, 4);
        assert_eq!(slots[0], b"hi");
        assert!(slots[1].is_empty());
        assert_eq!(decode(&slots[2]), vec![Action::Tap(0x2C)]);
        assert!(slots[3].is_empty());
        assert!(join(&[vec![b'x'; 20]], 16).is_none());
    }

    #[test]
    fn sixteen_bit_codes() {
        assert_eq!(decode(&[1, 5, 0x20, 0x52]), vec![Action::Tap(0x5220)]);
    }
}
