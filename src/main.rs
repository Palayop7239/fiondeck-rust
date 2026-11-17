mod config;
mod discovery;
mod gui;
mod messages;
mod parser;
mod protocol;
mod serial_worker;
mod surface;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crate::config::{Action, Config, Input};
use crate::messages::{button_label, knob_label, Cmd, DeviceEvent, SerialEvent};
use crate::parser::Parser;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `--gui` opens the iced window instead of running the device app.
    if std::env::args().any(|arg| arg == "--gui") {
        gui::run()?;
        return Ok(());
    }

    let config = config::load_or_create(Path::new(config::DEFAULT_PATH))?;
    println!(
        "Loaded {} bindings from {}",
        config.bindings.len(),
        config::DEFAULT_PATH
    );

    let running = Arc::new(AtomicBool::new(true));
    let running_for_ctrlc = running.clone();
    ctrlc::set_handler(move || {
        running_for_ctrlc.store(false, Ordering::SeqCst);
    })?;

    // Worker -> main: device events and errors.
    let (event_tx, event_rx) = mpsc::channel::<SerialEvent>();
    // Main -> worker: commands to send to the device.
    let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();

    // The worker finds the device and reconnects on its own, so startup never
    // waits for hardware to be present.
    let running_for_worker = running.clone();
    let worker = std::thread::spawn(move || {
        serial_worker::run(event_tx, running_for_worker, cmd_rx);
    });

    let mut brightness: i16 = config.brightness.min(10) as i16;
    println!("Looking for a Loupedeck. Press Ctrl+C to quit.");

    let mut parser = Parser::new();

    while running.load(Ordering::SeqCst) {
        let event = match event_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => event,
            // Idle tick; just check `running` again.
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            // Worker is gone.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        match event {
            SerialEvent::Waiting => println!("No device yet; still scanning."),

            SerialEvent::Connected(name) => {
                println!("Connected to {name}");
                // Commands sent before the port existed were dropped, so put
                // the screen back to the value the config asked for.
                set_brightness(brightness, &cmd_tx);
            }

            SerialEvent::Disconnected(reason) => {
                println!("Disconnected: {reason}");
                println!("Scanning again; plug it back in whenever you like.");
            }

            SerialEvent::Frame(frame) => {
                let Some(device_event) = parser.parse(&frame) else {
                    // Acknowledgements, ticks, unsupported messages.
                    continue;
                };

                handle_event(&device_event, &config, &cmd_tx, &mut brightness);
            }

            SerialEvent::Error(message) => println!("Serial error: {message}"),
        }
    }

    // Let the worker close and drop the port before the process exits, so the
    // next run can open it again without unplugging anything.
    let _ = worker.join();

    println!("Shutting down.");
    Ok(())
}

/// Print the event and run whatever action the config binds to it.
fn handle_event(
    event: &DeviceEvent,
    config: &Config,
    cmd_tx: &mpsc::Sender<Cmd>,
    brightness: &mut i16,
) {
    match event {
        DeviceEvent::ButtonDown { id } => {
            println!("{} pressed", button_label(*id));
            match config.action(Input::Press, *id) {
                Some(action) => run_action(action, 1, cmd_tx, brightness),
                None => println!("  (no binding for press id {id})"),
            }
        }

        DeviceEvent::ButtonUp { id } => {
            println!("{} released", button_label(*id));
        }

        DeviceEvent::KnobRotate { id, delta } => {
            let direction = if *delta > 0 { "right" } else { "left" };
            println!("{} turned {direction}", knob_label(*id));

            match config.action(Input::Rotate, *id) {
                Some(action) => run_action(action, *delta, cmd_tx, brightness),
                None => println!("  (no binding for rotate id {id})"),
            }
        }

        DeviceEvent::TouchStart { id, x, y } => {
            println!("touch start id={id} x={x} y={y}");

            // A touch is a position, not an id: work out which key was hit.
            let Some(key) = surface::key_at(*x, *y) else {
                println!("  (outside the screen)");
                return;
            };

            println!("  key {key}");
            match config.action(Input::Touch, key) {
                Some(action) => run_action(action, 1, cmd_tx, brightness),
                None => println!("  (no binding for touch key {key})"),
            }
        }

        DeviceEvent::TouchMove { id, x, y } => {
            println!("touch move id={id} x={x} y={y}");
        }

        DeviceEvent::TouchEnd { id, x, y } => {
            println!("touch end id={id} x={x} y={y}");
        }
    }
}

/// Turn one configured action into a command for the device.
///
/// `direction` is +1 for a press and the knob delta for a turn, so a
/// brightness step follows the direction the knob was actually turned.
fn run_action(action: &Action, direction: i8, cmd_tx: &mpsc::Sender<Cmd>, brightness: &mut i16) {
    match action {
        Action::Vibrate { pattern } => {
            let byte = pattern.byte();
            let _ = cmd_tx.send(Cmd::Vibrate(byte));
            println!("  -> vibrate {pattern:?} (0x{byte:02x})");
        }

        Action::BrightnessStep { step } => {
            *brightness = (*brightness + *step as i16 * direction as i16).clamp(0, 10);
            set_brightness(*brightness, cmd_tx);
        }

        Action::SetBrightness { level } => {
            *brightness = (*level).min(10) as i16;
            set_brightness(*brightness, cmd_tx);
        }

        Action::Disabled => println!("  (binding disabled)"),
    }
}

fn set_brightness(level: i16, cmd_tx: &mpsc::Sender<Cmd>) {
    let _ = cmd_tx.send(Cmd::SetBrightness(level as u8));
    println!("  -> brightness {level}/10");
}
