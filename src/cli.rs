//! Shared argument parser and command execution for native and browser workers.
macro_rules! out_print {($($arg:tt)*)=>{crate::cli::emit("stdout",format_args!($($arg)*))};}
macro_rules! out_println {($($arg:tt)*)=>{crate::cli::emit("stdout",format_args!("{}\n",format_args!($($arg)*)))};}
macro_rules! err_println {($($arg:tt)*)=>{crate::cli::emit("stderr",format_args!("{}\n",format_args!($($arg)*)))};}
macro_rules! notice_println {($($arg:tt)*)=>{crate::cli::emit_notice(format_args!("{}\n",format_args!($($arg)*)))};}
fn emit_notice(args: std::fmt::Arguments<'_>) {
    // Keep native JSON stdout clean; the browser can display notices separately.
    emit(
        if cfg!(target_arch = "wasm32") {
            "notice"
        } else {
            "stderr"
        },
        args,
    );
}
fn emit(stream: &str, args: std::fmt::Arguments<'_>) {
    #[cfg(target_arch = "wasm32")]
    crate::wasm::host_emit(stream, &args.to_string());
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        if stream == "stderr" {
            let _ = std::io::stderr().write_fmt(args);
        } else {
            let _ = std::io::stdout().write_fmt(args);
        }
    }
}
use crate::{
    device::Session,
    firmware::{Firmware, FirmwareKind, Version},
};
use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    version,
    about = "Shimano motor configuration and SM-PCE02 firmware tool",
    long_about = "Shimano motor configuration and SM-PCE02 firmware tool. Bluetooth settings and opt-in EP800 BLE firmware relay are experimental; BLE firmware hardware validation is pending. BLE recovery and radio DFU are disabled. Close E-TUBE before connecting. Firmware files are supplied separately."
)]
struct Cli {
    /// Allow discovery and application reads only; reject writes and mode-changing probes
    #[arg(long, global = true)]
    read_only: bool,
    /// Require this 12-hex-digit motor serial for a wired probe or settings operation
    #[arg(long, global = true)]
    expect_motor_serial: Option<String>,
    /// Select one resolved component model in the current session (probe/settings read/batch)
    #[arg(long, global = true, conflicts_with = "slot")]
    component: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Adapter (default: auto); COM3, usb, webusb, webserial, ble:DEVICE_ID, or webble"
    )]
    port: Option<String>,
    #[arg(
        long,
        global = true,
        help = "Read the six-digit BLE passkey from a file instead of prompting"
    )]
    ble_passkey_file: Option<PathBuf>,
    /// Require this 12-hex-digit motor serial inside the BLE settings connection
    #[arg(long, global = true)]
    ble_expect_serial: Option<String>,
    /// Experimental version-checked drive-unit settings authentication (one attempt)
    #[arg(long, global = true, requires = "ble_expect_serial")]
    ble_unit_auth: bool,
    /// Use stock gateway master routing (experimental; permits read-only validation)
    #[arg(long, global = true)]
    ble_master: bool,
    /// Opt in to experimental EP800 motor firmware relay over BLE
    #[arg(long, global = true)]
    ble_firmware: bool,
    /// New full settings export required before a BLE firmware write
    #[arg(long, global = true, requires = "ble_firmware")]
    ble_firmware_backup: Option<PathBuf>,
    /// One serial-checked legacy drive-unit authentication attempt over a wired connection
    #[arg(
        long,
        global = true,
        requires = "expect_motor_serial",
        conflicts_with = "ble_unit_auth"
    )]
    unit_auth: bool,
    #[arg(
        long,
        global = true,
        help = "Current discovery application slot (PnP can reassign it), or DCAS --recovery boot address"
    )]
    slot: Option<u8>,
    #[arg(
        long,
        global = true,
        help = "Raw transport trace as JSON lines (unit authentication excluded; BLE requires a new file)"
    )]
    trace: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Experimental Bluetooth discovery and authentication (native: --features ble)
    Ble {
        #[command(subcommand)]
        command: BleCommand,
    },
    /// List serial adapters and directly attached SM-PCE02 USB devices
    Ports,
    /// Decode and validate firmware offline
    Inspect {
        file: PathBuf,
        #[arg(long)]
        decode_to: Option<PathBuf>,
        #[arg(long)]
        companion: Option<PathBuf>,
    },
    /// Inventory connected component stock identities without motor authentication
    Units,
    /// Read motor model, serial and both MCU versions
    Probe,
    /// Read DCAS bootloader identity without erasing
    BootProbe {
        #[arg(long)]
        recovery: bool,
        /// Explicitly reset an already-recovering motor into its application
        #[arg(long, requires = "recovery")]
        reset: bool,
    },
    /// Read RX bootloader capability, reset RX, and verify application return
    RxProbe {
        /// Probe RX bootloader while its application image is unavailable
        #[arg(long)]
        recovery: bool,
    },
    /// Cycle adapter-supplied motor power and verify application return
    PowerCycle,
    /// Program a guarded M/Renesas image and verify application return
    FlashRx {
        file: PathBuf,
        #[arg(long)]
        companion: PathBuf,
        #[arg(long)]
        expect_serial: String,
        #[arg(long)]
        expect_sha256: String,
        #[arg(long)]
        journal: PathBuf,
        /// Recover RX through an intact, independently validated D application
        #[arg(long)]
        recovery: bool,
        #[arg(long)]
        write: bool,
    },
    /// Read, export, and change model-supported settings
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    /// Program a guarded D/DCASX image and verify both CPUs after reset
    FlashDcas {
        file: PathBuf,
        #[arg(long)]
        companion: PathBuf,
        #[arg(long)]
        expect_serial: String,
        #[arg(long)]
        expect_sha256: String,
        #[arg(long)]
        journal: PathBuf,
        #[arg(long)]
        recovery: bool,
        #[arg(long)]
        write: bool,
    },
}
#[derive(Subcommand)]
enum BleCommand {
    /// Guarded EP800 D+M upgrade in one serial-bound BLE transaction (default: non-erasing trial)
    FlashPair {
        #[arg(long)]
        dcas: PathBuf,
        #[arg(long)]
        rx: PathBuf,
        #[arg(long)]
        expect_serial: String,
        #[arg(long)]
        expect_dcas_sha256: String,
        #[arg(long)]
        expect_rx_sha256: String,
        #[arg(long)]
        journal: PathBuf,
        #[arg(long)]
        backup: PathBuf,
        #[arg(long)]
        write: bool,
    },
    /// Discover nearby E-TUBE gateways; --all also includes other advertisements
    Scan {
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=30))]
        seconds: u64,
        #[arg(long)]
        all: bool,
    },
    /// Authenticate and verify a normal gateway reply, then disconnect
    Authenticate,
    /// Read the unit slot bitmap after authentication
    Units,
    /// Attempt version-checked motor settings authentication without changing a setting
    UnitAuthenticate,
}
#[derive(Subcommand)]
enum SettingsCommand {
    /// List setting names, units, and accepted ranges
    Schema {
        #[arg(long, default_value = "ep800-crg", value_parser = ["ep800-crg", "ep600", "ep801"])]
        model: String,
    },
    /// Read one setting, or every supported setting when no name is given
    Read {
        name: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Change a setting with a durable original-value backup and readback
    Set {
        name: String,
        value: String,
        #[arg(long)]
        backup: PathBuf,
    },
    /// Restore one setting from a serial-bound backup
    Restore {
        file: PathBuf,
        #[arg(long)]
        backup: PathBuf,
    },
    /// Run an ordered JSON plan in one session, backing up and verifying each write
    Batch {
        plan: PathBuf,
        /// New prefix for PREFIX.journal.jsonl and PREFIX.NNN.backup.json
        #[arg(long)]
        backup_prefix: PathBuf,
        #[arg(skip)]
        operations: Vec<crate::settings::batch::Operation>,
    },
}
pub fn execute(args: Vec<String>) -> Result<()> {
    let mut cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            out_print!("{}", error);
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(!cli.read_only || read_only_command_allowed(&cli.command), "--read-only rejects settings writes/restores, privileged unit authentication, firmware, boot/recovery and power-cycle commands");
    ensure!(!cli.read_only || !cli.ble_unit_auth, "--read-only rejects --ble-unit-auth; use ordinary gateway authentication for baseline reads");
    ensure!(
        !cli.read_only || !cli.unit_auth,
        "--read-only rejects --unit-auth"
    );
    ensure!(
        !cli.unit_auth
            || (!cli.port.as_deref().is_some_and(crate::ble::is_port)
                && matches!(
                    cli.command,
                    Command::Probe
                        | Command::Settings {
                            command: SettingsCommand::Read { .. }
                                | SettingsCommand::Set { .. }
                                | SettingsCommand::Restore { .. }
                                | SettingsCommand::Batch { .. }
                        }
                )),
        "--unit-auth applies only to wired probe/settings operations"
    );
    if let Some(component) = &cli.component {
        ensure!(!component.trim().is_empty() && matches!(cli.command, Command::Probe | Command::Settings { command: SettingsCommand::Read { .. } | SettingsCommand::Batch { .. } }), "--component applies only to probe, settings read or batch; component-specific settings must be supported by the existing schema");
    }
    if let Some(expected) = &cli.expect_motor_serial {
        ensure!(
            expected.len() == 12 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
            "--expect-motor-serial requires exactly 12 hexadecimal digits"
        );
        ensure!(!cli.port.as_deref().is_some_and(crate::ble::is_port) && matches!(cli.command, Command::Probe | Command::Settings { command: SettingsCommand::Read { .. } | SettingsCommand::Set { .. } | SettingsCommand::Restore { .. } | SettingsCommand::Batch { .. } }), "--expect-motor-serial applies only to wired probe/settings operations; BLE uses --ble-expect-serial");
    }
    if let Command::Settings {
        command:
            SettingsCommand::Batch {
                plan,
                operations,
                backup_prefix,
            },
    } = &mut cli.command
    {
        ensure!(
            !backup_prefix.as_os_str().is_empty(),
            "backup prefix must not be empty"
        );
        *operations = crate::settings::batch::load(plan)?;
    }
    if let Command::Ble {
        command: BleCommand::Scan { seconds, all },
    } = &cli.command
    {
        out_println!(
            "{}",
            serde_json::to_string_pretty(&crate::ble::scan(*seconds, *all)?)?
        );
        return Ok(());
    }
    if cli.port.as_deref().is_some_and(crate::ble::is_port)
        && !matches!(cli.command, Command::Inspect { .. } | Command::Ports)
    {
        return execute_ble(cli);
    }
    ensure!(
        cli.ble_passkey_file.is_none(),
        "--ble-passkey-file requires a Bluetooth port"
    );
    ensure!(
        cli.ble_expect_serial.is_none(),
        "--ble-expect-serial requires a Bluetooth port"
    );
    ensure!(
        !cli.ble_unit_auth,
        "--ble-unit-auth requires a Bluetooth port"
    );
    ensure!(!cli.ble_master, "--ble-master requires a Bluetooth port");
    ensure!(
        !cli.ble_firmware,
        "--ble-firmware requires a Bluetooth port"
    );
    match cli.command {
        Command::Ble { .. } => {
            anyhow::bail!("select --port ble:DEVICE_ID (or connect ble in the browser)")
        }
        Command::Ports => {
            for p in crate::platform::available_ports()? {
                out_println!("{}  {}", p.port_name, p.port_type);
            }
        }
        Command::Inspect {
            file,
            decode_to,
            companion,
        } => {
            let fw = Firmware::load(file)?;
            if let Some(c) = companion {
                fw.validate_companion(&Firmware::load(c)?)?;
            }
            if let Some(out) = decode_to {
                use std::io::Write;
                let mut f = crate::storage::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&out)
                    .with_context(|| {
                        format!("create {} (must not already exist)", out.display())
                    })?;
                f.write_all(&fw.data)?;
                f.sync_all()?;
            }
            out_println!("{}", serde_json::to_string_pretty(&fw.metadata)?);
        }
        Command::Units => {
            let mut session =
                Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            let power = session.initialize()?;
            err_println!("Adapter ready: {}", serde_json::to_string(&power)?);
            let units = session.units()?;
            out_println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({"units": units}))?
            );
        }
        Command::Probe => {
            let mut session =
                Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            let power = session.initialize()?;
            err_println!("Adapter ready: {}", serde_json::to_string(&power)?);
            let result = session
                .probe_component(cli.slot, cli.component.as_deref())
                .and_then(|identity| {
                    check_motor_serial(&identity.serial, cli.expect_motor_serial.as_deref())?;
                    if cli.unit_auth {
                        let receipt = session.authenticate_unit_settings(
                            cli.expect_motor_serial.as_deref().unwrap(),
                        )?;
                        err_println!("Drive-unit authentication: {}", receipt);
                    }
                    Ok(identity)
                });
            let stopped = session.normal_stop();
            let identity =
                finish_operation(result, stopped.context("PC-link Stop acknowledgement"))?;
            out_println!("{}", serde_json::to_string_pretty(&identity)?);
        }
        Command::BootProbe { recovery, reset } => {
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            s.initialize()?;
            if !recovery {
                let i = s.normal_start(cli.slot)?;
                err_println!("Application: {}", serde_json::to_string(&i)?);
            }
            let identity = s.boot_start_at(!recovery, cli.slot)?;
            out_println!("{}", serde_json::to_string_pretty(&identity)?);
            if !recovery || reset {
                s.boot_reset()?;
                err_println!("Application reset acknowledged");
            }
        }
        Command::RxProbe { recovery } => {
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            s.initialize()?;
            let before = start_rx_session(&mut s, cli.slot, recovery)?;
            let boot = crate::rx::read_boot(&mut s, before.slot)?;
            out_println!("{}", serde_json::to_string_pretty(&boot)?);
            crate::rx::reset_fast(&mut s, before.slot)?;
            let after = start_rx_session(&mut s, Some(before.slot), recovery)?;
            s.normal_stop()?;
            ensure!(
                after.cargo == before.cargo
                    && after.dcas_version == before.dcas_version
                    && after.rx_version == before.rx_version,
                "application changed after RX probe"
            );
            err_println!(
                "{}: {}",
                if recovery {
                    "Cargo session restored; firmware remains in RX recovery"
                } else {
                    "Application return verified"
                },
                serde_json::to_string(&after)?
            );
        }
        Command::FlashRx {
            file,
            companion,
            expect_serial,
            expect_sha256,
            journal,
            recovery,
            write,
        } => {
            let fw = Firmware::load(file)?;
            let d = Firmware::load(companion)?;
            ensure!(
                fw.metadata.kind == FirmwareKind::Renesas,
                "flash-rx requires an M image"
            );
            fw.validate_companion(&d)?;
            ensure!(
                fw.metadata
                    .source_sha256
                    .eq_ignore_ascii_case(&expect_sha256),
                "source SHA-256 differs from --expect-sha256"
            );
            let serial = hex::decode(expect_serial.replace(['-', ':'], ""))?;
            ensure!(serial.len() == 6, "expected six-byte serial");
            let mut journal = Journal::new(&journal)?;
            journal.record("inputs", &fw.metadata)?;
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            s.initialize()?;
            let before = start_rx_session(&mut s, cli.slot, recovery)?;
            if !recovery {
                ensure!(
                    before.dcas_version == d.metadata.version.to_string(),
                    "live D version differs from supplied companion; flash D first"
                );
            }
            journal.record("application-before", &before)?;
            let boot = s.boot_start(true)?;
            journal.record("dcas-identity", &boot)?;
            let guard = (|| {
                ensure!(
                    boot.serial.as_slice() == serial,
                    "live serial differs from expected"
                );
                d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))
            })();
            s.boot_reset()?;
            guard?;
            let ready = start_rx_session(&mut s, Some(before.slot), recovery)?;
            if !write {
                let rx = crate::rx::read_boot(&mut s, ready.slot)?;
                journal.record("rx-boot", &rx)?;
                crate::rx::reset_fast(&mut s, before.slot)?;
                let after = start_rx_session(&mut s, Some(before.slot), recovery)?;
                s.normal_stop()?;
                journal.record("dry-run-complete", &after)?;
                out_println!("Compatibility checks passed; use --write to program the image");
                return Ok(());
            }
            journal.record("destructive-start", &fw.metadata)?;
            let receipt = match crate::rx::flash(&mut s, ready.slot, &fw, progress_reporter()) {
                Ok(r) => r,
                Err(e) => {
                    journal.record("uncertain", &format!("{e:#}"))?;
                    return Err(e);
                }
            };
            journal.record("finish-and-reset", &receipt)?;
            let after = s.normal_start(Some(before.slot))?;
            s.normal_stop()?;
            journal.record("application-after", &after)?;
            ensure!(
                after.cargo == before.cargo && after.dcas_version == d.metadata.version.to_string(),
                "cargo or D version changed after RX update"
            );
            ensure!(
                after.rx_version == fw.metadata.version.to_string(),
                "post-flash RX version mismatch"
            );
            journal.record("verified", &after)?;
            out_println!("{}", serde_json::to_string_pretty(&after)?);
        }
        Command::Settings {
            command: SettingsCommand::Schema { model },
        } => out_println!(
            "{}",
            serde_json::to_string_pretty(&crate::settings::schema_for_model(settings_model_name(
                &model
            )))?
        ),
        Command::Settings { command } => {
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            s.initialize()?;
            let result = s
                .normal_start_component(cli.slot, cli.component.as_deref())
                .and_then(|id| {
                    if let Some(expected) = cli.expect_motor_serial.as_deref() {
                        let serial = s.command(id.slot, 1, 0x3c, &[0], 0x3e)?;
                        check_motor_serial(
                            &hex::encode(crate::device::serial_bytes(&serial)?),
                            Some(expected),
                        )?;
                    }
                    err_println!("Motor ready: {}", serde_json::to_string(&id)?);
                    if cli.unit_auth {
                        let receipt = s.authenticate_unit_settings(
                            cli.expect_motor_serial.as_deref().unwrap(),
                        )?;
                        err_println!("Drive-unit authentication: {}", receipt);
                    }
                    execute_settings(&mut s, &id, command)
                });
            let stopped = s.normal_stop();
            let value = finish_operation(result, stopped.context("PC-link Stop acknowledgement"))?;
            out_println!("{}", serde_json::to_string_pretty(&value)?);
        }
        Command::PowerCycle => {
            use crate::platform::sleep;
            use std::time::Duration;
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            let power = s.initialize()?;
            ensure!(
                power.adapter_supply,
                "power-cycle requires an adapter-supplied motor without an external battery"
            );
            let before = s.normal_start(cli.slot)?;
            s.normal_stop()?;
            let ack = s.exchange(0x1a, &[4], Duration::from_secs(3), |f| {
                f.control == 0x3a && f.payload.first() == Some(&4)
            })?;
            ensure!(ack.payload.get(1) == Some(&0), "power-stop rejected");
            sleep(Duration::from_secs(3));
            let off = s.command(before.slot, 1, 0x2c, &[0], 0x2e);
            ensure!(
                off.is_err(),
                "motor still responds after adapter supply stop"
            );
            s.initialize()?;
            let after = s.normal_start(Some(before.slot))?;
            s.normal_stop()?;
            ensure!(
                after.cargo == before.cargo
                    && after.dcas_version == before.dcas_version
                    && after.rx_version == before.rx_version,
                "motor identity/version changed across power cycle"
            );
            out_println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"power_stop_acknowledged":true,"motor_unresponsive_while_off":true,"application":after})
                )?
            );
        }
        Command::FlashDcas {
            file,
            companion,
            expect_serial,
            expect_sha256,
            journal,
            recovery,
            write,
        } => {
            let fw = Firmware::load(file)?;
            ensure!(
                fw.metadata.kind == FirmwareKind::DcasX,
                "flash-dcas requires a D image"
            );
            fw.validate_companion(&Firmware::load(companion)?)?;
            ensure!(
                fw.metadata
                    .source_sha256
                    .eq_ignore_ascii_case(&expect_sha256),
                "source SHA-256 does not match --expect-sha256"
            );
            let serial = hex::decode(expect_serial.replace(['-', ':'], ""))?;
            ensure!(serial.len() == 6, "--expect-serial must contain six bytes");
            let mut journal = Journal::new(&journal)?;
            journal.record("inputs", &fw.metadata)?;
            let mut s = Session::open(cli.port.as_deref().unwrap_or("auto"), cli.trace.as_deref())?;
            s.initialize()?;
            let before = if recovery {
                None
            } else {
                Some(s.normal_start(cli.slot)?)
            };
            if let Some(i) = &before {
                journal.record("application-before", i)?;
                fw.validate_companion_version(parse_version(&i.rx_version)?)?;
            }
            let boot = s.boot_start_at(!recovery, cli.slot)?;
            journal.record("boot-identity", &boot)?;
            let guard = (|| {
                ensure!(
                    boot.serial.as_slice() == serial,
                    "live serial differs from --expect-serial"
                );
                fw.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))
            })();
            if let Err(e) = guard {
                journal.record("guard-rejected", &format!("{e:#}"))?;
                if !recovery {
                    s.boot_reset()
                        .context("guard rejected image; application reset failed")?;
                }
                return Err(e);
            }
            if !write {
                if !recovery {
                    s.boot_reset()?;
                }
                journal.record("dry-run-complete", &boot)?;
                out_println!("Compatibility checks passed; use --write to program the image");
                return Ok(());
            }
            journal.record("destructive-start", &fw.metadata)?;
            let receipt = match s.flash_dcas(&fw.data, &boot, progress_reporter()) {
                Ok(r) => r,
                Err(e) => {
                    journal.record("uncertain", &format!("{e:#}"))?;
                    return Err(e);
                }
            };
            journal.record("finish-and-reset", &receipt)?;
            let after = s.normal_start(if recovery { None } else { cli.slot })?;
            let stop = s.normal_stop();
            journal.record("application-after", &after)?;
            stop?;
            ensure!(
                after.dcas_version == fw.metadata.version.to_string(),
                "post-flash DCAS version mismatch"
            );
            ensure!(
                after.rx_version == fw.metadata.compatibility_version.to_string(),
                "post-flash RX version mismatch"
            );
            if let Some(before) = before {
                ensure!(
                    before.cargo == after.cargo,
                    "cargo identity changed after flashing"
                );
            }
            journal.record("verified", &after)?;
            out_println!("{}", serde_json::to_string_pretty(&after)?);
        }
    }
    Ok(())
}
fn execute_ble(cli: Cli) -> Result<()> {
    if cli.ble_firmware {
        return execute_ble_firmware(cli);
    }
    // Guard before credential/file access, radio discovery, or journal creation.
    ensure!(ble_command_allowed(&cli.command), "Command unavailable in a BLE settings session. Supported EP800 motor firmware relay requires --ble-firmware; BLE recovery, radio DFU, power-cycle and raw operations are disabled");
    let auth_only = matches!(
        cli.command,
        Command::Ble {
            command: BleCommand::UnitAuthenticate
        }
    );
    ensure!(
        !auth_only || cli.ble_expect_serial.is_some(),
        "ble unit-authenticate requires --ble-expect-serial"
    );
    ensure!(
        !cli.ble_unit_auth
            || matches!(cli.command, Command::Probe | Command::Settings { .. })
            || auth_only,
        "--ble-unit-auth applies only to probe, settings, or ble unit-authenticate"
    );
    if let Command::Settings {
        command: SettingsCommand::Schema { model },
    } = &cli.command
    {
        out_println!(
            "{}",
            serde_json::to_string_pretty(&crate::settings::schema_for_model(settings_model_name(
                model
            )))?
        );
        return Ok(());
    }
    if let Some(serial) = &cli.ble_expect_serial {
        ensure!(
            serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()),
            "--ble-expect-serial requires exactly 12 hexadecimal digits"
        );
    }
    let passkey = read_ble_passkey(&cli)?;
    notice_println!("Experimental Bluetooth transport: hardware validation pending.");
    let mut session =
        crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?;
    session.set_master_routing(cli.ble_master)?;
    let result = (|| -> Result<serde_json::Value> {
        session.authenticate()?;
        match cli.command {
            Command::Ble {
                command: BleCommand::Authenticate,
            } => Ok(
                serde_json::json!({"authenticated":true,"protocol_reply_verified":true,"hardware_validated":false}),
            ),
            Command::Units => Ok(serde_json::json!({"units": session.units()?})),
            Command::Ble {
                command: BleCommand::Units,
            } => Ok(serde_json::json!({"slots":session.discover_units()?})),
            Command::Ble {
                command: BleCommand::UnitAuthenticate,
            } => {
                let (id, serial) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.ble_expect_serial.as_deref(),
                )?;
                let receipt = session
                    .authenticate_unit_settings(cli.ble_expect_serial.as_deref().unwrap())?;
                Ok(serde_json::json!({"identity":id,"serial":serial,"authentication":receipt}))
            }
            Command::Probe => {
                let (id, serial) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.ble_expect_serial.as_deref(),
                )?;
                let mut value = serde_json::to_value(&id)?;
                value["serial"] = serde_json::json!(serial);
                let model = crate::device::UnitCommands::model_identity(&mut session, id.slot)?;
                ensure!(model.len() >= 3, "truncated motor model reply");
                value["model_series"] = serde_json::json!(model[1]);
                value["model_unit"] = serde_json::json!(model[2]);
                if cli.ble_unit_auth {
                    value["unit_authentication"] = session
                        .authenticate_unit_settings(cli.ble_expect_serial.as_deref().unwrap())?;
                }
                Ok(value)
            }
            Command::Settings { command } => {
                let (id, _) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.ble_expect_serial.as_deref(),
                )?;
                if cli.ble_unit_auth {
                    session
                        .authenticate_unit_settings(cli.ble_expect_serial.as_deref().unwrap())?;
                }
                execute_settings(&mut session, &id, command)
            }
            _ => unreachable!(),
        }
    })();
    let stopped = session.stop();
    let closed = session.close();
    let result = finish_operation(
        result,
        stopped.context("BLE PC-link stop was not confirmed"),
    );
    let value = finish_operation(result, closed.context("BLE disconnect failed"))?;
    out_println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn read_ble_passkey(cli: &Cli) -> Result<Option<String>> {
    cli.ble_passkey_file
        .as_ref()
        .map(|p| {
            let bytes = crate::storage::read_bounded(p, 16)?;
            let text = zeroize::Zeroizing::new(
                String::from_utf8(bytes)
                    .map_err(|_| anyhow::anyhow!("BLE passkey file must contain ASCII digits"))?,
            );
            let value = text.trim_end_matches(['\r', '\n']).to_owned();
            ensure!(
                value.len() == 6 && value.bytes().all(|b| b.is_ascii_digit()),
                "BLE passkey file must contain six ASCII digits"
            );
            Ok::<_, anyhow::Error>(value)
        })
        .transpose()
}

