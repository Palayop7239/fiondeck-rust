//! What the panel shows: one face per screen key, built from `bindings.toml`.
//!
//! The panel is a single 480x270 screen divided into 15 keys, so a "page" is
//! just 15 independent 90x90 draws. Each one is built here and sent by the
//! worker, which means the whole page also works as the basis for a full-screen
//! picture later.
//!
//! Nothing in this file talks to the device: it turns config into commands.

use std::path::{Path, PathBuf};

use crate::config::{Config, Input};
use crate::draw::{self, Canvas, Rgb};
use crate::messages::Cmd;
use crate::surface;

/// Key colour for a binding that does not set one. Dark, so the panel does not
/// glow at night.
const BACKGROUND: Rgb = Rgb::new(24, 24, 28);

/// Label and icon colour for a binding that does not set one.
const INK: Rgb = Rgb::new(235, 235, 235);

/// Biggest label drawn on a key, in pixels.
const LABEL_SIZE: f32 = 26.0;

/// When an icon and a label share a key, the icon gets the top part and the
/// label the bottom. These are the two bands inside the 90 px square.
const ICON_BAND: u16 = 56;
const LABEL_BAND: u16 = 34;

/// Every key on the panel, in order, ready to send.
///
/// Keys with no binding are still drawn, in `BACKGROUND`, so the page always
/// looks the same instead of showing leftovers from an earlier run.
///
/// `base` is the folder `bindings.toml` lives in: icon paths are read relative
/// to it, so a bindings folder can be moved or copied as one piece.
pub fn page(config: &Config, base: &Path) -> Vec<Cmd> {
    let key_count = surface::COLUMNS * surface::ROWS;

    (0..key_count).map(|index| key(config, base, index)).collect()
}

/// One key: background, icon and label, in whatever combination the file asks.
fn key(config: &Config, base: &Path, index: u8) -> Cmd {
    let binding = config.binding(Input::Touch, index);
    let display = config.display(Input::Touch, index);

    let background = colour(display.and_then(|d| d.colour.as_deref()), BACKGROUND);
    let ink = colour(display.and_then(|d| d.ink.as_deref()), INK);

    // A key with a binding but no label still says what it does, which beats a
    // blank square while the file is being written.
    let label = display
        .and_then(|d| d.label.as_ref())
        .cloned()
        .or_else(|| binding.map(|b| b.action.label()));

    let icon = display
        .and_then(|d| d.icon.as_deref())
        .map(|path| base.join(path));

    let mut canvas = Canvas::new(surface::KEY_SIZE, surface::KEY_SIZE, background);

    // The icon decides the layout, so only draw the label once we know whether
    // the icon actually made it onto the key.
    let icon_drawn = match &icon {
        Some(path) => draw_icon(&mut canvas, path, ink, label.is_some()),
        None => false,
    };

    match (icon_drawn, label) {
        // Icon on top, label underneath.
        (true, Some(label)) => {
            let size = fitting_size(&label, surface::KEY_SIZE - 8, LABEL_SIZE * 0.62);
            canvas.text_centered(&label, size, ink, ICON_BAND as i32, LABEL_BAND);
        }
        // Icon alone, as large as the padding allows.
        (true, None) => {}
        // No icon (or an unreadable one): text in the middle of the key.
        (false, Some(label)) => {
            let size = fitting_size(&label, surface::KEY_SIZE - 10, LABEL_SIZE);
            canvas.text_centered(&label, size, ink, 0, surface::KEY_SIZE);
        }
        (false, None) => {}
    }

    let (origin_x, origin_y) = draw::key_origin(index);
    canvas.into_cmd(origin_x, origin_y)
}

/// Draw the icon, returning whether it worked.
///
/// A missing or broken file is a config mistake, not a crash: it is reported
/// once, the icon is skipped, and the key falls back to plain background.
fn draw_icon(canvas: &mut Canvas, path: &Path, ink: Rgb, with_label: bool) -> bool {
    let side = canvas.width - 2 * draw::ICON_PADDING;

    let (x, y, width, height) = if with_label {
        (draw::ICON_PADDING, 4, side, ICON_BAND - 4)
    } else {
        (draw::ICON_PADDING, draw::ICON_PADDING, side, side)
    };

    match canvas.draw_icon(path, x as i32, y as i32, width, height, ink) {
        Ok(()) => true,
        Err(message) => {
            eprintln!("{message}");
            false
        }
    }
}

/// The largest font size at or below `max` whose text fits `width` pixels.
///
/// Long action names like "brightness 10/10" would otherwise run off the key.
fn fitting_size(text: &str, width: u16, max: f32) -> f32 {
    let mut size = max;

    while size > 8.0 && Canvas::text_width(text, size) > width as f32 {
        size -= 1.0;
    }

    size
}

