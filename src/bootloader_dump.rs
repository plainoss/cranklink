//! Read the fixed EP800 D resident window through its working application.
//!
//! This is the bounded read path used for the donor acquisition. It does not
//! enter firmware update mode, change adapter power, or write motor memory.
//!
//! Normal application service `32/8C -> 32/8E` reads 1-3 bytes per request.
//! It is admitted here only for EP800-CRG D4.1.11.0/4.2.0.0 with M4.1.11.0,
//! with model/board header `23 00 04 01` (hex) and resident version 5.0.6.1 checked
//! before acquisition. Reads cover the fixed 64 KiB resident window plus the
//! reviewed application version/model sample windows in `request`; this does
//! not establish a general memory-read capability of other E-TUBE components.
use crate::device::{serial_bytes, Identity, Session, UnitCommands, UnitIdentity};
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

const END: u32 = 0x10000;
const KNOWN_SHA256: &str = "59cc3fc007ca0b3595d8ba8e8207cc5257b05be84da8fd80278303d165488d9f";

#[derive(Debug, Serialize)]
pub struct DumpReceipt {
    pub serial: String,
    pub identity: Identity,
    pub bootloader_version: &'static str,
    pub start: u32,
    pub end_exclusive: u32,
    pub bytes: usize,
    pub passes: u8,
    pub sha256: String,
    pub matches_known_dump: bool,
    pub elapsed_seconds: f64,
}

/// Dump one 64 KiB pass from a working, already powered EP800-CRG.
///
/// `output` must be a new directory. Failed reads leave the partial binary and
/// an error receipt there. An optional serial selects one motor when several
/// are discovered; no serial is needed when there is exactly one EP800.
pub fn dump(port: &str, expected_serial: Option<&str>, output: &Path) -> Result<DumpReceipt> {
    if let Some(serial) = expected_serial {
        ensure!(
            serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()),
            "expected serial must contain 12 hexadecimal digits"
        );
    }
    fs::create_dir(output).context("output must be a new directory")?;
    let binary = output.join("resident.bin");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&binary)?;
    let started = Instant::now();
    let mut linked = false;
    let mut session = match Session::open(port, Some(&output.join("wire.jsonl"))) {
        Ok(session) => session,
        Err(error) => {
            write_result(
                output,
                &serde_json::json!({"success":false,"bytes":0,"error":format!("{error:#}")}),
            )?;
            return Err(error);
        }
    };
    let result = (|| -> Result<DumpReceipt> {
        // Session::initialize resets/unlocks the adapter supply. The proven
        // acquisition leaves the healthy bike's existing power untouched.
        session.set_protocol(0)?;
        adapter_alive(&mut session)?;
        let mut matches = Vec::new();
        for slot in session.discover()? {
            session.target(slot)?;
            let mut reply = vec![slot];
            reply.extend(session.command(slot, 1, 0x1c, &[0], 0x1e)?);
            let unit = UnitIdentity::from_reply(slot, reply)?;
            if unit.model_series == 0x23 && unit.model_unit == 0 {
                let serial = read_serial(&mut session, slot)?;
                if expected_serial.is_none_or(|expected| expected.eq_ignore_ascii_case(&serial)) {
                    matches.push((slot, serial));
                }
            }
        }
        ensure!(matches.len() == 1, "expected exactly one matching working EP800; found {} (use --expect-serial to select one)", matches.len());
        let (slot, serial) = matches.pop().unwrap();
        linked = true;
        let identity = session.normal_start_discovered(slot)?;
        validate_identity(&identity)?;
        ensure!(
            read_serial(&mut session, slot)? == serial,
            "motor serial changed during PC linking"
        );
        let prefix = check_samples(&mut session, &identity)?;
        ensure!(
            check_samples(&mut session, &identity)? == prefix,
            "independent bootloader samples disagree; full read not started"
        );
        let bytes = acquire(&mut file, &prefix, |address, count| {
            read(&mut session, slot, address, count)
        })?;
        ensure!(
            bytes[0x106..0x109] == [0x50, 6, 1],
            "dump bootloader version differs from sampled version"
        );
        ensure!(
            check_samples(&mut session, &identity)? == prefix,
            "bootloader samples changed during acquisition"
        );
        ensure!(
            read_serial(&mut session, slot)? == serial,
            "motor serial changed after acquisition"
        );
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Ok(DumpReceipt {
            serial,
            identity,
            bootloader_version: "5.0.6.1",
            start: 0,
            end_exclusive: END,
            bytes: bytes.len(),
            passes: 1,
            matches_known_dump: sha256 == KNOWN_SHA256,
            sha256,
            elapsed_seconds: started.elapsed().as_secs_f64(),
        })
    })();
    let stopped = if linked {
        session
            .normal_stop()
            .context("PC-link stop was not confirmed; no reset attempted")
    } else {
        Ok(())
    };
    let postflight = if let (Ok(receipt), Ok(())) = (&result, &stopped) {
        (|| -> Result<()> {
            adapter_alive(&mut session)?;
            ensure!(
                read_serial(&mut session, receipt.identity.slot)? == receipt.serial,
                "motor did not confirm its serial after PC-link stop"
            );
            Ok(())
        })()
    } else {
        Ok(())
    };
    let recorded = (|| -> Result<()> {
        file.sync_all()?;
        write_result(
            output,
            &serde_json::json!({
                "success":result.is_ok() && stopped.is_ok() && postflight.is_ok(),
                "bytes":file.metadata()?.len(),
                "result":result.as_ref().ok(),
                "error":result.as_ref().err().map(|e| format!("{e:#}")),
                "cleanup_error":stopped.as_ref().err().map(|e| format!("{e:#}")),
                "postflight_error":postflight.as_ref().err().map(|e| format!("{e:#}")),
            }),
        )
    })();
    // Report the acquisition or cleanup failure first; a receipt write
    // failure must not hide that the PC-link stop was never confirmed.
    let unrecorded = |error: anyhow::Error| match &recorded {
        Ok(()) => error,
        Err(write) => error.context(format!("result.json was not written: {write:#}")),
    };
    let receipt = result.map_err(unrecorded)?;
    stopped.map_err(unrecorded)?;
    postflight.map_err(unrecorded)?;
    recorded?;
    Ok(receipt)
}

