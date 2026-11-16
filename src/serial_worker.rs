//! Owns the serial port.
//!
//! One thread owns the port and does all reading and writing, so no two
//! threads ever touch it at once. The main thread talks to it through two
//! channels: `tx` for incoming events, `cmd_rx` for outgoing commands.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use serialport::SerialPort;

use crate::messages::{Cmd, SerialEvent};
use crate::protocol::{self, MAGIC_BYTE};

const BAUD_RATE: u32 = 256_000;

/// How long a single read waits for bytes before giving up. A short timeout
/// keeps the loop responsive so pending commands get sent promptly.
const READ_TIMEOUT: Duration = Duration::from_millis(50);

/// Refuse to buffer more than this while hunting for a frame start.
const MAX_FRAME_BUFFER: usize = 8192;

pub fn run(
    port_name: String,
    tx: mpsc::Sender<SerialEvent>,
    running: Arc<AtomicBool>,
    mut cmd_rx: mpsc::Receiver<Cmd>,
) {
    let mut port: Box<dyn SerialPort> = match serialport::new(&port_name, BAUD_RATE)
        .timeout(READ_TIMEOUT)
        .open()
    {
        Ok(port) => port,
        Err(e) => {
            let _ = tx.send(SerialEvent::Error(format!("open failed: {e}")));
            return;
        }
    };

    let _ = tx.send(SerialEvent::DeviceOpened(port_name));

    // Send the fake WebSocket upgrade. The device answers with an HTTP
    // response, but we do not wait for it: input frames are parsed the same
    // way either way, and `take_frame` skips any non-frame preamble bytes.
    if let Err(e) = port.write_all(protocol::HANDSHAKE_REQUEST) {
        let _ = tx.send(SerialEvent::Error(format!("handshake write failed: {e}")));
    }

    let mut frame_buf: Vec<u8> = Vec::new();
    let mut read_buf = [0u8; 1024];
    let mut transaction_id: u8 = 0;

    while running.load(Ordering::SeqCst) {
        match port.read(&mut read_buf) {
            Ok(0) => {}

            Ok(n) => {
                frame_buf.extend_from_slice(&read_buf[..n]);

                while let Some(frame) = take_frame(&mut frame_buf) {
                    let _ = tx.send(SerialEvent::Frame(frame));
                }
            }

            // Nothing arrived within READ_TIMEOUT. Expected most of the time.
            Err(e) if e.kind() == ErrorKind::TimedOut => {}

            Err(e) => {
                let _ = tx.send(SerialEvent::Error(format!("read failed: {e}")));
                break;
            }
        }

        // Send whatever the main thread asked for while the port is idle.
        if !drain_commands(&mut cmd_rx, &mut port, &mut transaction_id, &tx) {
            break;
        }
    }
}

/// Send every pending command. Returns false on a write error.
fn drain_commands(
    cmd_rx: &mut mpsc::Receiver<Cmd>,
    port: &mut Box<dyn SerialPort>,
    transaction_id: &mut u8,
    tx: &mpsc::Sender<SerialEvent>,
) -> bool {
    while let Ok(cmd) = cmd_rx.try_recv() {
        let packet = match cmd {
            Cmd::SetBrightness(level) => protocol::brightness_packet(*transaction_id, level),
            Cmd::Vibrate(pattern) => protocol::vibration_packet(*transaction_id, pattern),
        };

        // One write for the whole packet: outer wrapper + inner message.
        if let Err(e) = port.write_all(&packet) {
            let _ = tx.send(SerialEvent::Error(format!("write failed: {e}")));
            return false;
        }

        *transaction_id = protocol::next_transaction_id(*transaction_id);
    }

    true
}

/// Pull one complete frame out of `buf`, if there is one.
///
/// Frame layout: `[0x82][length][payload; length bytes]`.
/// Anything before the `0x82` (for example the handshake's HTTP response) is
/// discarded.
fn take_frame(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let Some(start) = buf.iter().position(|&b| b == MAGIC_BYTE) else {
        // No frame start in sight. Do not let the buffer grow forever.
        if buf.len() > MAX_FRAME_BUFFER {
            buf.clear();
        }
        return None;
    };

    buf.drain(..start);

    if buf.len() < 2 {
        return None;
    }

    let length = buf[1] as usize;
    let frame_size = 2 + length;

    if buf.len() < frame_size {
        return None;
    }

    let frame: Vec<u8> = buf.drain(..frame_size).collect();
    Some(frame[2..].to_vec()) // drop the 0x82 + length wrapper
}
