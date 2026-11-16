use serialport::{available_ports, SerialPortType};

// Find the first Loupedeck/Razer serial port.
// Returns its name (e.g. "COM4") or None.
pub fn find_loupedeck_port() -> Option<String> {
    let ports = available_ports().ok()?;

    for port in ports {
        if let SerialPortType::UsbPort(usb) = &port.port_type {
            match (usb.vid, usb.pid) {
                (0x2EC2, _) => return Some(port.port_name),
                (0x1532, _) => return Some(port.port_name),
                _ => {}
            }
        }
    }

    None
}