fn write_result(output: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("result.json"))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn adapter_alive(session: &mut Session) -> Result<()> {
    let reply = session.exchange(5, &[], Duration::from_secs(2), |frame| {
        frame.control == 0x25 && frame.payload.len() == 3
    })?;
    ensure!(
        reply.payload == [0x30, 5, 0],
        "unsupported adapter version; no supply setup attempted"
    );
    Ok(())
}

fn read_serial(commands: &mut impl UnitCommands, slot: u8) -> Result<String> {
    Ok(hex::encode(serial_bytes(&commands.command(
        slot,
        1,
        0x3c,
        &[0],
        0x3e,
    )?)?))
}

fn validate_identity(identity: &Identity) -> Result<()> {
    ensure!(
        identity.cargo == "00-CRG"
            && matches!(identity.dcas_version.as_str(), "4.1.11.0" | "4.2.0.0")
            && identity.rx_version == "4.1.11.0",
        "bootloader extraction supports working EP800-CRG D4.1.11.0/4.2.0.0 with M4.1.11.0 only"
    );
    Ok(())
}

fn check_samples(commands: &mut impl UnitCommands, identity: &Identity) -> Result<Vec<u8>> {
    let slot = identity.slot;
    let expected = if identity.dcas_version == "4.1.11.0" {
        [0x41, 11, 0]
    } else {
        [0x42, 0, 0]
    };
    ensure!(
        read(commands, slot, 0x10010, 3)? == expected,
        "application version bytes differ from identity"
    );
    let mut header = read(commands, slot, 0x10028, 3)?;
    header.extend(read(commands, slot, 0x1002b, 1)?);
    ensure!(
        header == [0x23, 0, 4, 1],
        "unsupported application model/board header"
    );
    ensure!(
        read(commands, slot, 0x106, 3)? == [0x50, 6, 1],
        "bootloader version is not 5.0.6.1"
    );
    let mut prefix = Vec::with_capacity(64);
    for address in (0..64).step_by(3) {
        prefix.extend(read(
            commands,
            slot,
            address,
            (64 - address as usize).min(3),
        )?);
    }
    validate_vectors(&prefix)?;
    Ok(prefix)
}

fn validate_vectors(prefix: &[u8]) -> Result<()> {
    ensure!(prefix.len() == 64, "64-byte vector prefix required");
    let sp = u32::from_le_bytes(prefix[..4].try_into()?);
    let reset = u32::from_le_bytes(prefix[4..8].try_into()?);
    ensure!(
        (0x20000000..=0x20010000).contains(&sp) && sp % 4 == 0,
        "implausible initial stack pointer"
    );
    ensure!(
        reset & 1 == 1 && (reset & !1) < END,
        "reset vector is outside the resident window or not Thumb"
    );
    Ok(())
}

