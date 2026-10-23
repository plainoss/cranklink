//! Operating-system boundaries shared by the CLI and the WebAssembly worker.
//!
//! The SM-PCE02 serial route uses 2,000,000 baud, 8N1, no flow control, and
//! asserted RTS. RTS does not imply CTS handshaking, and this host-side rate
//! does not specify the external E-TUBE wire rate. Other PC interfaces need
//! their own transport contract.
use anyhow::Result;
use std::io::{self, Read, Write};
use std::time::Duration;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
#[path = "discovery.rs"]
mod discovery;
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
#[path = "native_usb.rs"]
mod native_usb;

#[cfg(all(unix, not(target_arch = "wasm32"), not(target_os = "android")))]
#[path = "simulator_port.rs"]
mod simulator_port;

/// The synchronous protocol engine owns one transport. Browser calls block only
/// inside its dedicated worker; the main browser thread services USB requests.
pub trait Port: Read + Write {
    /// Bound individual I/O calls. The session applies a separate elapsed-time
    /// deadline while assembling and matching protocol replies across reads.
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()>;
}

/// Human-readable progress that keeps structured command stdout clean.
pub(crate) fn notice(message: &str) {
    #[cfg(target_arch = "wasm32")]
    crate::wasm::host_emit("notice", &format!("{message}\n"));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{message}");
}

#[cfg(all(not(target_arch = "wasm32"), not(test)))]
pub use std::thread::sleep;
#[cfg(not(target_arch = "wasm32"))]
pub use std::time::Instant;
/// Protocol pacing and settle delays are real-time waits on hardware. Unit
/// tests run against scripted ports that answer immediately, so they skip them.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub fn sleep(_: std::time::Duration) {}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
struct NativePort(Box<dyn serialport::SerialPort>);
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
impl Read for NativePort {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
impl Write for NativePort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
impl Port for NativePort {
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.0.set_timeout(timeout).map_err(Into::into)
    }
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn open_port(name: &str) -> Result<Box<dyn Port>> {
    if let Some(path) = name.strip_prefix("sim:") {
        #[cfg(unix)]
        return Ok(Box::new(simulator_port::SimulatorPort::open(path)?));
        #[cfg(not(unix))]
        anyhow::bail!("firmware simulator sockets currently require Unix: {path}");
    }
    let selected;
    let name = if name == "auto" {
        let serial = serialport::available_ports()
            .map(matching_serial)
            .map_err(|e| e.to_string());
        let usb = native_usb::devices();
        let metadata = usb
            .as_ref()
            .map(|devices| {
                devices
                    .iter()
                    .map(|d| discovery::UsbCandidate {
                        identity: format!("{:?}", d.id()),
                        serial: d.serial_number().map(str::to_owned),
                    })
                    .collect()
            })
            .map_err(|e| e.to_string());
        match discovery::select(serial, metadata, cfg!(target_os = "windows"))? {
            discovery::Choice::Usb(index) => return native_usb::open_device(usb?.remove(index)),
            discovery::Choice::Serial(port) => {
                selected = port;
                &selected
            }
        }
    } else {
        name
    };
    if name == "usb" {
        return native_usb::open();
    }
    use anyhow::Context;
    use serialport::{DataBits, FlowControl, Parity, StopBits};
    let mut port = serialport::new(name, 2_000_000)
        .data_bits(DataBits::Eight)
        .parity(Parity::None)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .timeout(Duration::from_millis(100))
        .open()
        .context("open SM-PCE02 serial port (close E-TUBE first)")?;
    port.write_request_to_send(true).context("assert RTS")?;
    Ok(Box::new(NativePort(port)))
}

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
fn matching_serial(ports: Vec<serialport::SerialPortInfo>) -> Vec<discovery::SerialCandidate> {
    ports
        .into_iter()
        .filter_map(|port| match port.port_type {
            serialport::SerialPortType::UsbPort(info)
                if info.vid == 0x1e44 && info.pid == 0x7151 =>
            {
                Some(discovery::SerialCandidate {
                    port: port.port_name,
                    serial: info.serial_number,
                })
            }
            _ => None,
        })
        .collect()
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug)]
pub struct Instant(f64);
#[cfg(target_arch = "wasm32")]
impl Instant {
    pub fn now() -> Self {
        Self(crate::wasm::host_now_ms())
    }
    pub fn elapsed(&self) -> Duration {
        let elapsed = (crate::wasm::host_now_ms() - self.0).max(0.0);
        Duration::from_secs_f64(elapsed / 1000.0)
    }
}
#[cfg(target_arch = "wasm32")]
pub fn now_ms() -> u64 {
    crate::wasm::host_now_ms().max(0.0) as u64
}
#[cfg(target_arch = "wasm32")]
pub fn sleep(duration: Duration) {
    // Keep large waits bounded by the host RPC representation.
    let mut remaining = duration.as_millis();
    while remaining > 0 {
        let step = remaining.min(u32::MAX as u128) as u32;
        crate::wasm::host_sleep(step);
        remaining -= u128::from(step);
    }
}

