/// Messages sent from the serial worker thread to the main thread.
#[derive(Debug, Clone)]
pub enum SerialEvent {
    /// No device yet. The worker keeps scanning in the background.
    Waiting,
    /// A port opened, and the handshake has been sent.
    Connected(String),
    /// The port went away: cable pulled, device reset, or a read/write error.
    /// The worker closes the port and starts scanning again.
    Disconnected(String),
    /// One complete packet, with the `0x82` wrapper already removed.
    Frame(Vec<u8>),
    /// Something went wrong that is worth printing but is not fatal.
    Error(String),
}

/// Something the user did on the device.
#[derive(Debug, Clone)]
pub enum DeviceEvent {
    TouchStart {
        id: u8,
        x: u16,
        y: u16,
    },
    TouchMove {
        id: u8,
        x: u16,
        y: u16,
    },
    TouchEnd {
        id: u8,
        x: u16,
        y: u16,
    },
    ButtonDown {
        id: u8,
    },
    ButtonUp {
        id: u8,
    },
    /// `delta` is positive clockwise, negative counter-clockwise.
    KnobRotate {
        id: u8,
        delta: i8,
    },
}

/// Commands the main thread sends to the serial worker thread.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Screen brightness, 0..=10.
    SetBrightness(u8),
    /// Haptic pattern byte, see `protocol::HAPTIC_*`.
    Vibrate(u8),
    /// Write a rectangle of pixels and show it on the panel.
    ///
    /// `pixels` is `width * height * 2` bytes of little-endian RGB565. The
    /// worker sends the frame buffer write followed by the refresh.
    Framebuffer {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        pixels: Vec<u8>,
    },
}

/// Human-readable name for a raw button id.
///
/// On the Live S the two knobs are ids 1 and 2; the four buttons below the
/// screen are ids 7, 8, 9 and 10.
pub fn button_label(id: u8) -> &'static str {
    match id {
        1 => "top knob press",
        2 => "bottom knob press",
        7 => "screen button 1",
        8 => "screen button 2",
        9 => "screen button 3",
        10 => "screen button 4",
        _ => "unknown button",
    }
}

/// Human-readable name for a raw knob id.
pub fn knob_label(id: u8) -> &'static str {
    match id {
        1 => "top knob",
        2 => "bottom knob",
        _ => "unknown knob",
    }
}
