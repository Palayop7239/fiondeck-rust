//! Drawing on the 480x270 panel.
//!
//! A `Canvas` is a small RGB image that you draw on; `into_cmd` turns it into
//! one `Cmd::Framebuffer`, which the worker sends as a frame buffer write
//! followed by a refresh. Everything here ends up going through that one path,
//! so text, icons and (later) GIF frames all work the same way.
//!
//! Colours are 8 bits per channel here, like CSS. The panel gets RGB565.

use std::fs;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use crate::messages::Cmd;
use crate::surface;

/// A colour with 8 bits per channel.
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

    /// Mix `self` over `other`, where `alpha` is 0 (fully `other`) to 255.
    fn blend(self, other: Rgb, alpha: u8) -> Rgb {
        let alpha = alpha as u16;
        let keep = 255 - alpha;

        let mix = |under: u8, over: u8| -> u8 {
            ((under as u16 * keep + over as u16 * alpha) / 255) as u8
        };

        Rgb::new(
            mix(other.r, self.r),
            mix(other.g, self.g),
            mix(other.b, self.b),
        )
    }
}

/// An image you can draw on before sending it to the panel.
pub struct Canvas {
    pub width: u16,
    pub height: u16,
    pixels: Vec<Rgb>,
}

impl Canvas {
    /// A canvas filled with one colour.
    pub fn new(width: u16, height: u16, background: Rgb) -> Self {
        let count = width as usize * height as usize;

        Self {
            width,
            height,
            pixels: vec![background; count],
        }
    }

    /// One pixel. Out-of-range coordinates are ignored, so a glyph that pokes
    /// over an edge is simply clipped.
    pub fn set(&mut self, x: i32, y: i32, color: Rgb) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }

        self.pixels[y as usize * self.width as usize + x as usize] = color;
    }

    /// One pixel, for reading back what was drawn.
    ///
    /// Only the tests use this today; it is also what the icon step will need
    /// when a drawing has to be shifted or scaled.
    #[allow(dead_code)]
    pub fn get(&self, x: u16, y: u16) -> Rgb {
        self.pixels[y as usize * self.width as usize + x as usize]
    }

    /// Paint a solid rectangle, for panels and backgrounds behind text.
    #[allow(dead_code)]
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u16, height: u16, color: Rgb) {
        for row in y..y + height as i32 {
            for column in x..x + width as i32 {
                self.set(column, row, color);
            }
        }
    }

    /// Draw text with its baseline at `baseline`, starting at `x`.
    ///
    /// Baseline is the line letters sit on: the bottom of "A", not the bottom
    /// of "g". Drawing from the baseline keeps the vertical position fixed
    /// whatever the text is.
    pub fn text(&mut self, x: i32, baseline: i32, text: &str, size: f32, color: Rgb) {
        let Some(font) = font() else {
            return;
        };

        let mut pen = x as f32;

        for character in text.chars() {
            let (metrics, coverage) = font.rasterize(character, size);

            // Glyphs are positioned from the baseline upwards, so the top of
            // the bitmap is `ymin + height` above it.
            let top = baseline - (metrics.ymin + metrics.height as i32);

            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    let alpha = coverage[row * metrics.width + column];
                    if alpha == 0 {
                        continue;
                    }

                    let pixel_x = pen as i32 + metrics.xmin + column as i32;
                    let pixel_y = top + row as i32;

                    let under = self.pixel_or(pixel_x, pixel_y);
                    self.set(pixel_x, pixel_y, color.blend(under, alpha));
                }
            }

            pen += metrics.advance_width;
        }
    }

    /// Width of `text`, in pixels, so it can be centred.
    pub fn text_width(text: &str, size: f32) -> f32 {
        let Some(font) = font() else {
            return 0.0;
        };

        text.chars()
            .map(|character| font.metrics(character, size).advance_width)
            .sum()
    }

    /// Turn the canvas into one frame buffer command, placed at `x`/`y` on the
    /// screen.
    pub fn into_cmd(self, x: u16, y: u16) -> Cmd {
        // A rectangle that hangs off the panel would draw nothing visible and
        // silently desynchronise the pixel count, so catch it in debug builds.
        debug_assert!(x as u32 + self.width as u32 <= surface::SCREEN_WIDTH as u32);
        debug_assert!(y as u32 + self.height as u32 <= surface::SCREEN_HEIGHT as u32);

        let mut pixels = Vec::with_capacity(self.pixels.len() * 2);

        for color in &self.pixels {
            let bytes = color.bytes();
            pixels.push(bytes[0]);
            pixels.push(bytes[1]);
        }

        Cmd::Framebuffer {
            x,
            y,
            width: self.width,
            height: self.height,
            pixels,
        }
    }

    fn pixel_or(&self, x: i32, y: i32) -> Rgb {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return Rgb::new(0, 0, 0);
        }

        self.pixels[y as usize * self.width as usize + x as usize]
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