#[cfg(target_arch = "wasm32")]
struct WebPort {
    timeout_ms: u32,
}
#[cfg(target_arch = "wasm32")]
impl Read for WebPort {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let limit = u32::try_from(bytes.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "read buffer too large"))?;
        let data = crate::wasm::rpc("io.read", &limit.to_le_bytes(), self.timeout_ms)?;
        if data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Browser transport read timeout",
            ));
        }
        if data.len() > bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Browser transport read exceeded requested buffer",
            ));
        }
        bytes[..data.len()].copy_from_slice(&data);
        Ok(data.len())
    }
}
#[cfg(target_arch = "wasm32")]
impl Write for WebPort {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        crate::wasm::rpc("io.write", bytes, self.timeout_ms)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(target_arch = "wasm32")]
impl Port for WebPort {
    fn set_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.timeout_ms = timeout.as_millis().min(u128::from(u32::MAX)) as u32;
        Ok(())
    }
}
#[cfg(target_arch = "wasm32")]
impl Drop for WebPort {
    fn drop(&mut self) {
        let _ = crate::wasm::rpc("io.close", &[], 1000);
    }
}
#[cfg(target_arch = "wasm32")]
pub fn open_port(name: &str) -> Result<Box<dyn Port>> {
    anyhow::ensure!(
        matches!(name, "auto" | "webusb" | "webserial"),
        "browser transport requires auto, webusb, or webserial"
    );
    let config = serde_json::to_vec(&serde_json::json!({
        "port": name, "baudRate": 2_000_000, "dataBits": 8,
        "stopBits": 1, "parity": "none", "flowControl": "none", "rts": true
    }))?;
    crate::wasm::rpc("io.open", &config, 5000)?;
    Ok(Box::new(WebPort { timeout_ms: 100 }))
}

#[derive(Debug)]
pub struct PortInfo {
    pub port_name: String,
    pub port_type: String,
}
#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn available_ports() -> Result<Vec<PortInfo>> {
    let mut ports: Vec<_> = serialport::available_ports()?
        .into_iter()
        .map(|p| PortInfo {
            port_name: p.port_name,
            port_type: format!("{:?}", p.port_type),
        })
        .collect();
    // A missing USB stack (containers, sandboxes) must not hide serial ports.
    match native_usb::devices() {
        Ok(devices) if devices.is_empty() => {}
        Ok(devices) => ports.push(PortInfo {
            port_name: "usb".into(),
            port_type: format!(
                "Direct SM-PCE02 FTDI USB ({} device(s); no COM driver required)",
                devices.len()
            ),
        }),
        // Not a port: report it without listing a "usb" adapter that does not exist.
        Err(error) => notice(&format!("Direct USB enumeration unavailable: {error:#}")),
    }
    Ok(ports)
}
#[cfg(target_arch = "wasm32")]
pub fn available_ports() -> Result<Vec<PortInfo>> {
    Ok(vec![
        PortInfo {
            port_name: "webusb".into(),
            port_type: "SM-PCE02 selected with connect usb".into(),
        },
        PortInfo {
            port_name: "webserial".into(),
            port_type: "SM-PCE02 selected with connect serial".into(),
        },
    ])
}

#[cfg(all(test, not(target_arch = "wasm32"), not(target_os = "android")))]
mod discovery_filter_tests {
    use super::*;
    #[test]
    fn automatic_serial_candidates_require_both_exact_usb_ids() {
        use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};
        let usb = |port: &str, vid, pid| SerialPortInfo {
            port_name: port.into(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: None,
                manufacturer: Some("SHIMANO".into()),
                product: Some("SM-PCE02".into()),
            }),
        };
        let candidates = matching_serial(vec![
            usb("COM3", 0x1e44, 0x7151),
            usb("COM4", 0x0403, 0x7151),
            usb("COM5", 0x1e44, 0x6001),
            SerialPortInfo {
                port_name: "SM-PCE02".into(),
                port_type: SerialPortType::Unknown,
            },
            SerialPortInfo {
                port_name: "COM6".into(),
                port_type: SerialPortType::BluetoothPort,
            },
        ]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].port, "COM3");
    }
}

/// Android owns Bluetooth through its platform GATT implementation. Wired
/// transports are unavailable in the Android app.
#[cfg(target_os = "android")]
pub fn open_port(_name: &str) -> Result<Box<dyn Port>> {
    anyhow::bail!("the Android app supports BLE only")
}

#[cfg(target_os = "android")]
pub fn available_ports() -> Result<Vec<PortInfo>> {
    Ok(Vec::new())
}