fn ble_firmware_backup(
    session: &mut crate::ble::BleSession,
    id: &crate::device::Identity,
    serial: &str,
    path: &std::path::Path,
) -> Result<serde_json::Value> {
    use std::io::Write;
    let mut backup = execute_settings(
        session,
        id,
        SettingsCommand::Read {
            name: None,
            output: None,
        },
    )?;
    ensure!(
        backup["unavailable"]
            .as_object()
            .is_some_and(|m| m.is_empty()),
        "incomplete settings backup; refusing firmware write"
    );
    backup["serial"] = serde_json::json!(serial);
    backup["identity"] = serde_json::to_value(id)?;
    let mut f = crate::storage::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut f, &backup)?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    Ok(backup)
}
fn execute_ble_pair(cli: Cli) -> Result<()> {
    ensure!(
        !cli.ble_unit_auth,
        "omit --ble-unit-auth for firmware updates"
    );
    let Command::Ble {
        command:
            BleCommand::FlashPair {
                dcas,
                rx,
                expect_serial,
                expect_dcas_sha256,
                expect_rx_sha256,
                journal,
                backup,
                write,
            },
    } = &cli.command
    else {
        unreachable!()
    };
    ensure!(
        expect_serial.len() == 12 && expect_serial.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected firmware serial must be 12 hex digits"
    );
    if let Some(s) = &cli.ble_expect_serial {
        ensure!(
            s.eq_ignore_ascii_case(expect_serial),
            "conflicting expected serials"
        );
    }
    let serial: [u8; 6] = hex::decode(expect_serial)?.try_into().unwrap();
    let d = Firmware::load(dcas)?;
    let m = Firmware::load(rx)?;
    ensure!(
        d.metadata.kind == FirmwareKind::DcasX && m.metadata.kind == FirmwareKind::Renesas,
        "flash-pair requires D and M images"
    );
    ensure!(
        d.metadata.series == 0x23 && d.metadata.unit == 0 && d.metadata.board == Some(1),
        "BLE pair updates support EP800 board 1 only"
    );
    ensure!(
        matches!(d.metadata.version.0, [4, 1, 11, 0] | [4, 2, 0, 0]),
        "BLE pair updater requires a supported EP800 target version"
    );
    d.validate_companion(&m)?;
    ensure!(
        d.metadata
            .source_sha256
            .eq_ignore_ascii_case(expect_dcas_sha256)
            && m.metadata
                .source_sha256
                .eq_ignore_ascii_case(expect_rx_sha256),
        "firmware SHA-256 mismatch"
    );
    ensure!(!backup.exists(), "backup already exists; use a new path");
    let journal_path = journal.clone();
    let mut journal = Journal::new(journal)?;
    journal.record("inputs",&serde_json::json!({"dcas":d.metadata,"rx":m.metadata,"write":write,"serial":expect_serial,"transport":"ble"}))?;
    let passkey = read_ble_passkey(&cli)?;
    let mut session =
        crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?;
    session.set_master_routing(true)?;
    let result = (|| -> Result<serde_json::Value> {
        session.authenticate()?;
        let (before, _) = ble_checked_start(&mut session, cli.slot, None, Some(expect_serial))?;
        ensure!(
            parse_version(&before.dcas_version)? <= d.metadata.version
                && parse_version(&before.rx_version)? <= m.metadata.version,
            "flash-pair only permits upgrades, not downgrades"
        );
        journal.record("application-before", &before)?;
        let settings_before = ble_firmware_backup(&mut session, &before, expect_serial, backup)?;
        journal.record(
            "settings-backup",
            &serde_json::json!({"path":backup,"serial":expect_serial}),
        )?;
        // Both non-erasing probes precede every destructive operation. Prove RX
        // return first, then independently read the D boot identity immediately
        // before flashing, without changing the motor's selected identity.
        let mut link = session.motor_firmware(&serial)?;
        let rx_boot = link.enter_rx()?;
        journal.record("rx-boot-before", &rx_boot)?;
        link.reset_rx()?;
        link.application_return()?;
        let (checked, _) = ble_checked_start(&mut session, None, None, Some(expect_serial))?;
        ensure!(
            checked.cargo == before.cargo
                && checked.dcas_version == before.dcas_version
                && checked.rx_version == before.rx_version,
            "RX probe did not restore the unchanged application"
        );
        let mut link = session.motor_firmware(&serial)?;
        let boot = link.enter_dcas()?;
        journal.record("dcas-boot-before", &boot)?;
        let guard = (|| -> Result<()> {
            ensure!(boot.serial == serial, "boot serial changed");
            d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))
        })();
        if guard.is_err() || !*write {
            link.reset_dcas()?;
        }
        if let Err(e) = guard {
            link.application_return()?;
            return Err(e);
        }
        if *write {
            journal.record("dcas-destructive-start", &d.metadata)?;
            let receipt = link.flash_dcas(&d.data, &boot, progress_reporter())?;
            journal.record("dcas-finish-and-reset", &receipt)?;
        }
        link.application_return()?;
        // A changed D/M pair can reject headline versions with status 39
        // until M is updated. Require the acknowledged D journal and fresh
        // serial/cargo/boot checks, then read both versions strictly at the end.
        let incomplete_pair = *write && before.rx_version != m.metadata.version.to_string();
        if !incomplete_pair {
            let (middle, _) = ble_checked_start(&mut session, None, None, Some(expect_serial))?;
            ensure!(
                middle.cargo == before.cargo && middle.rx_version == before.rx_version,
                "identity or RX changed after D operation"
            );
            ensure!(
                middle.dcas_version
                    == if *write {
                        d.metadata.version.to_string()
                    } else {
                        before.dcas_version.clone()
                    },
                "D version readback failed"
            );
            journal.record("application-between", &middle)?;
        } else {
            journal.record("application-between-version-deferred", &serde_json::json!({"reason":"M companion still requires updating; strict two-CPU readback follows M", "dcas_finish_and_reset_acknowledged":true}))?;
        }
        if *write {
            let mut link = if incomplete_pair {
                session.continue_pair_rx(&serial, &d, &m, &journal_path)?
            } else {
                session.motor_firmware(&serial)?
            };
            let boot = link.enter_rx()?;
            ensure!(
                boot.version == rx_boot.version
                    && boot.capability_sequence == rx_boot.capability_sequence,
                "RX boot capability changed before erase"
            );
            journal.record("rx-destructive-start", &m.metadata)?;
            let receipt = link.flash_rx(&m, boot, progress_reporter())?;
            journal.record("rx-finish-and-reset", &receipt)?;
            link.application_return()?;
        }
        let (after, _) = ble_checked_start(&mut session, None, None, Some(expect_serial))?;
        ensure!(
            after.cargo == before.cargo,
            "cargo identity changed after pair operation"
        );
        ensure!(
            after.dcas_version
                == if *write {
                    d.metadata.version.to_string()
                } else {
                    before.dcas_version.clone()
                },
            "final D version mismatch"
        );
        ensure!(
            after.rx_version
                == if *write {
                    m.metadata.version.to_string()
                } else {
                    before.rx_version.clone()
                },
            "final RX version mismatch"
        );
        let settings_after = execute_settings(
            &mut session,
            &after,
            SettingsCommand::Read {
                name: None,
                output: None,
            },
        )?;
        journal.record("settings-after", &settings_after)?;
        ensure!(
            settings_after["unavailable"]
                .as_object()
                .is_some_and(|m| m.is_empty()),
            "post-update settings export incomplete"
        );
        ensure!(
            crate::settings::decoded_snapshots_equal(&settings_before, &settings_after),
            "settings changed after firmware operation; inspect backup and journal"
        );
        journal.record(
            if *write {
                "verified"
            } else {
                "dry-run-complete"
            },
            &after,
        )?;
        Ok(
            serde_json::json!({"transport":"ble","write":write,"serial":expect_serial,"application_before":before,"application_after":after,"settings_unchanged":true}),
        )
    })();
    if let Err(e) = &result {
        journal.record("uncertain", &format!("{e:#}"))?;
    }
    let stopped = session.stop();
    let closed = session.close();
    let result = finish_operation(result, stopped.context("BLE PC-link cleanup not confirmed"));
    let result = finish_operation(result, closed.context("BLE disconnect failed"));
    if result.is_ok() {
        journal.record("cleanup-confirmed", &true)?;
    }
    let value = result?;
    out_println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

