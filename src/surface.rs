//! Physical layout of the Loupedeck Live S, as numbers.
//!
//! The touchscreen is one 480x270 panel, but the firmware and the official
//! app treat it as a grid of keys. Splitting it the same way is what lets a
//! binding say "key 7" instead of "x=198, y=40".
//!
//! Both the GUI and the event loop use these constants, so there is only one
//! place to fix if a measurement turns out to be off.

/// Touchscreen size in pixels.
pub const SCREEN_WIDTH: u16 = 480;
pub const SCREEN_HEIGHT: u16 = 270;

/// Grid the screen is divided into.
pub const COLUMNS: u8 = 5;
pub const ROWS: u8 = 3;

/// Size of one screen key in pixels: 480/5 and 270/3.
pub const KEY_WIDTH: u16 = SCREEN_WIDTH / COLUMNS as u16;
pub const KEY_HEIGHT: u16 = SCREEN_HEIGHT / ROWS as u16;

/// The four physical buttons next to the screen.
pub const BUTTON_IDS: [u8; 4] = [7, 8, 9, 10];

/// The two knobs.
pub const KNOB_IDS: [u8; 2] = [1, 2];

/// Which screen key a touch landed on, if any.
///
/// Touches outside the panel are ignored rather than clamped, so a stray
/// coordinate cannot fire the wrong action.
pub fn key_at(x: u16, y: u16) -> Option<u8> {
    if x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT {
        return None;
    }

    let column = x / KEY_WIDTH;
    let row = y / KEY_HEIGHT;

    Some(row as u8 * COLUMNS + column as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_map_to_first_and_last_key() {
        assert_eq!(key_at(0, 0), Some(0));
        assert_eq!(
            key_at(SCREEN_WIDTH - 1, SCREEN_HEIGHT - 1),
            Some(COLUMNS * ROWS - 1)
        );
    }

    #[test]
    fn keys_are_numbered_row_by_row() {
        assert_eq!(key_at(95, 89), Some(0));
        assert_eq!(key_at(96, 0), Some(1));
        assert_eq!(key_at(0, 90), Some(5));
        assert_eq!(key_at(96, 90), Some(6));
    }

    #[test]
    fn outside_the_panel_is_none() {
        assert_eq!(key_at(SCREEN_WIDTH, 0), None);
        assert_eq!(key_at(0, SCREEN_HEIGHT), None);
    }
}
