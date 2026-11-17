//! Owns the serial port: discovery, connection, reading and writing.
//!
//! One thread owns the port, so no two threads ever touch it at once. The
//! main thread talks to it through two channels: `tx` for events, `cmd_rx`
//! for commands.
//!
//! That thread also does the reconnecting. It scans until a device shows up,
//! keeps scanning after an unplug, and retries an open that fails because the
//! previous process has not let go of the port yet. The rest of the program
//! never blocks on the device being present.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serialport::{ClearBuffer, SerialPort};

use crate::discovery;
use crate::messages::{Cmd, SerialEvent};
use crate::protocol::{self, MAGIC_BYTE};

const BAUD_RATE: u32 = 256_000;

/// How long a single read waits for bytes before giving up. A short timeout
/// keeps the loop responsive so pending commands get sent promptly.
const READ_TIMEOUT: Duration = Duration::from_millis(50);

/// Refuse to buffer more than this while hunting for a frame start.
const MAX_FRAME_BUFFER: usize = 8192;

/// How often to look for a device that is not plugged in.
const SCAN_INTERVAL: Duration = Duration::from_millis(700);

/// Pause after a disconnect so Windows can finish re-enumerating the port.
const RECONNECT_DELAY: Duration = Duration::from_millis(1000);

/// Pause after a failed open. Usually means another process still holds the
/// port, which fixes itself once that process dies.
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(1000);

/// How long to wait for the device to answer the upgrade request. A device
/// that does not answer is a device that will ignore our commands, so it is
/// better to close and retry than to sit there looking connected.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_millis(2000);

/// Time given to the close frame before the handle is dropped.
const CLOSE_GRACE: Duration = Duration::from_millis(100);

/// Why a connected session ended.
enum SessionEnd {
    /// Asked to stop (Ctrl+C).
    Shutdown,
    /// The link broke; worth telling the user and retrying.
    Lost(String),
}

pub fn run(
    tx: mpsc::Sender<SerialEvent>,
    running: Arc<AtomicBool>,
    mut cmd_rx: mpsc::Receiver<Cmd>,
) {
    let mut transaction_id: u8 = 0;
    let mut told_them_we_are_waiting = false;

    while running.load(Ordering::SeqCst) {
        let port_name = match discovery::find_loupedeck_port() {
            Some(name) => name,
            None => {
                // Nobody to send commands to; drop them so they do not pile up
                // and fire all at once when the device appears.
                discard_commands(&mut cmd_rx);

                if !told_them_we_are_waiting {
                    let _ = tx.send(SerialEvent::Waiting);
                    told_them_we_are_waiting = true;
                }

                sleep_while_running(&running, SCAN_INTERVAL);
                continue;
            }
        };

        let mut port = match open(&port_name) {
            Ok(port) => port,
            Err(error) => {
                let _ = tx.send(SerialEvent::Error(format!(
                    "could not open {port_name}: {error}"
                )));
                // The device is busy, not gone. Anything the user does in the
                // meantime would fire late, so drop it; the main thread
                // re-applies brightness when the connection succeeds.
                discard_commands(&mut cmd_rx);
                sleep_while_running(&running, OPEN_RETRY_DELAY);
                continue;
            }
        };

        told_them_we_are_waiting = false;

        let end = session(
            &port_name,
            &mut port,
            &tx,
            &running,
            &mut cmd_rx,
            &mut transaction_id,
        );

        // Tell the device the session is over before releasing the handle.
        // Skipping this leaves it believing a client is still attached, which
        // is what forces an unplug/replug before the next run works.
        let _ = port.write_all(protocol::CLOSE_FRAME);
        let _ = port.flush();

        if matches!(end, SessionEnd::Shutdown) {
            // Give the close frame time to leave the wire.
            std::thread::sleep(CLOSE_GRACE);
        }

        // Drop the handle explicitly, before scanning again. This is what lets
        // the next run open the port without unplugging anything.
        drop(port);

        match end {
            SessionEnd::Shutdown => {}
            SessionEnd::Lost(reason) => {
                let _ = tx.send(SerialEvent::Disconnected(reason));
                discard_commands(&mut cmd_rx);
                sleep_while_running(&running, RECONNECT_DELAY);
            }
        }
    }
}

/// Open the port with the settings the device expects.
fn open(port_name: &str) -> Result<Box<dyn SerialPort>, serialport::Error> {
    serialport::new(port_name, BAUD_RATE).timeout(READ_TIMEOUT).open()
}

