//! Drawing on the 480x270 panel.
//!
//! Step 1: solid colours. Everything here ends up as one `Cmd::Framebuffer`,
//! so text and images can be added later without touching the serial side.
//!
//! The panel is addressed in pixels, not keys: a key is just a 90x90 rectangle
//! at a known offset, which is why `key_origin` exists.

use crate::messages::Cmd;
use crate::surface;

/// A colour with 8 bits per channel, like CSS. The panel wants RGB565.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Pack into 5-6-5: red in the top 5 bits, green in the middle 6, blue in
    /// the bottom 5.
    pub fn rgb565(self) -> u16 {
        let r = (self.r as u16 >> 3) & 0x1f;
        let g = (self.g as u16 >> 2) & 0x3f;
        let b = (self.b as u16 >> 3) & 0x1f;

        (r << 11) | (g << 5) | b
    }

    /// The two bytes the panel expects: little-endian.
    pub fn bytes(self) -> [u8; 2] {
        self.rgb565().to_le_bytes()
    }
}

/// Top-left corner of a key, in screen pixels.
pub fn key_origin(index: u8) -> (u16, u16) {
    let column = index % surface::COLUMNS;
    let row = index / surface::COLUMNS;

    (
        surface::KEY_MARGIN_X + column as u16 * surface::KEY_SIZE,
        row as u16 * surface::KEY_SIZE,
    )
}

/// Fill a rectangle with one colour.
pub fn fill_rect(x: u16, y: u16, width: u16, height: u16, color: Rgb) -> Cmd {
    debug_assert!(x as u32 + width as u32 <= surface::SCREEN_WIDTH as u32);
    debug_assert!(y as u32 + height as u32 <= surface::SCREEN_HEIGHT as u32);

    let pixel = color.bytes();
    let count = width as usize * height as usize;

    let mut pixels = Vec::with_capacity(count * 2);
    for _ in 0..count {
        pixels.push(pixel[0]);
        pixels.push(pixel[1]);
    }

    Cmd::Framebuffer {
        x,
        y,
        width,
        height,
        pixels,
    }
}

/// Fill one key with a solid colour.
pub fn fill_key(index: u8, color: Rgb) -> Cmd {
    let (x, y) = key_origin(index);

    fill_rect(x, y, surface::KEY_SIZE, surface::KEY_SIZE, color)
}

/// One colour per key, to check the layout by eye.
///
/// The order is deliberate: the first five are pure hues, the next four mix
/// channels, and the rest are dark or grey. If a key shows the wrong one, the
/// grid offset or the key size is wrong.
pub const PALETTE: [Rgb; 15] = [
    Rgb::new(255, 0, 0),     // key 0  red
    Rgb::new(255, 128, 0),   // key 1  orange
    Rgb::new(255, 255, 0),   // key 2  yellow
    Rgb::new(0, 255, 0),     // key 3  green
    Rgb::new(0, 255, 255),   // key 4  cyan
    Rgb::new(0, 0, 255),     // key 5  blue
    Rgb::new(128, 0, 255),   // key 6  purple
    Rgb::new(255, 0, 255),   // key 7  magenta
    Rgb::new(255, 255, 255), // key 8  white
    Rgb::new(128, 128, 128), // key 9  grey
    Rgb::new(128, 0, 0),     // key 10 dark red
    Rgb::new(0, 128, 0),     // key 11 dark green
    Rgb::new(0, 0, 128),     // key 12 dark blue
    Rgb::new(0, 0, 0),       // key 13 black
    Rgb::new(192, 192, 192), // key 14 light grey
];

/// One command per key, each in its own palette colour.
pub fn paint_test() -> Vec<Cmd> {
    PALETTE
        .iter()
        .enumerate()
        .map(|(index, color)| fill_key(index as u8, *color))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_pack_as_5_6_5_little_endian() {
        assert_eq!(Rgb::new(255, 0, 0).bytes(), [0x00, 0xf8]);
        assert_eq!(Rgb::new(0, 255, 0).bytes(), [0xe0, 0x07]);
        assert_eq!(Rgb::new(0, 0, 255).bytes(), [0x1f, 0x00]);
        assert_eq!(Rgb::new(255, 255, 255).bytes(), [0xff, 0xff]);
        assert_eq!(Rgb::new(0, 0, 0).bytes(), [0x00, 0x00]);
    }

    #[test]
    fn keys_start_after_the_margin() {
        assert_eq!(key_origin(0), (15, 0));
        assert_eq!(key_origin(4), (15 + 4 * 90, 0));
        assert_eq!(key_origin(5), (15, 90));
        assert_eq!(key_origin(14), (15 + 4 * 90, 180));
    }

    #[test]
    fn a_filled_key_has_one_pixel_pair_per_pixel() {
        let command = fill_key(0, Rgb::new(255, 0, 0));

        match command {
            Cmd::Framebuffer {
                x,
                y,
                width,
                height,
                pixels,
            } => {
                assert_eq!((x, y), (15, 0));
                assert_eq!((width, height), (90, 90));
                assert_eq!(pixels.len(), 90 * 90 * 2);
                assert_eq!(pixels[0..2], [0x00, 0xf8]);
                assert_eq!(pixels[pixels.len() - 2..], [0x00, 0xf8]);
            }
            other => panic!("expected a framebuffer command, got {other:?}"),
        }
    }

    #[test]
    fn the_paint_test_covers_every_key() {
        assert_eq!(paint_test().len(), 15);
    }
}
