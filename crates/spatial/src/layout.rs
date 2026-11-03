//! Where each channel of a Windows multichannel stream goes.

/// `WAVEFORMATEXTENSIBLE.dwChannelMask` bits, in channel order.
const BITS: [(u32, Speaker); 18] = [
    (0x1, Speaker::FrontLeft),
    (0x2, Speaker::FrontRight),
    (0x4, Speaker::FrontCenter),
    (0x8, Speaker::Lfe),
    (0x10, Speaker::BackLeft),
    (0x20, Speaker::BackRight),
    (0x40, Speaker::FrontLeftOfCenter),
    (0x80, Speaker::FrontRightOfCenter),
    (0x100, Speaker::BackCenter),
    (0x200, Speaker::SideLeft),
    (0x400, Speaker::SideRight),
    (0x800, Speaker::FrontCenter),
    (0x1000, Speaker::FrontLeft),
    (0x2000, Speaker::FrontCenter),
    (0x4000, Speaker::FrontRight),
    (0x8000, Speaker::BackLeft),
    (0x10000, Speaker::BackCenter),
    (0x20000, Speaker::BackRight),
];

pub const MASK_STEREO: u32 = 0x3;
pub const MASK_5_1: u32 = 0x3F;
pub const MASK_7_1: u32 = 0x63F;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    FrontLeft,
    FrontRight,
    FrontCenter,
    Lfe,
    BackLeft,
    BackRight,
    FrontLeftOfCenter,
    FrontRightOfCenter,
    BackCenter,
    SideLeft,
    SideRight,
}

impl Speaker {
    pub fn name(self) -> &'static str {
        match self {
            Speaker::FrontLeft => "Front left",
            Speaker::FrontRight => "Front right",
            Speaker::FrontCenter => "Center",
            Speaker::Lfe => "Subwoofer",
            Speaker::BackLeft => "Rear left",
            Speaker::BackRight => "Rear right",
            Speaker::FrontLeftOfCenter => "Front left of center",
            Speaker::FrontRightOfCenter => "Front right of center",
            Speaker::BackCenter => "Rear center",
            Speaker::SideLeft => "Side left",
            Speaker::SideRight => "Side right",
        }
    }
}

/// The speakers of a stream with `channels` channels and channel mask `mask`
/// (0 when the format doesn't say). Channels past the mask are dropped (`None`).
pub fn speakers(channels: usize, mask: u32) -> Vec<Option<Speaker>> {
    let mask = if mask != 0 {
        mask
    } else {
        match channels {
            1 => 0x4,
            2 => MASK_STEREO,
            3 => 0x7,
            4 => 0x33,
            5 => 0x37,
            6 => MASK_5_1,
            7 => 0x13F,
            _ => MASK_7_1,
        }
    };
    let mut out: Vec<Option<Speaker>> = BITS.iter().filter(|(bit, _)| mask & bit != 0).map(|&(_, s)| Some(s)).collect();
    out.resize(channels, None);
    out
}

/// The angle a speaker is placed at (degrees, positive to the left), given
/// the whole layout: surrounds sit at the side in 5.1 and split into side
/// and rear in 7.1. `None` for the subwoofer.
pub fn azimuth(speaker: Speaker, layout: &[Option<Speaker>]) -> Option<i32> {
    let has = |s| layout.contains(&Some(s));
    let both = has(Speaker::SideLeft) && has(Speaker::BackLeft);
    Some(match speaker {
        Speaker::FrontCenter => 0,
        Speaker::FrontLeft => 30,
        Speaker::FrontRight => -30,
        Speaker::FrontLeftOfCenter => 15,
        Speaker::FrontRightOfCenter => -15,
        Speaker::SideLeft if both => 100,
        Speaker::SideRight if both => -100,
        Speaker::SideLeft => 110,
        Speaker::SideRight => -110,
        Speaker::BackLeft if both => 145,
        Speaker::BackRight if both => -145,
        Speaker::BackLeft => 110,
        Speaker::BackRight => -110,
        Speaker::BackCenter => 180,
        Speaker::Lfe => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use Speaker::*;

    #[test]
    fn seven_one_in_windows_order() {
        let l = speakers(8, MASK_7_1);
        assert_eq!(l, [FrontLeft, FrontRight, FrontCenter, Lfe, BackLeft, BackRight, SideLeft, SideRight].map(Some));
        assert_eq!(azimuth(SideLeft, &l), Some(100));
        assert_eq!(azimuth(BackRight, &l), Some(-145));
        assert_eq!(azimuth(Lfe, &l), None);
    }

    #[test]
    fn five_one_surrounds_go_to_the_side() {
        let l = speakers(6, MASK_5_1);
        assert_eq!(azimuth(BackLeft, &l), Some(110));
        let side = speakers(6, 0x60F);
        assert_eq!(side[4], Some(SideLeft));
        assert_eq!(azimuth(SideRight, &side), Some(-110));
    }

    #[test]
    fn missing_or_short_masks() {
        assert_eq!(speakers(2, 0), [Some(FrontLeft), Some(FrontRight)]);
        assert_eq!(speakers(8, 0)[3], Some(Lfe));
        // More channels than the mask names: the extras are dropped.
        assert_eq!(speakers(4, MASK_STEREO), [Some(FrontLeft), Some(FrontRight), None, None]);
    }
}