/// One connection, from handshake to disconnect.
fn session(
    port_name: &str,
    port: &mut Box<dyn SerialPort>,
    tx: &mpsc::Sender<SerialEvent>,
    running: &AtomicBool,
    cmd_rx: &mut mpsc::Receiver<Cmd>,
    transaction_id: &mut u8,
) -> SessionEnd {
    // Anything the driver buffered while we were away belongs to the previous
    // connection.
    let _ = port.clear(ClearBuffer::All);

    // If the last run died without closing cleanly, the device still believes
    // a client is attached and ignores a fresh upgrade until it is power
    // cycled. A close frame clears that state, so a reconnect works without
    // unplugging anything.
    let _ = port.write_all(protocol::CLOSE_FRAME);
    let _ = port.flush();

    // The fake WebSocket upgrade. Waiting for the reply matters: until the
    // device answers, it is not listening to commands.
    let handshake = port
        .write_all(protocol::HANDSHAKE_REQUEST)
        .and_then(|()| port.flush());

    if let Err(error) = handshake {
        return SessionEnd::Lost(format!("handshake write failed: {error}"));
    }

    if let Err(error) = await_handshake(port) {
        return SessionEnd::Lost(error);
    }

    // Only now is the device actually listening to us.
    let _ = tx.send(SerialEvent::Connected(port_name.to_owned()));

    let mut frame_buf: Vec<u8> = Vec::new();
    let mut read_buf = [0u8; 1024];

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
            Err(error) if error.kind() == ErrorKind::TimedOut => {}

            // Unplugged, reset, or the driver gave up. Either way the handle
            // is no longer useful.
            Err(error) => return SessionEnd::Lost(format!("read failed: {error}")),
        }

        // Send whatever the main thread asked for while the port is idle.
        if let Err(error) = drain_commands(cmd_rx, port, transaction_id) {
            return SessionEnd::Lost(error);
        }
    }

    // Closing down on request.
    SessionEnd::Shutdown
}

/// Wait for the device's upgrade reply.
///
/// Accepts either the `HTTP/1.1` line or a frame magic byte, because a device
/// that is already streaming is just as usable as one that replied politely.
fn await_handshake(port: &mut Box<dyn SerialPort>) -> Result<(), String> {
    let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
    let mut seen: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1024];

    while Instant::now() < deadline {
        match port.read(&mut chunk) {
            Ok(0) => {}

            Ok(n) => {
                seen.extend_from_slice(&chunk[..n]);

                if contains(&seen, protocol::HANDSHAKE_REPLY) || seen.contains(&MAGIC_BYTE) {
                    return Ok(());
                }

                if seen.len() > MAX_FRAME_BUFFER {
                    return Err("unexpected data before the handshake reply".to_owned());
                }
            }

            Err(error) if error.kind() == ErrorKind::TimedOut => {}
            Err(error) => return Err(format!("handshake read failed: {error}")),
        }
    }

    Err("device did not answer the handshake".to_owned())
}

/// Is `needle` somewhere in `haystack`?
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Send every pending command. Returns an error message on a write failure.
fn drain_commands(
    cmd_rx: &mut mpsc::Receiver<Cmd>,
    port: &mut Box<dyn SerialPort>,
    transaction_id: &mut u8,
) -> Result<(), String> {
    while let Ok(cmd) = cmd_rx.try_recv() {
        let packet = match cmd {
            Cmd::SetBrightness(level) => protocol::brightness_packet(*transaction_id, level),
            Cmd::Vibrate(pattern) => protocol::vibration_packet(*transaction_id, pattern),
        };

        // One write for the whole packet: outer wrapper + inner message.
        port.write_all(&packet)
            .map_err(|error| format!("write failed: {error}"))?;

        *transaction_id = protocol::next_transaction_id(*transaction_id);
    }

    Ok(())
}

/// Throw away commands that arrived while there was no device to run them on.
fn discard_commands(cmd_rx: &mut mpsc::Receiver<Cmd>) {
    while cmd_rx.try_recv().is_ok() {}
}

/// Sleep, but wake up early when the user asks us to stop.
fn sleep_while_running(running: &AtomicBool, total: Duration) {
    let step = Duration::from_millis(50);
    let deadline = Instant::now() + total;

    while running.load(Ordering::SeqCst) && Instant::now() < deadline {
        std::thread::sleep(step);
    }
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
