//! Stock-firmware validation of the production DCASX sender and prefetch policy.
use super::*;
use std::{
    cell::RefCell,
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    rc::Rc,
};

struct Peer {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}
impl Peer {
    fn request(&mut self, value: serde_json::Value) -> Result<serde_json::Value> {
        serde_json::to_writer(&mut self.input, &value)?;
        self.input.write_all(b"\n")?;
        self.input.flush()?;
        let mut line = String::new();
        ensure!(self.output.read_line(&mut line)? > 0, "offline peer exited");
        let value: serde_json::Value = serde_json::from_str(&line)?;
        ensure!(value["ok"] == true, "offline peer: {}", value["error"]);
        Ok(value)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
struct PipeGatt(Rc<RefCell<Peer>>);
impl Gatt for PipeGatt {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        vec![CharacteristicInfo {
            id: PCE_COMMAND,
            read: false,
            write: true,
            write_without_response: true,
            notify: false,
            indicate: false,
        }]
    }
    fn read(&mut self, _: u16) -> Result<Vec<u8>> {
        bail!("unexpected read")
    }
    fn subscribe(&mut self, _: u16) -> Result<()> {
        bail!("unexpected subscribe")
    }
    fn passkey(&mut self) -> Result<String> {
        bail!("unexpected passkey")
    }
    fn write(&mut self, id: u16, value: &[u8], without_response: bool) -> Result<()> {
        ensure!(id == PCE_COMMAND, "unexpected characteristic");
        self.0.borrow_mut().request(
            serde_json::json!({"op":"write", "bytes":value, "without_response":without_response}),
        )?;
        Ok(())
    }
    fn receive(&mut self, timeout: Duration) -> Result<Option<Notification>> {
        let value = self
            .0
            .borrow_mut()
            .request(serde_json::json!({"op":"receive", "timeout_ms":timeout.as_millis()}))?;
        Ok(serde_json::from_value(value["notification"].clone())?)
    }
    fn write_batch(&mut self, id: u16, batch: &[Vec<u8>], _: bool) -> Result<()> {
        ensure!(id == PCE_COMMAND, "unexpected characteristic");
        self.0
            .borrow_mut()
            .request(serde_json::json!({"op":"write-batch", "batch":batch}))?;
        Ok(())
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Only the baseline's prefetch capability is overridden. All scheduling and
/// stale receipt handling run in production code.
struct Schedule<'a> {
    inner: MotorFirmware<'a>,
    prefetch: bool,
}
impl FirmwareIo for Schedule<'_> {
    fn supports_dcas_upload_prefetch(&self) -> bool {
        self.prefetch && self.inner.supports_dcas_upload_prefetch()
    }
    fn prepare_dcas_upload(&mut self) -> Result<()> {
        self.inner.prepare_dcas_upload()
    }
    fn prepare_dcas_transfer(&mut self) -> Result<()> {
        self.inner.prepare_dcas_transfer()
    }
    fn send(&mut self, control: u8, payload: &[u8]) -> Result<()> {
        self.inner.send(control, payload)
    }
    fn wait(&mut self, timeout: Duration, check: impl Fn(&Frame) -> bool) -> Result<Frame> {
        self.inner.wait(timeout, check)
    }
    fn discard_matching(&mut self, check: impl Fn(&Frame) -> bool) {
        self.inner.discard_matching(check);
    }
    fn exchange(
        &mut self,
        control: u8,
        payload: &[u8],
        timeout: Duration,
        check: impl Fn(&Frame) -> bool,
    ) -> Result<Frame> {
        self.inner.exchange(control, payload, timeout, check)
    }
}

#[test]
#[ignore = "requires CRANKLINK_D_RELAY_CONFIG and CRANKLINK_SIM_PYTHON stock-image fixtures"]
fn stock_d_relay_actual_sender_matrix() {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("CRANKLINK_D_RELAY_CONFIG").unwrap()).unwrap(),
    )
    .unwrap();
    let python = std::env::var("CRANKLINK_SIM_PYTHON").unwrap();
    let mut cases = 0;
    for gateway in config["gateways"].as_array().unwrap() {
        for prefetch in [false, true] {
            for fault in [
                "none",
                "reorder",
                "duplicate",
                "drop-82",
                "drop-83",
                "drop-c0",
                "drop-motor",
                "nak-82",
                "nak-motor",
                "corrupt-fragment",
                "drop-terminal",
                "disconnect-fragment",
                "duplicate-drop-83-at-wrap",
                "duplicate-drop-c0-at-wrap",
                "drop-prefetch-82",
                "nak-prefetch-82",
                "disconnect-prefetch",
                "corrupt-prefetch",
            ] {
                let acknowledgements = if fault == "none" && gateway == &config["gateways"][0] {
                    vec![0, 30, 60]
                } else {
                    vec![0]
                };
                for ack_ms in acknowledgements {
                    let length: usize = if fault == "none" {
                        std::env::var("CRANKLINK_D_RELAY_IMAGE_BYTES")
                            .unwrap_or_else(|_| "156300".into())
                            .parse()
                            .unwrap()
                    } else if fault == "duplicate" || fault.starts_with("duplicate-drop-") {
                        64 * 130 + 13
                    } else {
                        269
                    };
                    let mut process = Command::new(&python)
                        .arg(concat!(
                            env!("CARGO_MANIFEST_DIR"),
                            "/tools/firmware-sim/d_relay_peer.py"
                        ))
                        .arg(config["radio"].as_str().unwrap())
                        .arg(gateway.as_str().unwrap())
                        .arg(fault)
                        .arg(length.to_string())
                        .arg(ack_ms.to_string())
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped())
                        .spawn()
                        .unwrap();
                    let input = process.stdin.take().unwrap();
                    let output = BufReader::new(process.stdout.take().unwrap());
                    let peer = Rc::new(RefCell::new(Peer {
                        process,
                        input,
                        output,
                    }));
                    let mut session =
                        BleSession::new(Box::new(PipeGatt(peer.clone())), None).unwrap();
                    session.firmware_active = true;
                    let mut image: Vec<u8> = (0..length).map(|i| (i % 251) as u8).collect();
                    image[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
                    let mut schedule = Schedule {
                        inner: MotorFirmware {
                            session: &mut session,
                            slot: 13,
                            incoming: VecDeque::new(),
                            rx_mode: false,
                            dcas_mode: true,
                        },
                        prefetch,
                    };
                    let identity = BootIdentity {
                        slot: 13,
                        version: [5, 0, 6, 1],
                        board: 1,
                        series: 0x23,
                        unit: 0,
                        serial: [1, 2, 3, 4, 5, 6],
                    };
                    let result =
                        firmware_io::flash_dcas(&mut schedule, &image, &identity, |_, _| {});
                    let stats = peer
                        .borrow_mut()
                        .request(serde_json::json!({"op":"stats"}))
                        .unwrap();
                    let success = ["none", "reorder", "duplicate"].contains(&fault);
                    assert_eq!(stats["erases"], 1);
                    assert_eq!(
                        result.is_ok(),
                        success,
                        "gateway={gateway} prefetch={prefetch} fault={fault}: {result:?} {stats}"
                    );
                    if success {
                        assert_eq!(stats["transfers"], length.div_ceil(64));
                        assert_eq!(stats["finish"], true);
                        assert_eq!(stats["reset"], true);
                        assert_eq!(stats["early_starts"], 0);
                        if fault == "none" && length == 156300 && !prefetch {
                            assert!(
                                (stats["stream_seconds"].as_f64().unwrap() - 331.022).abs() < 0.001,
                                "sequential timing no longer matches calibration: {stats}"
                            );
                        }
                    } else {
                        assert_eq!(stats["finish"], false);
                        assert_eq!(stats["reset"], false);
                        assert!(
                            stats["transfers"].as_u64().unwrap()
                                <= if fault.starts_with("duplicate-drop-") {
                                    129
                                } else if fault.contains("prefetch") {
                                    2
                                } else {
                                    1
                                }
                        );
                        let uploads = stats["uploads"].as_u64().unwrap();
                        let expected = if fault == "disconnect-prefetch" {
                            1
                        } else if fault.contains("prefetch") {
                            2
                        } else if fault.starts_with("duplicate-drop-") {
                            129
                        } else if ["drop-terminal", "disconnect-fragment"].contains(&fault) {
                            0
                        } else {
                            1
                        };
                        assert!(
                            uploads >= expected && uploads <= expected + u64::from(prefetch),
                            "unexpected extra uploads: {stats}"
                        );
                        assert!(
                            schedule.send(0x88, &[0x27, 0, 0, 0]).is_err(),
                            "failed session was not fenced"
                        );
                    }
                    println!("D relay gateway={gateway} prefetch={prefetch} ack_ms={ack_ms} fault={fault} length={length}: {stats}");
                    cases += 1;
                }
            }
        }
    }
    println!("D relay matrix: {cases} cases passed; stock radio/gateway, modeled motor bootloader; no hardware");
}