/// Parameters are `[address_high, address_low, address_middle, count, 00]`.
/// The address is 24-bit but its bytes are neither ordinary LE nor BE order.
/// Validate count and allowed windows before sending any command.
fn request(address: u32, count: usize) -> Result<[u8; 5]> {
    // The only reads outside the resident window are the reviewed application
    // version and model/board samples needed to verify the read handler.
    ensure!(
        count > 0
            && count <= 3
            && address.checked_add(count as u32).is_some_and(|end| {
                end <= END
                    || (address >= 0x10010 && end <= 0x10013)
                    || (address >= 0x10028 && end <= 0x1002c)
            }),
        "read outside fixed bootloader/sample windows"
    );
    Ok([
        (address >> 16) as u8,
        address as u8,
        (address >> 8) as u8,
        count as u8,
        0,
    ])
}

/// Replies echo the three address bytes in request order, then three data
/// positions, even for a 1- or 2-byte read. Accept exactly six parameters or
/// seven with reserved final FF; return only the requested data positions.
fn decode(address: u32, count: usize, params: &[u8]) -> Result<Vec<u8>> {
    request(address, count)?;
    ensure!(
        params.len() == 6 || (params.len() == 7 && params[6] == 0xff),
        "unexpected bootloader read response length/padding"
    );
    let echoed = (u32::from(params[0]) << 16) | u32::from(params[1]) | (u32::from(params[2]) << 8);
    ensure!(
        echoed == address,
        "read address echo mismatch at {address:#x}"
    );
    Ok(params[3..3 + count].to_vec())
}

fn read(commands: &mut impl UnitCommands, slot: u8, address: u32, count: usize) -> Result<Vec<u8>> {
    let params = commands.command(slot, 0x32, 0x8c, &request(address, count)?, 0x8e)?;
    decode(address, count, &params)
}