/// Complete all offline guards before credential access or radio ownership.
fn execute_ble_firmware(cli: Cli) -> Result<()> {
    if matches!(
        cli.command,
        Command::Ble {
            command: BleCommand::FlashPair { .. }
        }
    ) {
        return execute_ble_pair(cli);
    }
    ensure!(
        !cli.ble_unit_auth,
        "firmware entry uses its own protocol; omit --ble-unit-auth"
    );
    let (rx, file, companion, serial, sha, journal_path, write) = match &cli.command {
        Command::BootProbe { recovery:false, .. } => (false,None,None,cli.ble_expect_serial.clone(),None,None,false),
        Command::RxProbe { recovery:false } => (true,None,None,cli.ble_expect_serial.clone(),None,None,false),
        Command::FlashDcas { file,companion,expect_serial,expect_sha256,journal,recovery:false,write } => (false,Some(file),Some(companion),Some(expect_serial.clone()),Some(expect_sha256),Some(journal),*write),
        Command::FlashRx { file,companion,expect_serial,expect_sha256,journal,recovery:false,write } => (true,Some(file),Some(companion),Some(expect_serial.clone()),Some(expect_sha256),Some(journal),*write),
        _ => anyhow::bail!("--ble-firmware permits normal-mode boot-probe/rx-probe/flash-dcas/flash-rx only; BLE recovery is disabled"),
    };
    let serial = serial.context(
        "BLE firmware operations require --ble-expect-serial (or flash --expect-serial)",
    )?;
    ensure!(
        serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()),
        "expected firmware serial must be 12 hex digits"
    );
    if let Some(s) = &cli.ble_expect_serial {
        ensure!(
            s.eq_ignore_ascii_case(&serial),
            "conflicting BLE/firmware expected serials"
        );
    }
    let serial_bytes: [u8; 6] = hex::decode(&serial)?.try_into().unwrap();
    let firmware = if let Some(path) = file {
        let fw = Firmware::load(path)?;
        let other = Firmware::load(companion.unwrap())?;
        ensure!(
            fw.metadata.kind
                == if rx {
                    FirmwareKind::Renesas
                } else {
                    FirmwareKind::DcasX
                },
            "wrong firmware kind for command"
        );
        ensure!(
            fw.metadata.series == 0x23 && fw.metadata.unit == 0,
            "BLE firmware supports EP800 23/00 only"
        );
        fw.validate_companion(&other)?;
        ensure!(
            fw.metadata.source_sha256.eq_ignore_ascii_case(sha.unwrap()),
            "source SHA-256 does not match --expect-sha256"
        );
        Some((fw, other))
    } else {
        None
    };
    ensure!(
        !write || cli.ble_firmware_backup.is_some(),
        "BLE firmware writes require --ble-firmware-backup PATH"
    );
    if let Some(path) = &cli.ble_firmware_backup {
        ensure!(!path.exists(), "BLE firmware backup already exists");
    }
    let mut journal = journal_path.map(|p| Journal::new(p)).transpose()?;
    if let (Some(j), Some((fw, other))) = (&mut journal, &firmware) {
        j.record(
            "inputs",
            &serde_json::json!({"image":fw.metadata,"companion":other.metadata,"transport":"ble"}),
        )?;
    }
    let passkey = read_ble_passkey(&cli)?;
    notice_println!("Experimental BLE motor firmware: first verify a non-erasing probe; no automatic erase/program retries.");
    let mut session =
        crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?;
    session.set_master_routing(true)?;
    let result = (|| -> Result<serde_json::Value> {
        session.authenticate()?;
        let (before, _) = ble_checked_start(&mut session, cli.slot, None, Some(&serial))?;
        if let Some(j) = &mut journal {
            j.record("application-before", &before)?;
        }
        if let Some(path) = &cli.ble_firmware_backup {
            let backup = execute_settings(
                &mut session,
                &before,
                SettingsCommand::Read {
                    name: None,
                    output: Some(path.clone()),
                },
            )?;
            ensure!(
                backup["unavailable"]
                    .as_object()
                    .is_some_and(|m| m.is_empty()),
                "full BLE settings backup has unavailable values; refusing firmware write"
            );
            if let Some(j) = &mut journal {
                j.record(
                    "settings-backup",
                    &serde_json::json!({"path":path,"serial":serial}),
                )?;
            }
        }
        // Independently prove the D boot serial, product and board before either erase.
        let mut link = session.motor_firmware(&serial_bytes)?;
        let boot = link.enter_dcas()?;
        if let Some(j) = &mut journal {
            j.record("dcas-boot-identity", &boot)?;
        }
        let guard = (|| -> Result<()> {
            ensure!(
                boot.serial == serial_bytes
                    && boot.series == 0x23
                    && boot.unit == 0
                    && boot.board == 1
                    && boot.version[0] >= 5,
                "BLE boot identity/board/serial mismatch"
            );
            if let Some((fw, other)) = &firmware {
                let d = if rx { other } else { fw };
                d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))?;
                if !rx {
                    fw.validate_companion_version(parse_version(&before.rx_version)?)?;
                } else {
                    ensure!(
                        before.dcas_version == other.metadata.version.to_string(),
                        "RX companion must match the running D image"
                    );
                }
            }
            Ok(())
        })();
        if rx || !write || guard.is_err() {
            link.reset_dcas()?;
        }
        if let Err(e) = guard {
            link.application_return()?;
            return Err(e);
        }
        let receipt = if rx {
            link.application_return()?;
            let (current, _) = ble_checked_start(&mut session, None, None, Some(&serial))?;
            ensure!(
                current.cargo == before.cargo
                    && current.dcas_version == before.dcas_version
                    && current.rx_version == before.rx_version,
                "application changed during D boot probe"
            );
            let mut link = session.motor_firmware(&serial_bytes)?;
            let rx_boot = link.enter_rx()?;
            if let Some(j) = &mut journal {
                j.record("rx-boot-identity", &rx_boot)?;
            }
            let value = if write {
                let fw = &firmware.as_ref().unwrap().0;
                if let Some(j) = &mut journal {
                    j.record("destructive-start", &fw.metadata)?;
                }
                serde_json::to_value(link.flash_rx(fw, rx_boot, progress_reporter())?)?
            } else {
                link.reset_rx()?;
                serde_json::to_value(rx_boot)?
            };
            link.application_return()?;
            value
        } else {
            let value = if write {
                let fw = &firmware.as_ref().unwrap().0;
                if let Some(j) = &mut journal {
                    j.record("destructive-start", &fw.metadata)?;
                }
                serde_json::to_value(link.flash_dcas(&fw.data, &boot, progress_reporter())?)?
            } else {
                serde_json::to_value(boot)?
            };
            link.application_return()?;
            value
        };
        let (after, _) = ble_checked_start(&mut session, None, None, Some(&serial))?;
        ensure!(
            after.cargo == before.cargo,
            "cargo identity changed after BLE firmware operation"
        );
        if write {
            let fw = &firmware.as_ref().unwrap().0;
            ensure!(
                (if rx {
                    &after.rx_version
                } else {
                    &after.dcas_version
                }) == &fw.metadata.version.to_string(),
                "post-flash firmware version mismatch"
            );
            ensure!(
                if rx {
                    after.dcas_version == before.dcas_version
                } else {
                    after.rx_version == before.rx_version
                },
                "companion MCU changed unexpectedly"
            );
        } else {
            ensure!(
                before.dcas_version == after.dcas_version && before.rx_version == after.rx_version,
                "version changed during non-erasing BLE probe"
            );
        }
        if let Some(j) = &mut journal {
            j.record(
                if write {
                    "verified"
                } else {
                    "dry-run-complete"
                },
                &after,
            )?;
        }
        Ok(
            serde_json::json!({"transport":"ble","write":write,"application_before":before,"receipt":receipt,"application_after":after,"serial":serial}),
        )
    })();
    if let (Err(error), Some(j)) = (&result, &mut journal) {
        j.record("uncertain", &format!("{error:#}"))?;
    }
    let stopped = session.stop();
    let closed = session.close();
    let result = finish_operation(
        result,
        stopped.context("BLE firmware cleanup not confirmed"),
    );
    let value = finish_operation(result, closed.context("BLE disconnect failed"))?;
    out_println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn ble_checked_start(
    session: &mut crate::ble::BleSession,
    slot: Option<u8>,
    component: Option<&str>,
    expected: Option<&str>,
) -> Result<(crate::device::Identity, String)> {
    let id = session.start_component(slot, component)?;
    let serial = crate::device::UnitCommands::command(session, id.slot, 1, 0x3c, &[0], 0x3e)?;
    let serial = hex::encode(crate::device::serial_bytes(&serial)?);
    if let Some(expected) = expected {
        ensure!(
            serial.eq_ignore_ascii_case(expected),
            "BLE motor serial mismatch: expected {expected}, got {serial}"
        );
    }
    Ok((id, serial))
}
fn check_motor_serial(actual: &str, expected: Option<&str>) -> Result<()> {
    if let Some(expected) = expected {
        ensure!(
            actual.eq_ignore_ascii_case(expected),
            "motor serial mismatch: expected {expected}, got {actual}"
        );
    }
    Ok(())
}
fn finish_operation<T>(result: Result<T>, cleanup: Result<()>) -> Result<T> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("cleanup also failed: {cleanup:#}")))
        }
    }
}
fn read_only_command_allowed(command: &Command) -> bool {
    matches!(
        command,
        Command::Ports
            | Command::Inspect { .. }
            | Command::Units
            | Command::Probe
            | Command::Settings {
                command: SettingsCommand::Read { .. } | SettingsCommand::Schema { .. }
            }
            | Command::Ble {
                command: BleCommand::Scan { .. } | BleCommand::Authenticate | BleCommand::Units
            }
    )
}
fn ble_command_allowed(command: &Command) -> bool {
    matches!(
        command,
        Command::Probe
            | Command::Units
            | Command::Settings { .. }
            | Command::Ble {
                command: BleCommand::Authenticate
                    | BleCommand::Units
                    | BleCommand::UnitAuthenticate
            }
    )
}

