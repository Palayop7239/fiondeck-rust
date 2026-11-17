//! Outgoing wire format for the Loupedeck Live S.
//!
//! Reference: `foxxyz/loupedeck` — `device.js` (`send`) and
//! `connections/serial.js` (`send`).
//!
//! Every command is wrapped in two layers:
//!
//! 1. The inner message (this is what the device actually parses):
//!
//!    ```text
//!    [length][command][transaction id][data...]
//!    ```
//!
//!    `length` counts every byte of the message, including itself.
//!    `transaction id` starts at 1, increments per command and skips 0
//!    (the device ignores 0).
//!
//! 2. The outer serial wrapper (a fake WebSocket frame):
//!
//!    ```text
//!    [0x82][0x80 | message length][0x00 0x00 0x00 0x00][message]
//!    ```
//!
//!    The high bit on the length means "masked". The four zero bytes are
//!    the mask key; the device does not actually apply the mask, so they
//!    are always zero.
//!
//!    Messages longer than 255 bytes use the WebSocket extended form:
//!    `0x82 0xff 00 00 00 00 <length as 4 bytes, big endian> 00 00 00 00`.

/// First byte of every frame travelling in either direction.
pub const MAGIC_BYTE: u8 = 0x82;

/// Command: set screen brightness (one data byte, 0..=10).
pub const SET_BRIGHTNESS: u8 = 0x09;
/// Command: trigger a haptic pattern (one data byte).
pub const SET_VIBRATION: u8 = 0x1b;

/// The fake WebSocket upgrade the device expects right after the port opens.
/// Line endings are LF, not CRLF.
pub const HANDSHAKE_REQUEST: &[u8] =
    b"GET /index.html\nHTTP/1.1\nConnection: Upgrade\nUpgrade: websocket\nSec-WebSocket-Key: 123abc\n\n";

/// The first line of the device's reply to the upgrade.
///
/// Worth waiting for: it is the only confirmation that the device accepted
/// this connection, and it arrives before any input frames do.
pub const HANDSHAKE_REPLY: &[u8] = b"HTTP/1.1";

/// A WebSocket close frame, as sent by the reference client on shutdown
/// (`connections/serial.js`, `WS_CLOSE_FRAME`).
///
/// Skipping this is why the device can stay wedged after an unclean exit: it
/// still believes a client is attached, so it ignores the next handshake
/// until it is power cycled.
pub const CLOSE_FRAME: &[u8] = &[0x88, 0x80, 0x00, 0x00, 0x00, 0x00];

/// Wrap `command` + `data` into one complete packet ready to write to the port.
pub fn build_packet(command: u8, transaction_id: u8, data: &[u8]) -> Vec<u8> {
    // 1. Inner message.
    let mut message = Vec::with_capacity(3 + data.len());
    message.push((3 + data.len()).min(0xff) as u8); // length, including this byte
    message.push(command);
    message.push(transaction_id);
    message.extend_from_slice(data);

    // 2. Outer wrapper.
    let mut packet = Vec::with_capacity(14 + message.len());
    packet.push(MAGIC_BYTE);

    if message.len() > 0xff {
        packet.push(0xff);
        packet.extend_from_slice(&[0, 0, 0, 0]); // mask key
        packet.extend_from_slice(&(message.len() as u32).to_be_bytes());
        packet.extend_from_slice(&[0, 0, 0, 0]); // padding, header is 14 bytes
    } else {
        packet.push(0x80 | message.len() as u8); // mask bit + length
        packet.extend_from_slice(&[0, 0, 0, 0]); // mask key
    }

    packet.extend_from_slice(&message);
    packet
}

/// Brightness packet. `level` is clamped to the firmware maximum of 10.
pub fn brightness_packet(transaction_id: u8, level: u8) -> Vec<u8> {
    build_packet(SET_BRIGHTNESS, transaction_id, &[level.min(10)])
}

/// Vibration packet. `pattern` is one of the `HAPTIC_*` constants.
pub fn vibration_packet(transaction_id: u8, pattern: u8) -> Vec<u8> {
    build_packet(SET_VIBRATION, transaction_id, &[pattern])
}

/// Next transaction id: 1, 2, 3, ... 255, 1, 2, ...
pub fn next_transaction_id(current: u8) -> u8 {
    let next = current.wrapping_add(1);
    if next == 0 {
        1
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These expected bytes come straight from the reference library's tests
    // (`test/device-live-s.test.js`), plus the serial wrapper from
    // `connections/serial.js`.

    #[test]
    fn brightness_packet_matches_reference() {
        // device.js sends inner message 04 09 01 00 for brightness 0.
        assert_eq!(
            brightness_packet(1, 0),
            vec![0x82, 0x84, 0, 0, 0, 0, 0x04, 0x09, 0x01, 0x00]
        );
    }

    #[test]
    fn brightness_is_clamped_to_ten() {
        // device.js maps 1.0 to 0x0a and never higher.
        assert_eq!(
            brightness_packet(2, 99),
            vec![0x82, 0x84, 0, 0, 0, 0, 0x04, 0x09, 0x02, 0x0a]
        );
    }

    #[test]
    fn vibration_packet_matches_reference() {
        // device.js sends inner message 04 1b 01 56 for pattern 0x56.
        assert_eq!(
            vibration_packet(1, 0x56),
            vec![0x82, 0x84, 0, 0, 0, 0, 0x04, 0x1b, 0x01, 0x56]
        );
    }

    #[test]
    fn transaction_ids_skip_zero() {
        assert_eq!(next_transaction_id(0), 1);
        assert_eq!(next_transaction_id(1), 2);
        assert_eq!(next_transaction_id(254), 255);
        assert_eq!(next_transaction_id(255), 1);
    }
}
