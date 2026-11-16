use std::collections::HashSet;

use crate::messages::DeviceEvent;

// Command bytes the device sends to us.
const BUTTON_PRESS: u8 = 0x00;
const KNOB_ROTATE: u8 = 0x01;
const TOUCH: u8 = 0x4d;
const TOUCH_END: u8 = 0x6d;

/// Turns packets from the device into `DeviceEvent`s.
///
/// Keeps a little state: the device reports touch start and touch move with
/// the same command byte, so we track which touch ids are already down to
/// tell them apart.
pub struct Parser {
    active_touches: HashSet<u8>,
}

impl Parser {
    pub fn new() -> Self {
        Self {
            active_touches: HashSet::new(),
        }
    }

    /// Parse one packet. Returns `None` for acknowledgements and anything
    /// else we do not care about.
    ///
    /// Packet layout (outer `0x82` wrapper already removed):
    /// `[length][command][transaction id][data...]`
    pub fn parse(&mut self, packet: &[u8]) -> Option<DeviceEvent> {
        // Sanity check: the length byte must match the packet we got.
        if packet.len() < 4 || packet[0] as usize != packet.len() {
            return None;
        }

        let command = packet[1];
        let data = &packet[3..];

        match command {
            BUTTON_PRESS => {
                let id = *data.first()?;
                match data.get(1)? {
                    0x00 => Some(DeviceEvent::ButtonDown { id }),
                    _ => Some(DeviceEvent::ButtonUp { id }),
                }
            }

            KNOB_ROTATE => {
                let id = *data.first()?;
                let delta = *data.get(1)? as i8;
                Some(DeviceEvent::KnobRotate { id, delta })
            }

            TOUCH | TOUCH_END => {
                // data = [unknown, x_hi, x_lo, y_hi, y_lo, touch id]
                if data.len() < 6 {
                    return None;
                }
                let x = u16::from_be_bytes([data[1], data[2]]);
                let y = u16::from_be_bytes([data[3], data[4]]);
                let id = data[5];

                if command == TOUCH_END {
                    self.active_touches.remove(&id);
                    Some(DeviceEvent::TouchEnd { id, x, y })
                } else if self.active_touches.insert(id) {
                    Some(DeviceEvent::TouchStart { id, x, y })
                } else {
                    Some(DeviceEvent::TouchMove { id, x, y })
                }
            }

            // Brightness/vibration acknowledgements, ticks, version replies...
            _ => None,
        }
    }
}