fn settings_model_name(name: &str) -> crate::settings::SettingsModel {
    match name {
        "ep600" => crate::settings::SettingsModel::Ep600,
        "ep801" => crate::settings::SettingsModel::Ep801,
        _ => crate::settings::SettingsModel::Ep800Crg,
    }
}

fn execute_settings(
    session: &mut dyn crate::device::UnitCommands,
    id: &crate::device::Identity,
    command: SettingsCommand,
) -> Result<serde_json::Value> {
    let identity = session.model_identity(id.slot)?;
    ensure!(
        identity.len() >= 3 && identity[2] == 0,
        "unsupported settings motor identity"
    );
    let model = match identity[1] {
        0x23 if id.cargo == "00-CRG" => crate::settings::SettingsModel::Ep800Crg,
        0x30 if id.dcas_version == "4.6.0.1" => crate::settings::SettingsModel::Ep600,
        0x35 if id.dcas_version == "4.6.0.1" => crate::settings::SettingsModel::Ep801,
        _ => anyhow::bail!("settings schema is not verified for this model/firmware"),
    };
    match command {
        SettingsCommand::Read { name, output } => {
            let v = if let Some(name) = name {
                serde_json::to_value(crate::settings::read_for_model(
                    session, id.slot, &name, model,
                )?)?
            } else {
                let mut snapshot = serde_json::to_value(crate::settings::read_all_for_model(
                    session, id.slot, model,
                ))?;
                snapshot["capabilities"] = crate::settings::capabilities_for_model(model);
                snapshot
            };
            if let Some(path) = output {
                use std::io::Write;
                let mut f = crate::storage::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)?;
                serde_json::to_writer_pretty(&mut f, &v)?;
                f.write_all(b"\n")?;
                f.sync_all()?;
            }
            Ok(v)
        }
        SettingsCommand::Set {
            name,
            value,
            backup,
        } => Ok(serde_json::to_value(crate::settings::set_for_model(
            session, id.slot, &name, &value, &backup, model,
        )?)?),
        SettingsCommand::Restore { file, backup } => Ok(serde_json::to_value(
            crate::settings::restore_for_model(session, id.slot, &file, &backup, model)?,
        )?),
        SettingsCommand::Batch {
            operations,
            backup_prefix,
            ..
        } => crate::settings::batch::run(session, id.slot, &operations, &backup_prefix, model),
        SettingsCommand::Schema { .. } => unreachable!(),
    }
}

