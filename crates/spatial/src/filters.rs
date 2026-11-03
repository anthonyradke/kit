//! The virtual speaker filters built into kit (`data/ku100.bin`, made by
//! `scripts/make_spatial_filters.py` from the SADIE II KU100 measurements).

use std::sync::OnceLock;

const DATA: &[u8] = include_bytes!("../data/ku100.bin");
const MAGIC: &[u8; 8] = b"KITSPAT1";

/// One measured direction.
pub struct Direction {
    /// Degrees, 0 ahead, positive to the left.
    pub azimuth: i32,
    /// The dummy head's response with no room, per ear (left, right).
    pub direct: [Vec<f32>; 2],
    /// The room's first 35 ms from this direction, without the direct sound.
    pub early: [Vec<f32>; 2],
}

pub struct Filters {
    pub rate: u32,
    pub directions: Vec<Direction>,
    /// The late room, shared by every direction; starts at sample 0 like the others.
    pub tail: [Vec<f32>; 2],
    /// Samples before the direct sound of the frontal direction peaks, so
    /// the LFE channel can be lined up with it.
    pub direct_delay: usize,
}

impl Filters {
    /// The direction closest to `azimuth`.
    pub fn nearest(&self, azimuth: i32) -> usize {
        let dist = |a: i32| {
            let d = (a - azimuth).rem_euclid(360);
            d.min(360 - d)
        };
        (0..self.directions.len()).min_by_key(|&i| dist(self.directions[i].azimuth)).unwrap_or(0)
    }
}

pub fn builtin() -> &'static Filters {
    static FILTERS: OnceLock<Filters> = OnceLock::new();
    FILTERS.get_or_init(|| parse(DATA).expect("built-in spatial filters are valid"))
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.0.len() < n {
            return Err("file ends early".into());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn floats(&mut self, n: usize) -> Result<Vec<f32>, String> {
        Ok(self.take(n * 4)?.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect())
    }

    fn pair(&mut self, n: usize) -> Result<[Vec<f32>; 2], String> {
        Ok([self.floats(n)?, self.floats(n)?])
    }
}

pub fn parse(bytes: &[u8]) -> Result<Filters, String> {
    let mut r = Reader(bytes);
    if r.take(8)? != MAGIC {
        return Err("not a kit spatial filter file".into());
    }
    let rate = r.u32()?;
    let count = r.u32()? as usize;
    let direct_len = r.u32()? as usize;
    let early_len = r.u32()? as usize;
    let tail_len = r.u32()? as usize;
    let _onset = r.u32()?;
    let mut directions = Vec::with_capacity(count);
    for _ in 0..count {
        let azimuth = r.u32()? as i32;
        let direct = r.pair(direct_len)?;
        let early = r.pair(early_len)?;
        directions.push(Direction { azimuth, direct, early });
    }
    let tail = r.pair(tail_len)?;
    if !r.0.is_empty() {
        return Err("trailing data".into());
    }
    if directions.is_empty() {
        return Err("no directions".into());
    }
    let mut filters = Filters { rate, directions, tail, direct_delay: 0 };
    let front = &filters.directions[filters.nearest(0)].direct;
    filters.direct_delay =
        (0..direct_len).max_by(|&a, &b| (front[0][a].powi(2) + front[1][a].powi(2)).total_cmp(&(front[0][b].powi(2) + front[1][b].powi(2)))).unwrap_or(0);
    Ok(filters)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_filters_load() {
        let f = builtin();
        assert_eq!(f.rate, 48000);
        assert_eq!(f.directions.len(), 12);
        assert!(f.directions.iter().all(|d| d.direct[0].len() == 256 && d.early[1].len() == 1680));
        assert_eq!(f.tail[0].len(), 12000);
        assert!(f.direct_delay > 0 && f.direct_delay < 256);
        assert_eq!(f.directions[f.nearest(30)].azimuth, 30);
        assert_eq!(f.directions[f.nearest(-28)].azimuth, -30);
        assert_eq!(f.directions[f.nearest(179)].azimuth, 180);
        assert_eq!(f.directions[f.nearest(-179)].azimuth, 180);
    }

    #[test]
    fn left_is_left() {
        let f = builtin();
        let e = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>();
        let left = &f.directions[f.nearest(100)].direct;
        assert!(e(&left[0]) > 4.0 * e(&left[1]));
        let right = &f.directions[f.nearest(-100)].direct;
        assert!(e(&right[1]) > 4.0 * e(&right[0]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"nope").is_err());
        assert!(parse(&DATA[..DATA.len() - 4]).is_err());
    }
}
