//! The tray icon's low-battery mark: the icon's yellow dot, grown and turned
//! red. The dot alone is a couple of pixels in the tray, so it has to be big.

/// Center and radii as fractions of the icon's width.
const CENTER: f32 = 0.74;
const DOT: f32 = 0.25;
const RING: f32 = 0.30;
/// `--bad` in the UI.
const RED: [u8; 3] = [0xe5, 0x57, 0x4e];
/// The icon's own ground, so the ring cuts the dot out of whatever is under it.
const GROUND: [u8; 3] = [0x14, 0x15, 0x17];

/// A copy of an RGBA icon with the mark drawn over its lower right corner.
pub fn low_battery(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = rgba.to_vec();
    let size = width as f32;
    let (cx, cy) = (CENTER * size, CENTER * height as f32);
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % width) as f32 + 0.5, (i as u32 / width) as f32 + 0.5);
        let distance = (x - cx).hypot(y - cy);
        over(px, GROUND, RING * size - distance);
        over(px, RED, DOT * size - distance);
    }
    out
}

/// Blend `color` onto a pixel. `edge` is how far inside the shape the pixel
/// is, so its last pixel of width fades out and the circle stays smooth.
fn over(px: &mut [u8; 4], color: [u8; 3], edge: f32) {
    let cover = (edge + 0.5).clamp(0.0, 1.0);
    for (channel, target) in px.iter_mut().zip(color) {
        *channel = (*channel as f32 + (target as f32 - *channel as f32) * cover).round() as u8;
    }
    px[3] = px[3].max((cover * 255.0).round() as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn marks_the_corner_and_leaves_the_rest() {
        const SIZE: u32 = 32;
        let icon = [10, 20, 30, 255].repeat((SIZE * SIZE) as usize);
        let marked = low_battery(&icon, SIZE, SIZE);
        assert_eq!(marked.len(), icon.len());
        assert_eq!(pixel(&marked, SIZE, 23, 23), [RED[0], RED[1], RED[2], 255]);
        assert_eq!(pixel(&marked, SIZE, 4, 4), [10, 20, 30, 255]);
    }

    #[test]
    fn stays_visible_where_the_icon_is_transparent() {
        const SIZE: u32 = 32;
        let icon = vec![0; (SIZE * SIZE * 4) as usize];
        let marked = low_battery(&icon, SIZE, SIZE);
        assert_eq!(pixel(&marked, SIZE, 23, 23)[3], 255);
        assert_eq!(pixel(&marked, SIZE, 1, 1)[3], 0);
    }
}