fn start_rx_session(
    session: &mut Session,
    slot: Option<u8>,
    recovery: bool,
) -> Result<crate::device::Identity> {
    if recovery {
        session.normal_start_for_rx_recovery(slot)
    } else {
        session.normal_start(slot)
    }
}
fn parse_version(v: &str) -> Result<Version> {
    let p = v
        .split('.')
        .map(str::parse::<u8>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(p.len() == 4, "expected four version components");
    Ok(Version(p.try_into().unwrap()))
}
fn progress_reporter() -> impl FnMut(usize, usize) {
    let mut last = 0;
    move |done, total| {
        if done == total || done.saturating_sub(last) >= 8192 {
            err_println!("Programming {done}/{total} bytes ({}%)", done * 100 / total);
            last = done;
        }
    }
}

struct Journal(crate::storage::File);
impl Journal {
    fn new(path: &std::path::Path) -> Result<Self> {
        Ok(Self(
            crate::storage::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .context("journal must be a new file")?,
        ))
    }
    fn record(&mut self, stage: &str, data: &impl serde::Serialize) -> Result<()> {
        use std::io::Write;
        writeln!(
            self.0,
            "{}",
            serde_json::json!({"time_ms":crate::platform::now_ms(),"stage":stage,"data":data})
        )?;
        self.0.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod ble_tests {
    #[test]
    fn firmware_opt_in_keeps_read_only_recovery_serial_and_transport_guards() {
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (
                vec!["--port", "ble:fake", "--ble-firmware", "boot-probe"],
                "require --ble-expect-serial",
            ),
            (
                vec!["--port", "usb", "--ble-firmware", "boot-probe"],
                "requires a Bluetooth port",
            ),
            (
                vec![
                    "--port",
                    "ble:fake",
                    "--ble-firmware",
                    "boot-probe",
                    "--recovery",
                ],
                "BLE recovery is disabled",
            ),
            (
                vec![
                    "--port",
                    "ble:fake",
                    "--ble-firmware",
                    "--read-only",
                    "rx-probe",
                ],
                "--read-only rejects",
            ),
            (
                vec![
                    "--port",
                    "ble:fake",
                    "--ble-firmware",
                    "--ble-expect-serial",
                    "invalid",
                    "rx-probe",
                ],
                "12 hex digits",
            ),
        ];
        for (args, expected) in cases {
            let mut command = vec!["cranklink".into()];
            command.extend(args.into_iter().map(String::from));
            let error = execute(command).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
    #[test]
    fn batch_preflight_and_read_only_rejection_happen_before_transport_open() {
        let args = [
            "cranklink",
            "--port",
            "ble:fake",
            "--read-only",
            "settings",
            "batch",
            "missing.json",
            "--backup-prefix",
            "unused",
        ];
        assert!(execute(args.map(str::to_owned).to_vec())
            .unwrap_err()
            .to_string()
            .contains("--read-only rejects"));
        let path = std::env::temp_dir().join(format!(
            "cranklink-invalid-plan-{}-{}.json",
            std::process::id(),
            crate::platform::now_ms()
        ));
        std::fs::write(&path, r#"[{"op":"set","name":"lamp","value":true}]"#).unwrap();
        let args = vec![
            "cranklink".into(),
            "--port".into(),
            "ble:fake".into(),
            "settings".into(),
            "batch".into(),
            path.display().to_string(),
            "--backup-prefix".into(),
            "unused".into(),
        ];
        let error = execute(args).unwrap_err();
        assert!(error.to_string().contains("batch plan must be"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn master_routing_option_permits_read_only_validation_and_requires_ble() {
        let cli = super::Cli::try_parse_from([
            "cranklink",
            "--port",
            "ble:fake",
            "--ble-master",
            "--read-only",
            "probe",
        ])
        .unwrap();
        assert!(cli.ble_master && cli.read_only);
        let error = super::execute(vec![
            "cranklink".into(),
            "--ble-master".into(),
            "probe".into(),
        ])
        .unwrap_err();
        assert!(error.to_string().contains("requires a Bluetooth port"));
    }
    use super::*;
    #[test]
    fn dangerous_commands_refuse_ble_before_opening_files_or_radio() {
        for port in ["ble:fake", "webble"] {
            for args in [
                vec!["boot-probe"],
                vec!["boot-probe", "--recovery", "--reset"],
                vec!["rx-probe"],
                vec!["power-cycle"],
                vec![
                    "flash-dcas",
                    "missing.dat",
                    "--companion",
                    "missing-m.dat",
                    "--expect-serial",
                    "000000000000",
                    "--expect-sha256",
                    "bad",
                    "--journal",
                    "must-not-create.jsonl",
                    "--write",
                ],
                vec![
                    "flash-rx",
                    "missing.dat",
                    "--companion",
                    "missing-d.dat",
                    "--expect-serial",
                    "000000000000",
                    "--expect-sha256",
                    "bad",
                    "--journal",
                    "must-not-create.jsonl",
                ],
            ] {
                let mut cmd = vec!["cranklink".to_owned(), "--port".into(), port.into()];
                cmd.extend(args.into_iter().map(str::to_owned));
                let err = execute(cmd).unwrap_err().to_string();
                assert!(
                    err.contains("Command unavailable in a BLE settings session")
                        && err.contains("requires --ble-firmware"),
                    "{err}"
                );
            }
        }
    }
    #[test]
    fn read_only_rejects_mutating_commands_before_opening_transport_or_files() {
        for port in ["usb", "ble:fake"] {
            for args in [
                vec!["boot-probe"],
                vec!["rx-probe"],
                vec!["power-cycle"],
                vec![
                    "settings",
                    "set",
                    "lamp",
                    "1",
                    "--backup",
                    "must-not-create.json",
                ],
                vec![
                    "settings",
                    "restore",
                    "missing.json",
                    "--backup",
                    "must-not-create.json",
                ],
                vec!["ble", "unit-authenticate"],
                vec![
                    "flash-dcas",
                    "missing.dat",
                    "--companion",
                    "missing-m.dat",
                    "--expect-serial",
                    "000000000000",
                    "--expect-sha256",
                    "bad",
                    "--journal",
                    "must-not-create.jsonl",
                ],
                vec![
                    "flash-rx",
                    "missing.dat",
                    "--companion",
                    "missing-d.dat",
                    "--expect-serial",
                    "000000000000",
                    "--expect-sha256",
                    "bad",
                    "--journal",
                    "must-not-create.jsonl",
                    "--write",
                ],
                vec![
                    "--ble-unit-auth",
                    "--ble-expect-serial",
                    "000000000000",
                    "probe",
                ],
            ] {
                let mut command = vec!["cranklink", "--read-only", "--port", port];
                command.extend(args);
                let error = execute(command.into_iter().map(str::to_owned).collect()).unwrap_err();
                assert!(
                    error.to_string().contains("--read-only rejects"),
                    "{error:#}"
                );
            }
        }
        for args in [
            vec!["units"],
            vec!["probe"],
            vec!["settings", "read"],
            vec!["ble", "units"],
        ] {
            let mut command = vec!["cranklink", "--read-only"];
            command.extend(args);
            let cli = Cli::try_parse_from(command).unwrap();
            assert!(read_only_command_allowed(&cli.command));
            assert!(ble_command_allowed(&cli.command));
        }
    }
    #[test]
    fn wired_serial_guard_rejects_bad_options_before_open_and_wrong_motor() {
        assert!(check_motor_serial("a1b2c3d4e5f6", Some("A1B2C3D4E5F6")).is_ok());
        assert!(check_motor_serial("a1b2c3d4e5f6", Some("010203040506")).is_err());
        for args in [
            vec!["--expect-motor-serial", "bad", "probe"],
            vec!["--expect-motor-serial", "010203040506", "units"],
            vec![
                "--expect-motor-serial",
                "010203040506",
                "--port",
                "ble:fake",
                "probe",
            ],
        ] {
            let mut command = vec!["cranklink"];
            command.extend(args);
            assert!(execute(command.into_iter().map(str::to_owned).collect())
                .unwrap_err()
                .to_string()
                .contains("--expect-motor-serial"));
        }
    }
    #[test]
    fn failed_read_preserves_cleanup_failure_in_diagnostics() {
        let error = finish_operation::<()>(
            Err(anyhow::anyhow!("read failed")),
            Err(anyhow::anyhow!("stop not confirmed")),
        )
        .unwrap_err();
        let text = format!("{error:#}");
        assert!(text.contains("read failed") && text.contains("stop not confirmed"));
        assert!(finish_operation(Ok(()), Err(anyhow::anyhow!("stop failed"))).is_err());
    }
    #[test]
    fn wired_unit_auth_rejects_invalid_options_before_opening_transport() {
        for args in [
            vec!["--unit-auth", "probe"],
            vec![
                "--read-only",
                "--unit-auth",
                "--expect-motor-serial",
                "010203040506",
                "probe",
            ],
            vec![
                "--unit-auth",
                "--expect-motor-serial",
                "010203040506",
                "--port",
                "ble:fake",
                "probe",
            ],
            vec![
                "--unit-auth",
                "--expect-motor-serial",
                "010203040506",
                "units",
            ],
        ] {
            let mut cmd = vec!["cranklink".to_owned()];
            cmd.extend(args.into_iter().map(str::to_owned));
            assert!(execute(cmd).is_err());
        }
        assert!(Cli::try_parse_from([
            "cranklink",
            "--unit-auth",
            "--expect-motor-serial",
            "010203040506",
            "settings",
            "set",
            "destination",
            "1",
            "--backup",
            "new.json"
        ])
        .is_ok());
    }
    #[test]
    fn scan_duration_is_bounded_by_parser() {
        for n in ["0", "31", "999999"] {
            assert!(Cli::try_parse_from(["cranklink", "ble", "scan", "--seconds", n]).is_err());
        }
    }
    #[test]
    fn unit_auth_identity_guards_fail_before_credentials_or_radio() {
        assert!(Cli::try_parse_from([
            "cranklink",
            "--port",
            "ble:fake",
            "--ble-unit-auth",
            "probe"
        ])
        .is_err());
        for (args, expected) in [
            (
                vec!["--port", "ble:fake", "ble", "unit-authenticate"],
                "requires --ble-expect-serial",
            ),
            (
                vec![
                    "--port",
                    "ble:fake",
                    "--ble-expect-serial",
                    "bad",
                    "--ble-passkey-file",
                    "does-not-exist",
                    "probe",
                ],
                "exactly 12 hexadecimal",
            ),
            (
                vec![
                    "--port",
                    "COM-do-not-open",
                    "--ble-expect-serial",
                    "000000000000",
                    "probe",
                ],
                "requires a Bluetooth port",
            ),
        ] {
            let mut cmd = vec!["cranklink".to_owned()];
            cmd.extend(args.into_iter().map(str::to_owned));
            let error = execute(cmd).unwrap_err().to_string();
            assert!(error.contains(expected), "{error}");
        }
    }
}
