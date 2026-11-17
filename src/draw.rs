//! Drawing on the 480x270 panel.
//!
//! A `Canvas` is a small RGB image that you draw on; `into_cmd` turns it into
//! one `Cmd::Framebuffer`, which the worker sends as a frame buffer write
//! followed by a refresh. Everything here ends up going through that one path,
//! so text, icons and (later) GIF frames all work the same way.
//!
//! Colours are 8 bits per channel here, like CSS. The panel gets RGB565.

use std::fs;
use std::path::Path;
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

    /// Read a `"#rrggbb"` colour, as written in `bindings.toml`.
    ///
    /// Strict on purpose: six digits with a leading `#`, so a typo is reported
    /// instead of quietly turning into black.
    pub fn from_hex(text: &str) -> Result<Rgb, String> {
        let digits = text.strip_prefix('#').unwrap_or(text);

        // Checking ASCII first matters: slicing a non-ASCII string at a fixed
        // byte offset can panic.
        if !digits.is_ascii() || digits.len() != 6 {
            return Err(format!(
                "colour {text:?} should look like \"#2f6f4f\" (six hex digits)"
            ));
        }

        let channel = |start: usize| -> Result<u8, String> {
            u8::from_str_radix(&digits[start..start + 2], 16)
                .map_err(|_| format!("colour {text:?} is not hexadecimal"))
        };

        Ok(Rgb::new(channel(0)?, channel(2)?, channel(4)?))
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

    /// Draw an image file inside a box, keeping its shape.
    ///
    /// The picture is scaled to fit `width x height` and centred, so a wide
    /// photo gets empty space above and below rather than being squashed. Any
    /// transparency in the file is blended over what is already on the canvas,
    /// which is what makes PNG icons with a soft edge look right.
    pub fn draw_image(
        &mut self,
        path: &Path,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
    ) -> Result<(), String> {
        let (scaled, origin_x, origin_y) = fitted_image(path, x, y, width, height)?;

        for (column, row, pixel) in scaled.enumerate_pixels() {
            let [r, g, b, alpha] = pixel.0;
            if alpha == 0 {
                continue;
            }

            let pixel_x = origin_x + column as i32;
            let pixel_y = origin_y + row as i32;

            let color = Rgb::new(r, g, b);
            let under = self.pixel_or(pixel_x, pixel_y);

            self.set(pixel_x, pixel_y, color.blend(under, alpha));
        }

        Ok(())
    }

    /// Draw a one-colour icon in `color`, using the file's transparency only.
    ///
    /// This is the difference between a photo and an icon. An icon file is a
    /// shape, not a picture: `assets/icons` are black or white glyphs with a
    /// transparent background, so a black one vanishes on a dark key and a
    /// white one vanishes on a light key. Ignoring the stored colour and
    /// painting the shape in the colour the caller asks for makes each icon
    /// work on any background, and lets one glyph be green when it is on and
    /// grey when it is off.
    pub fn draw_icon(
        &mut self,
        path: &Path,
        x: i32,
        y: i32,
        width: u16,
        height: u16,
        color: Rgb,
    ) -> Result<(), String> {
        let (scaled, origin_x, origin_y) = fitted_image(path, x, y, width, height)?;

        for (column, row, pixel) in scaled.enumerate_pixels() {
            let alpha = pixel.0[3];
            if alpha == 0 {
                continue;
            }

            let pixel_x = origin_x + column as i32;
            let pixel_y = origin_y + row as i32;

            let under = self.pixel_or(pixel_x, pixel_y);

            self.set(pixel_x, pixel_y, color.blend(under, alpha));
        }

        Ok(())
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

    /// Draw one line horizontally centred, in the band `top..top + height`.
    ///
    /// The baseline is nudged by `size * 0.7` because that is roughly how far
    /// the visible top of a capital letter sits below the top of the line.
    pub fn text_centered(
        &mut self,
        text: &str,
        size: f32,
        color: Rgb,
        top: i32,
        height: u16,
    ) {
        let width = Self::text_width(text, size);
        let x = (self.width as f32 - width) / 2.0;
        let baseline = top as f32 + (height as f32 + size * 0.7) / 2.0;

        self.text(x as i32, baseline as i32, text, size, color);
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

/// Load an image and scale it to fit a box, returning it with where it goes.
///
/// Kept separate from `Canvas` because two callers want the same pixels for
/// different reasons: a photo keeps its colours, an icon only keeps its shape.
fn fitted_image(
    path: &Path,
    x: i32,
    y: i32,
    width: u16,
    height: u16,
) -> Result<(image::RgbaImage, i32, i32), String> {
    let source =
        image::open(path).map_err(|error| format!("could not open {}: {error}", path.display()))?;

    // Work in RGBA so a PNG with transparency keeps it.
    let source = source.to_rgba8();
    let (source_width, source_height) = source.dimensions();

    if source_width == 0 || source_height == 0 {
        return Err(format!("{} has no pixels", path.display()));
    }

    // One scale factor for both axes, so the shape is kept. `min` means the
    // longer side decides it, which is what "fit" means.
    let scale = (width as f32 / source_width as f32).min(height as f32 / source_height as f32);
    let scaled_width = ((source_width as f32 * scale).round() as u32).max(1);
    let scaled_height = ((source_height as f32 * scale).round() as u32).max(1);

    // Triangle is a cheap smooth filter: better than nearest-neighbour on
    // photos, and fast enough for a 90 px key.
    let scaled = image::imageops::resize(
        &source,
        scaled_width,
        scaled_height,
        image::imageops::FilterType::Triangle,
    );

    // Centre the result in the box.
    let origin_x = x + (width as i32 - scaled_width as i32) / 2;
    let origin_y = y + (height as i32 - scaled_height as i32) / 2;

    Ok((scaled, origin_x, origin_y))
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

    canvas.text_centered(label, size, foreground, 0, surface::KEY_SIZE);

    let (origin_x, origin_y) = key_origin(index);
    canvas.into_cmd(origin_x, origin_y)
}

/// One key: an image centred on a solid background.
///
/// The key's colour shows through wherever the image is transparent, and
/// around it if the image is not square. Bindings will call this once keys can
/// carry an icon; only the tests use it until then.
#[allow(dead_code)]
pub fn image_key(index: u8, background: Rgb, path: &Path) -> Result<Cmd, String> {
    let mut canvas = Canvas::new(surface::KEY_SIZE, surface::KEY_SIZE, background);
    canvas.draw_image(path, 0, 0, surface::KEY_SIZE, surface::KEY_SIZE)?;

    let (origin_x, origin_y) = key_origin(index);
    Ok(canvas.into_cmd(origin_x, origin_y))
}

/// The whole screen as one image, for `--image`.
///
/// A full 480x270 frame is the best look at how scaling copes: a 259 KB frame
/// arrives in one piece and any aspect-ratio mistake is obvious.
pub fn image_test(path: &Path) -> Result<Cmd, String> {
    let mut canvas = Canvas::new(
        surface::SCREEN_WIDTH,
        surface::SCREEN_HEIGHT,
        Rgb::new(0, 0, 0),
    );

    canvas.draw_image(
        path,
        0,
        0,
        surface::SCREEN_WIDTH,
        surface::SCREEN_HEIGHT,
    )?;

    Ok(canvas.into_cmd(0, 0))
}

/// Space left around an icon inside a key.
///
/// 90 - 2 * 9 = 72, which is exactly the size of the icons in `assets/icons`,
/// so they land on the panel 1:1 with no resampling and stay perfectly sharp.
pub const ICON_PADDING: u16 = 9;

/// One key: a one-colour icon centred on a solid background.
pub fn icon_key(index: u8, background: Rgb, path: &Path, color: Rgb) -> Result<Cmd, String> {
    let mut canvas = Canvas::new(surface::KEY_SIZE, surface::KEY_SIZE, background);
    let inner = surface::KEY_SIZE - 2 * ICON_PADDING;

    canvas.draw_icon(
        path,
        ICON_PADDING as i32,
        ICON_PADDING as i32,
        inner,
        inner,
        color,
    )?;

    let (origin_x, origin_y) = key_origin(index);
    Ok(canvas.into_cmd(origin_x, origin_y))
}

/// The icons in `assets/icons`, one per key, in different colour pairs.
///
/// Reads them off disk at run time, so a missing or unreadable file fails
/// loudly instead of silently leaving a black key.
pub fn icon_test() -> Result<Vec<Cmd>, String> {
    const WHITE: Rgb = Rgb::new(255, 255, 255);
    const BLACK: Rgb = Rgb::new(0, 0, 0);
    const GREY: Rgb = Rgb::new(40, 40, 40);
    const GREEN: Rgb = Rgb::new(47, 111, 79);
    const RED: Rgb = Rgb::new(200, 60, 60);
    const MINT: Rgb = Rgb::new(120, 255, 170);
    const LIGHT: Rgb = Rgb::new(235, 235, 235);

    // (file, background, icon colour): white ink on dark keys, and one dark
    // ink on a light key, because the files come in both flavours.
    let demos: [(&str, Rgb, Rgb); 5] = [
        ("assets/icons/headphones-off.png", GREY, WHITE),
        ("assets/icons/headphones-on.png", GREEN, WHITE),
        ("assets/icons/mic-off.png", GREY, RED),
        ("assets/icons/mic-on.png", GREY, MINT),
        ("assets/icons/mixer-icon.png", LIGHT, BLACK),
    ];

    demos
        .iter()
        .enumerate()
        .map(|(index, (file, background, color))| {
            icon_key(index as u8, *background, Path::new(file), *color)
        })
        .collect()
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
    use std::path::PathBuf;

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
    fn hex_colours_are_read_strictly() {
        assert_eq!(Rgb::from_hex("#2f6f4f"), Ok(Rgb::new(0x2f, 0x6f, 0x4f)));
        assert_eq!(Rgb::from_hex("2f6f4f"), Ok(Rgb::new(0x2f, 0x6f, 0x4f)));

        // A typo must be reported, not turned into black.
        assert!(Rgb::from_hex("orange").is_err());
        assert!(Rgb::from_hex("#12345").is_err());
        assert!(Rgb::from_hex("#gggggg").is_err());
        // Non-ASCII is six bytes too, so this would panic if not guarded.
        assert!(Rgb::from_hex("#é1234").is_err());
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

    /// Write a solid rectangle of colour as a PNG, for the image tests.
    ///
    /// `save` picks the format from the extension, so the file must end in
    /// `.png`. It goes in the system temp folder and the caller deletes it.
    fn write_test_image(name: &str, width: u32, height: u32, pixel: [u8; 4]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("fiondeck-{}-{name}.png", std::process::id()));
        let buffer = image::RgbaImage::from_pixel(width, height, image::Rgba(pixel));

        buffer.save(&path).expect("could not write the test image");
        path
    }

    #[test]
    fn a_wide_image_keeps_its_shape() {
        // A 40x20 picture drawn into a 20x20 square becomes 20x10, centred:
        // rows 5..15, with the background visible above and below.
        let path = write_test_image("wide", 40, 20, [255, 0, 0, 255]);
        let background = Rgb::new(0, 0, 255);

        let mut canvas = Canvas::new(20, 20, background);
        canvas.draw_image(&path, 0, 0, 20, 20).expect("draw");

        assert_eq!(canvas.get(0, 4), background, "above the picture");
        assert_eq!(canvas.get(0, 5), Rgb::new(255, 0, 0), "first row");
        assert_eq!(canvas.get(19, 14), Rgb::new(255, 0, 0), "last row");
        assert_eq!(canvas.get(0, 15), background, "below the picture");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_tall_image_keeps_its_shape() {
        // 20x40 into 20x20 becomes 10x20, centred: columns 5..15.
        let path = write_test_image("tall", 20, 40, [0, 255, 0, 255]);
        let background = Rgb::new(0, 0, 0);

        let mut canvas = Canvas::new(20, 20, background);
        canvas.draw_image(&path, 0, 0, 20, 20).expect("draw");

        assert_eq!(canvas.get(4, 0), background, "left of the picture");
        assert_eq!(canvas.get(5, 0), Rgb::new(0, 255, 0), "first column");
        assert_eq!(canvas.get(14, 19), Rgb::new(0, 255, 0), "last column");
        assert_eq!(canvas.get(15, 0), background, "right of the picture");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn transparent_pixels_leave_the_key_alone() {
        let path = write_test_image("clear", 8, 8, [0, 255, 0, 0]);
        let background = Rgb::new(10, 20, 30);

        let mut canvas = Canvas::new(8, 8, background);
        canvas.draw_image(&path, 0, 0, 8, 8).expect("draw");

        assert_eq!(canvas.get(4, 4), background);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_image_is_an_error_not_a_panic() {
        let mut canvas = Canvas::new(8, 8, Rgb::new(0, 0, 0));

        let result = canvas.draw_image(Path::new("does-not-exist.png"), 0, 0, 8, 8);

        assert!(result.is_err());
    }

    #[test]
    fn an_icon_key_fills_the_square() {
        let path = write_test_image("key", 32, 32, [255, 255, 0, 255]);
        let background = Rgb::new(0, 0, 0);

        match image_key(2, background, &path).expect("draw") {
            Cmd::Framebuffer {
                x,
                y,
                width,
                height,
                pixels,
            } => {
                assert_eq!((x, y), (15 + 2 * 90, 0));
                assert_eq!((width, height), (90, 90));
                assert_eq!(pixels.len(), 90 * 90 * 2);
                // A square image leaves no background, so the first pixel is
                // already the picture: yellow, little-endian.
                assert_eq!(pixels[0..2], Rgb::new(255, 255, 0).bytes());
            }
            other => panic!("expected a framebuffer command, got {other:?}"),
        }

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_icon_is_painted_in_the_colour_you_ask_for() {
        // The file is black, but a black glyph is invisible on a dark key, so
        // the stored colour must be ignored and the shape drawn in the tint.
        let path = write_test_image("black-glyph", 4, 4, [0, 0, 0, 255]);
        let background = Rgb::new(255, 255, 255);
        let tint = Rgb::new(0, 200, 0);

        let mut canvas = Canvas::new(10, 10, background);
        canvas
            .draw_icon(&path, 3, 3, 4, 4, tint)
            .expect("draw");

        assert_eq!(canvas.get(3, 3), tint, "top-left of the glyph");
        assert_eq!(canvas.get(6, 6), tint, "bottom-right of the glyph");
        assert_eq!(canvas.get(2, 3), background, "left of the box");
        assert_eq!(canvas.get(7, 7), background, "right of the box");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn an_icon_key_leaves_a_margin_around_the_icon() {
        // 72 px of glyph in a 90 px key leaves 9 px of background on every
        // side, and needs no scaling at all.
        let path = write_test_image("glyph", 72, 72, [0, 0, 0, 255]);
        let background = Rgb::new(0, 0, 255);
        let tint = Rgb::new(255, 255, 255);

        match icon_key(0, background, &path, tint).expect("draw") {
            Cmd::Framebuffer {
                width,
                height,
                pixels,
                ..
            } => {
                assert_eq!((width, height), (90, 90));

                let pixel_at = |x: usize, y: usize| {
                    let offset = (y * 90 + x) * 2;
                    [pixels[offset], pixels[offset + 1]]
                };

                assert_eq!(pixel_at(0, 0), background.bytes(), "corner");
                assert_eq!(pixel_at(8, 8), background.bytes(), "just inside");
                assert_eq!(pixel_at(9, 9), tint.bytes(), "glyph starts");
                assert_eq!(pixel_at(80, 80), tint.bytes(), "glyph ends");
                assert_eq!(pixel_at(81, 81), background.bytes(), "margin again");
                assert_eq!(pixel_at(89, 89), background.bytes(), "far corner");
            }
            other => panic!("expected a framebuffer command, got {other:?}"),
        }

        let _ = std::fs::remove_file(path);
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
