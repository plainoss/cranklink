//! Non-erasing experiment: compare sequential and prequeued acknowledged GETs.
use anyhow::{ensure, Result};
use clap::Parser;
use std::{fs, path::PathBuf};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    device: String,
    #[arg(long)]
    passkey_file: PathBuf,
    #[arg(long)]
    expect_serial: String,
    #[arg(long)]
    output_dir: PathBuf,
}

fn main() -> Result<()> {
    let a = Args::parse();
    ensure!(
        a.expect_serial.len() == 12 && a.expect_serial.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected serial must be 12 hex digits"
    );
    fs::create_dir_all(&a.output_dir)?;
    let key = zeroize::Zeroizing::new(fs::read_to_string(a.passkey_file)?);
    let port_str = if a.device.starts_with("ble-sim:") || a.device.starts_with("ble:") {
        a.device
    } else {
        format!("ble:{}", a.device)
    };
    let mut session = cranklink::ble::open(
        &port_str,
        Some(&a.output_dir.join("wire.jsonl")),
        Some(key.trim().to_owned()),
    )?;
    session.set_master_routing(true)?;
    session.authenticate()?;
    let result = (|| -> Result<serde_json::Value> {
        let before = session.start(None)?;
        let serial = cranklink::device::UnitCommands::command(
            &mut session,
            before.slot,
            1,
            0x3c,
            &[0],
            0x3e,
        )?;
        ensure!(
            hex::encode(cranklink::device::serial_bytes(&serial)?)
                .eq_ignore_ascii_case(&a.expect_serial),
            "motor serial mismatch"
        );
        let mut result = session.benchmark_acknowledged_reads()?;
        result["identity"] = serde_json::to_value(before)?;
        Ok(result)
    })();
    let stopped = session.stop();
    let closed = session.close();
    let value = result?;
    stopped?;
    closed?;
    fs::write(
        a.output_dir.join("result.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
