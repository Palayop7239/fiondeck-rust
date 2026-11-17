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

/// Size of one screen key in pixels. Keys are square, and on the Live S there
/// is a 15 px dead margin at each end of the row (15 + 5*90 + 15 = 480).
///
/// Source: `device.js` in foxxyz/loupedeck — `keySize = 90`, `visibleX =
/// [15, 465]`.
pub const KEY_SIZE: u16 = 90;
pub const KEY_MARGIN_X: u16 = 15;

/// The four physical buttons next to the screen.
pub const BUTTON_IDS: [u8; 4] = [7, 8, 9, 10];

/// The two knobs.
pub const KNOB_IDS: [u8; 2] = [1, 2];

/// Which screen key a touch landed on, if any.
///
/// Touches outside the keys are ignored rather than clamped, so a stray
/// coordinate cannot fire the wrong action.
pub fn key_at(x: u16, y: u16) -> Option<u8> {
    let grid_width = COLUMNS as u16 * KEY_SIZE;

    if y >= SCREEN_HEIGHT || x < KEY_MARGIN_X || x >= KEY_MARGIN_X + grid_width {
        return None;
    }

    let column = (x - KEY_MARGIN_X) / KEY_SIZE;
    let row = y / KEY_SIZE;

    Some(row as u8 * COLUMNS + column as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_map_to_first_and_last_key() {
        assert_eq!(key_at(KEY_MARGIN_X, 0), Some(0));
        assert_eq!(
            key_at(KEY_MARGIN_X + COLUMNS as u16 * KEY_SIZE - 1, SCREEN_HEIGHT - 1),
            Some(COLUMNS * ROWS - 1)
        );
    }

    #[test]
    fn keys_are_numbered_row_by_row() {
        assert_eq!(key_at(KEY_MARGIN_X + 89, 89), Some(0));
        assert_eq!(key_at(KEY_MARGIN_X + 90, 0), Some(1));
        assert_eq!(key_at(KEY_MARGIN_X, 90), Some(5));
        assert_eq!(key_at(KEY_MARGIN_X + 90, 90), Some(6));
    }

    #[test]
    fn the_side_margins_are_not_keys() {
        assert_eq!(key_at(0, 0), None);
        assert_eq!(key_at(KEY_MARGIN_X - 1, 100), None);
        assert_eq!(key_at(KEY_MARGIN_X + COLUMNS as u16 * KEY_SIZE, 100), None);
        assert_eq!(key_at(SCREEN_WIDTH - 1, 269), None);
    }

    #[test]
    fn below_the_keys_is_none() {
        assert_eq!(key_at(KEY_MARGIN_X, SCREEN_HEIGHT), None);
    }
}