/// One key: a solid background with `label` centred in it.
pub fn label_key(index: u8, background: Rgb, label: &str, foreground: Rgb, size: f32) -> Cmd {
    let mut canvas = Canvas::new(surface::KEY_SIZE, surface::KEY_SIZE, background);

    let width = Canvas::text_width(label, size);

    // Baseline placed so the label sits roughly in the middle of the square.
    let x = (surface::KEY_SIZE as f32 - width) / 2.0;
    let baseline = (surface::KEY_SIZE as f32 + size * 0.7) / 2.0;

    canvas.text(x as i32, baseline as i32, label, size, foreground);

    let (origin_x, origin_y) = key_origin(index);
    canvas.into_cmd(origin_x, origin_y)
}

/// One colour per key, each showing its own number.
///
/// The order is deliberate: the first five are pure hues, the next four mix
/// channels, and the rest are dark or grey. If a key shows the wrong colour or
/// the wrong number, the grid offset or the key size is wrong.
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

/// The paint test: every key in its palette colour, with its number on it.
pub fn paint_test() -> Vec<Cmd> {
    PALETTE
        .iter()
        .enumerate()
        .map(|(index, color)| {
            // Dark colours get light text and the other way round.
            let bright = color.r as u32 + color.g as u32 + color.b as u32 > 380;
            let text_color = if bright {
                Rgb::new(0, 0, 0)
            } else {
                Rgb::new(255, 255, 255)
            };

            label_key(index as u8, *color, &index.to_string(), text_color, 52.0)
        })
        .collect()
}

/// The font used for labels.
///
/// Loaded once, from the system, on first use. A real app would ship its own
/// font; this keeps the repo free of font files while the drawing code is
/// still being built.
fn font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();

    FONT.get_or_init(load_font).as_ref()
}

fn load_font() -> Option<Font> {
    // First one that exists and parses.
    const CANDIDATES: [&str; 5] = [
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ];

    for path in CANDIDATES {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };

        match Font::from_bytes(bytes, FontSettings::default()) {
            Ok(font) => {
                println!("Using font {path}");
                return Some(font);
            }
            Err(error) => eprintln!("could not read {path}: {error}"),
        }
    }

    eprintln!("no usable system font found; labels will be blank");
    None
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
    fn a_filled_canvas_becomes_one_command() {
        let mut canvas = Canvas::new(4, 3, Rgb::new(0, 0, 0));
        canvas.fill_rect(1, 1, 2, 1, Rgb::new(255, 0, 0));

        assert_eq!(canvas.get(0, 0), Rgb::new(0, 0, 0));
        assert_eq!(canvas.get(1, 1), Rgb::new(255, 0, 0));
        assert_eq!(canvas.get(2, 1), Rgb::new(255, 0, 0));
        assert_eq!(canvas.get(3, 1), Rgb::new(0, 0, 0));

        match canvas.into_cmd(15, 90) {
            Cmd::Framebuffer {
                x,
                y,
                width,
                height,
                pixels,
            } => {
                assert_eq!((x, y), (15, 90));
                assert_eq!((width, height), (4, 3));
                assert_eq!(pixels.len(), 4 * 3 * 2);
                // Second row, second pixel: pure red, little-endian.
                let offset = (1 * 4 + 1) * 2;
                assert_eq!(pixels[offset..offset + 2], [0x00, 0xf8]);
            }
            other => panic!("expected a framebuffer command, got {other:?}"),
        }
    }

    #[test]
    fn drawing_outside_the_canvas_is_clipped() {
        let mut canvas = Canvas::new(2, 2, Rgb::new(0, 0, 0));

        canvas.set(-1, 0, Rgb::new(255, 255, 255));
        canvas.set(0, -1, Rgb::new(255, 255, 255));
        canvas.set(2, 0, Rgb::new(255, 255, 255));
        canvas.set(0, 2, Rgb::new(255, 255, 255));

        assert_eq!(canvas.get(0, 0), Rgb::new(0, 0, 0));
    }

    #[test]
    fn a_full_key_label_is_one_key_sized_command() {
        match label_key(6, Rgb::new(0, 0, 0), "6", Rgb::new(255, 255, 255), 52.0) {
            Cmd::Framebuffer {
                x,
                y,
                width,
                height,
                pixels,
            } => {
                assert_eq!((x, y), (15 + 90, 90));
                assert_eq!((width, height), (90, 90));
                assert_eq!(pixels.len(), 90 * 90 * 2);
            }
            other => panic!("expected a framebuffer command, got {other:?}"),
        }
    }

    #[test]
    fn the_paint_test_covers_every_key() {
        assert_eq!(paint_test().len(), 15);
    }
}