fn acquire(
    file: &mut File,
    prefix: &[u8],
    mut read: impl FnMut(u32, usize) -> Result<Vec<u8>>,
) -> Result<Vec<u8>> {
    validate_vectors(prefix)?;
    let mut bytes = Vec::with_capacity(END as usize);
    let mut checkpoint = 4096;
    for address in (0..END).step_by(3) {
        let count = ((END - address) as usize).min(3);
        let chunk = read(address, count)
            .with_context(|| format!("bootloader read at {address:#x}; partial binary retained"))?;
        ensure!(
            chunk.len() == count,
            "decoded chunk has wrong length at {address:#x}"
        );
        if address < 64 {
            let n = count.min(64 - address as usize);
            ensure!(
                chunk[..n] == prefix[address as usize..address as usize + n],
                "dump differs from validated vector prefix"
            );
        }
        file.write_all(&chunk)?;
        bytes.extend(chunk);
        if bytes.len() >= checkpoint || bytes.len() == END as usize {
            file.sync_all()?;
            eprintln!("Bootloader: {}/{} bytes", bytes.len(), END);
            checkpoint += 4096;
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefix() -> Vec<u8> {
        let mut bytes = vec![0; 64];
        bytes[..4].copy_from_slice(&0x20001000u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x101u32.to_le_bytes());
        bytes
    }

    #[test]
    fn request_allowlist_and_strict_replies() {
        assert_eq!(request(0xfffd, 3).unwrap(), [0, 0xfd, 0xff, 3, 0]);
        assert_eq!(request(0x10010, 3).unwrap(), [1, 0x10, 0, 3, 0]);
        for (a, n) in [
            (0xffff, 2),
            (END, 1),
            (0, 0),
            (0, 4),
            (u32::MAX, 3),
            (0x10011, 3),
            (0x1002b, 2),
        ] {
            assert!(request(a, n).is_err(), "{a:x}/{n}");
        }
        assert_eq!(
            decode(0xffff, 1, &[0, 0xff, 0xff, 7, 99, 88, 0xff]).unwrap(),
            [7]
        );
        for params in [
            &[0, 0xff, 0xff, 7][..],
            &[0, 0xff, 0xfe, 7, 99, 88],
            &[0, 0xff, 0xff, 7, 99, 88, 0],
        ] {
            assert!(decode(0xffff, 1, params).is_err());
        }
    }

    #[test]
    fn identity_and_vectors_are_bounded() {
        let mut identity = Identity {
            slot: 13,
            cargo: "00-CRG".into(),
            dcas_version: "4.1.11.0".into(),
            rx_version: "4.1.11.0".into(),
        };
        validate_identity(&identity).unwrap();
        identity.dcas_version = "4.3.0.0".into();
        assert!(validate_identity(&identity).is_err());
        validate_vectors(&prefix()).unwrap();
        let mut bytes = prefix();
        bytes[4..8].copy_from_slice(&0x10001u32.to_le_bytes());
        assert!(validate_vectors(&bytes).is_err());
        assert!(validate_vectors(&[0; 64]).is_err());
    }

    #[test]
    fn sampling_uses_only_the_application_read_command_and_stops_on_bad_header() {
        struct Reader {
            memory: Vec<u8>,
            addresses: Vec<u32>,
        }
        impl UnitCommands for Reader {
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                response: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!((slot, group, command, response), (13, 0x32, 0x8c, 0x8e));
                assert_eq!(params.len(), 5);
                assert_eq!(params[4], 0);
                let address = (u32::from(params[0]) << 16)
                    | u32::from(params[1])
                    | (u32::from(params[2]) << 8);
                self.addresses.push(address);
                let mut reply = params[..3].to_vec();
                reply.extend_from_slice(&self.memory[address as usize..address as usize + 3]);
                reply.push(0xff);
                Ok(reply)
            }
        }
        let identity = Identity {
            slot: 13,
            cargo: "00-CRG".into(),
            dcas_version: "4.1.11.0".into(),
            rx_version: "4.1.11.0".into(),
        };
        let mut reader = Reader {
            memory: vec![0; 0x10030],
            addresses: Vec::new(),
        };
        reader.memory[..64].copy_from_slice(&prefix());
        reader.memory[0x106..0x109].copy_from_slice(&[0x50, 6, 1]);
        reader.memory[0x10010..0x10013].copy_from_slice(&[0x41, 11, 0]);
        reader.memory[0x10028..0x1002c].copy_from_slice(&[0x23, 0, 4, 1]);
        assert_eq!(check_samples(&mut reader, &identity).unwrap(), prefix());
        reader.addresses.clear();
        reader.memory[0x10028] = 0x24;
        assert!(check_samples(&mut reader, &identity).is_err());
        assert_eq!(reader.addresses, [0x10010, 0x10028, 0x1002b]);
        let count = reader.addresses.len();
        assert!(read(&mut reader, 13, END, 1).is_err());
        assert_eq!(reader.addresses.len(), count); // Guard before any command.
    }

    #[test]
    fn an_existing_output_is_rejected_before_opening_any_adapter() {
        let output = std::env::temp_dir().join(format!(
            "cranklink-dump-existing-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        fs::create_dir(&output).unwrap();
        fs::write(output.join("resident.bin"), b"keep me").unwrap();
        let error = dump("not-a-port", None, &output).unwrap_err();
        assert!(error.to_string().contains("new directory"));
        assert_eq!(fs::read(output.join("resident.bin")).unwrap(), b"keep me");
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    fn acquisition_is_one_ordered_pass_and_retains_partial_data() {
        let path = std::env::temp_dir().join(format!(
            "cranklink-dump-test-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let prefix = prefix();
        let mut calls = 0;
        let bytes = acquire(&mut file, &prefix, |address, count| {
            assert_eq!(address, calls * 3);
            calls += 1;
            Ok((address..address + count as u32)
                .map(|a| prefix.get(a as usize).copied().unwrap_or(a as u8))
                .collect())
        })
        .unwrap();
        assert_eq!(calls, 21846);
        assert_eq!(bytes.len(), END as usize);
        assert_eq!(file.metadata().unwrap().len(), u64::from(END));
        file.set_len(0).unwrap();
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut calls = 0;
        assert!(acquire(&mut file, &prefix, |address, count| {
            calls += 1;
            ensure!(address < 9, "synthetic read failure");
            Ok(prefix[address as usize..address as usize + count].to_vec())
        })
        .is_err());
        assert_eq!(calls, 4); // No retry or subsequent requests after failure.
        assert_eq!(file.metadata().unwrap().len(), 9);
        drop(file);
        fs::remove_file(path).unwrap();
    }
}