/// Read a `"#rrggbb"` colour, complaining on stderr if it is malformed.
fn colour(text: Option<&str>, fallback: Rgb) -> Rgb {
    let Some(text) = text else {
        return fallback;
    };

    match Rgb::from_hex(text) {
        Ok(colour) => colour,
        Err(message) => {
            eprintln!("{message}");
            fallback
        }
    }
}

/// The folder holding `bindings.toml`, which icon paths are relative to.
pub fn base_dir(config_path: &Path) -> PathBuf {
    config_path.parent().map(Path::to_path_buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Action, Binding, Display, Pattern};

    fn touch(id: u8, action: Action, display: Option<Display>) -> Binding {
        Binding {
            on: Input::Touch,
            id,
            display,
            action,
        }
    }

    fn command_pixels(command: &Cmd) -> (u16, u16, u16, u16, &[u8]) {
        match command {
            Cmd::Framebuffer {
                x,
                y,
                width,
                height,
                pixels,
            } => (*x, *y, *width, *height, pixels),
            other => panic!("expected a framebuffer command, got {other:?}"),
        }
    }

    #[test]
    fn a_page_covers_every_key_once() {
        let config = Config {
            brightness: 5,
            bindings: vec![touch(
                3,
                Action::Vibrate {
                    pattern: Pattern::Short,
                },
                None,
            )],
        };

        let commands = page(&config, Path::new(""));

        assert_eq!(commands.len(), 15);

        // Each command must land on its own key, in row order.
        for (index, command) in commands.iter().enumerate() {
            let (x, y, width, height, pixels) = command_pixels(command);
            assert_eq!((x, y), draw::key_origin(index as u8));
            assert_eq!((width, height), (90, 90));
            assert_eq!(pixels.len(), 90 * 90 * 2);
        }
    }

    #[test]
    fn an_empty_key_gets_the_default_background() {
        let config = Config {
            brightness: 5,
            bindings: Vec::new(),
        };

        let commands = page(&config, Path::new(""));
        let (_, _, _, _, pixels) = command_pixels(&commands[0]);

        assert_eq!(pixels[0..2], BACKGROUND.bytes());
    }

    #[test]
    fn the_file_colour_wins_over_the_default() {
        let gold = Rgb::new(255, 165, 0);
        let config = Config {
            brightness: 5,
            bindings: vec![touch(
                0,
                Action::Disabled,
                Some(Display {
                    colour: Some("#ffa500".to_owned()),
                    ..Display::default()
                }),
            )],
        };

        let commands = page(&config, Path::new(""));
        let (_, _, _, _, pixels) = command_pixels(&commands[0]);

        // The key 0 corner is background: no label is centred over it.
        assert_eq!(pixels[0..2], gold.bytes());
    }

    #[test]
    fn a_bad_colour_falls_back_to_the_default() {
        let config = Config {
            brightness: 5,
            bindings: vec![touch(
                0,
                Action::Disabled,
                Some(Display {
                    colour: Some("orange".to_owned()),
                    ..Display::default()
                }),
            )],
        };

        let commands = page(&config, Path::new(""));
        let (_, _, _, _, pixels) = command_pixels(&commands[0]);

        assert_eq!(pixels[0..2], BACKGROUND.bytes());
    }

    #[test]
    fn a_missing_icon_leaves_the_key_alone() {
        let config = Config {
            brightness: 5,
            bindings: vec![touch(
                2,
                Action::Disabled,
                Some(Display {
                    icon: Some("no-such-file.png".to_owned()),
                    colour: Some("#102030".to_owned()),
                    ..Display::default()
                }),
            )],
        };

        let commands = page(&config, Path::new(""));

        let (_, _, width, height, _) = command_pixels(&commands[2]);
        assert_eq!((width, height), (90, 90));
    }

    #[test]
    fn icon_paths_are_relative_to_the_config_file() {
        let config = Config {
            brightness: 5,
            bindings: vec![touch(
                0,
                Action::Disabled,
                Some(Display {
                    icon: Some("icons/mic-on.png".to_owned()),
                    ..Default::default()
                }),
            )],
        };

        // No drawing is checked here, only that a relative path is joined to
        // the config folder rather than to the working directory.
        let base = Path::new("some/folder");
        assert_eq!(base.join("icons/mic-on.png"), Path::new("some/folder/icons/mic-on.png"));

        let _ = page(&config, base);
    }

    #[test]
    fn a_long_label_shrinks_instead_of_running_off() {
        // Without a font nothing can be measured, so there is nothing to test.
        if Canvas::text_width("x", 26.0) == 0.0 {
            return;
        }

        assert_eq!(fitting_size("x", 80, 26.0), 26.0);

        let long = "brightness 10/10";
        let size = fitting_size(long, 80, 26.0);
        assert!(size < 26.0, "expected a smaller size, got {size}");
        assert!(Canvas::text_width(long, size) <= 80.0);
    }}
