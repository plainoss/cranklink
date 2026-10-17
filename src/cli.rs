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

pub mod session;

use crate::{
    device::Session,
    firmware::{Firmware, FirmwareKind, Version},
};
use anyhow::{ensure, Context, Result};
use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
#[value(rename_all = "kebab-case")]
pub enum OutputFormat {
    #[default]
    Auto,
    Text,
    Json,
}

impl OutputFormat {
    pub fn is_json(self) -> bool {
        match self {
            OutputFormat::Json => true,
            OutputFormat::Text => false,
            OutputFormat::Auto => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use std::io::IsTerminal;
                    !std::io::stdout().is_terminal()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    false
                }
            }
        }
    }
}

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Yellow.on_default());

const EXAMPLES: &str = "\
EXAMPLES:
  # Instant discovery of attached USB adapters (<5ms) or active Bluetooth scan:
  cranklink scan
  cranklink scan --ble

  # Probe motor identity and both MCU firmware versions:
  cranklink --port usb probe

  # Read or modify settings (auto-authenticates OEM fields and saves pre-write backup):
  cranklink -s 010203040506 settings read destination
  cranklink -s 010203040506 settings set destination 1
  cranklink -s 010203040506 settings set profile1.boost.torque 85 --dry-run

  # Offline firmware inspection and bootloader recovery:
  cranklink firmware inspect DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat
  cranklink firmware probe --target dcas --recovery
  cranklink -s 010203040506 firmware flash dcas DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat --recovery --write

  # Start persistent JSON-RPC driver session for scripts or AI agents:
  cranklink session --json
";

#[derive(Parser)]
#[command(
    name = "cranklink",
    version,
    about = "Shimano STEPS e-bike diagnostic, recovery, and configuration CLI",
    long_about = "Shimano STEPS e-bike diagnostic, recovery, and configuration CLI with BLE & USB/serial support.\nSafe by default: all firmware flashing requires explicit --write to program; pre-write backups are durable and automatic.",
    styles = STYLES,
    after_help = EXAMPLES
)]
struct Cli {
    // --- Connection & Transport ---
    #[arg(
        long,
        short = 'p',
        global = true,
        help_heading = "Connection & Transport",
        help = "Adapter (default: auto); usb, COM3, ble:DEVICE_ID, /dev/ttyUSB0"
    )]
    port: Option<String>,

    #[arg(
        long,
        global = true,
        help_heading = "Connection & Transport",
        help = "Bus slot override or recovery boot address"
    )]
    slot: Option<u8>,

    #[arg(
        long,
        global = true,
        help_heading = "Connection & Transport",
        help = "Log raw transport frames as JSON lines"
    )]
    trace: Option<PathBuf>,

    // --- Target Verification ---
    #[arg(
        long,
        short = 's',
        visible_aliases = ["expect-motor-serial", "ble-expect-serial"],
        global = true,
        help_heading = "Target Verification",
        help = "Require this 12-hex-digit motor serial (auto-sanitized from colons/hyphens)"
    )]
    expect_serial: Option<String>,

    #[arg(
        long,
        short = 'c',
        global = true,
        help_heading = "Target Verification",
        help = "Target component model filter (e.g. DU-EP800-CRG)",
        conflicts_with = "slot"
    )]
    component: Option<String>,

    // --- Authentication ---
    #[arg(
        long,
        global = true,
        help_heading = "Authentication",
        help = "6-digit Bluetooth passkey (or set CRANKLINK_PASSKEY)"
    )]
    passkey: Option<String>,

    #[arg(
        long,
        global = true,
        help_heading = "Authentication",
        help = "Read 6-digit Bluetooth passkey from file"
    )]
    passkey_file: Option<PathBuf>,

    #[arg(
        long,
        visible_alias = "ble-unit-auth",
        global = true,
        help_heading = "Authentication",
        help = "Explicitly authenticate with the drive-unit (e.g. for modern EP801/EP600 BLE)"
    )]
    unit_auth: bool,

    #[arg(
        long,
        global = true,
        help_heading = "Authentication",
        help = "Forbid automatic drive-unit authentication on OEM writes"
    )]
    no_auth: bool,

    // --- Safety ---
    #[arg(
        long,
        global = true,
        help_heading = "Safety",
        help = "Reject all writes, bootloader entries, and motor resets"
    )]
    read_only: bool,

    #[arg(
        long,
        global = true,
        help_heading = "Safety",
        help = "Use stock gateway master routing (experimental)"
    )]
    ble_master: bool,

    // --- Display & Format ---
    #[arg(
        long,
        global = true,
        help_heading = "Display & Output",
        help = "Force machine-readable JSON output on stdout"
    )]
    json: bool,

    #[arg(
        long,
        global = true,
        help_heading = "Display & Output",
        default_value = "auto",
        value_enum,
        help = "Output presentation format"
    )]
    format: OutputFormat,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for attached wired adapters (<5ms) and nearby Bluetooth gateways (--ble)
    Scan {
        /// Also scan for nearby Bluetooth LE E-TUBE gateways (runs active radio scan)
        #[arg(long)]
        ble: bool,
        /// Duration of Bluetooth scan in seconds (1-30)
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=30))]
        seconds: u64,
        /// Include non-Shimano Bluetooth advertisements during --ble scan
        #[arg(long)]
        all: bool,
    },
    /// Inventory connected E-TUBE bus components (motor, display, shifter)
    Units,
    /// Probe active drive unit (model, serial, MCU firmware versions)
    Probe,
    /// Persistent interactive JSON-RPC session driver for AI agents & test runners
    Session,
    /// Read, export, and configure motor settings
    Settings {
        #[command(subcommand)]
        command: SettingsCommand,
    },
    /// Inspect, verify, probe, and flash motor firmware
    Firmware {
        #[command(subcommand)]
        command: FirmwareCommand,
    },
    /// Hardware adapter diagnostics (SM-PCE02)
    Adapter {
        #[command(subcommand)]
        command: AdapterCommand,
    },
    /// Bluetooth gateway operations (scan, link testing)
    Ble {
        #[command(subcommand)]
        command: BleCommand,
    },
}

#[derive(Subcommand)]
enum SettingsCommand {
    /// List setting names, units, accepted ranges, and OEM flags
    Schema {
        /// Specific setting name to inspect (optional)
        name: Option<String>,
        /// Drive unit model schema
        #[arg(long, default_value = "ep800-crg", value_parser = ["ep800-crg", "ep600", "ep801"])]
        model: String,
    },
    /// Read one setting, or every supported setting when no name is given
    Read {
        /// Name of specific setting to read
        name: Option<String>,
        /// Export full settings snapshot to a JSON file
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Change a setting with durable pre-write backup and readback verification
    Set {
        /// Name of setting to write
        name: String,
        /// New value
        value: String,
        /// Pre-write backup path (auto-generated if omitted)
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Validate schema, bounds, and permissions without writing to EEPROM
        #[arg(long)]
        dry_run: bool,
    },
    /// Restore one setting from a serial-bound backup
    Restore {
        /// Backup file to restore
        file: PathBuf,
        /// Pre-restore backup path (auto-generated if omitted)
        #[arg(long)]
        backup: Option<PathBuf>,
    },
    /// Run an ordered JSON plan in one session, backing up and verifying each write
    Batch {
        /// Path to JSON batch plan
        plan: PathBuf,
        /// Prefix for journal and numbered backups (default: batch-TIMESTAMP)
        #[arg(long)]
        backup_prefix: Option<PathBuf>,
        #[arg(skip)]
        operations: Vec<crate::settings::batch::Operation>,
    },
}

#[derive(Subcommand)]
enum FirmwareCommand {
    /// Decode and validate firmware container file offline
    Inspect {
        /// Firmware container file (.dat)
        file: PathBuf,
        /// Companion firmware container file (.dat) to verify pair compatibility
        #[arg(long)]
        companion: Option<PathBuf>,
        /// Extract decrypted raw binary payload to file
        #[arg(long)]
        decode_to: Option<PathBuf>,
    },
    /// Probe microcontroller bootloader state
    Probe {
        /// Target microcontroller bootloader
        #[arg(long, value_parser = ["dcas", "rx"])]
        target: String,
        /// Probe while in recovery mode without application return
        #[arg(long)]
        recovery: bool,
        /// Explicitly reset an already-recovering motor into its application
        #[arg(long, requires = "recovery")]
        reset: bool,
    },
    /// Program guarded firmware image (dry-run by default; requires --write to commit)
    Flash {
        #[command(subcommand)]
        command: FlashCommand,
    },
}

#[derive(Subcommand)]
enum FlashCommand {
    /// Program D / DCASX microcontroller image over wired connection
    Dcas {
        /// Path to DCAS container file (.dat)
        file: PathBuf,
        /// Required matching RX companion container file (.dat)
        #[arg(long)]
        companion: PathBuf,
        /// Optional assertion for expected file SHA-256 (auto-computed by default)
        #[arg(long)]
        expect_sha256: Option<String>,
        /// Transaction journal path (auto-generated if omitted)
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Recover from an already-interrupted bootloader state
        #[arg(long)]
        recovery: bool,
        /// Commit destructive erase and programming (default: safe dry-run)
        #[arg(long)]
        write: bool,
    },
    /// Program M / Renesas RX microcontroller image over wired connection
    Rx {
        /// Path to Renesas RX container file (.dat)
        file: PathBuf,
        /// Required matching DCAS companion container file (.dat)
        #[arg(long)]
        companion: PathBuf,
        /// Optional assertion for expected file SHA-256 (auto-computed by default)
        #[arg(long)]
        expect_sha256: Option<String>,
        /// Transaction journal path (auto-generated if omitted)
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Recover RX through an intact D application
        #[arg(long)]
        recovery: bool,
        /// Commit destructive erase and programming (default: safe dry-run)
        #[arg(long)]
        write: bool,
    },
    /// Program dual-MCU D+M in one serial-bound transaction over BLE relay (BLE only)
    BlePair {
        /// Path to DCAS container file (.dat)
        #[arg(long)]
        dcas: PathBuf,
        /// Path to Renesas RX container file (.dat)
        #[arg(long)]
        rx: PathBuf,
        /// Optional assertion for DCAS file SHA-256 (auto-computed by default)
        #[arg(long)]
        expect_dcas_sha256: Option<String>,
        /// Optional assertion for RX file SHA-256 (auto-computed by default)
        #[arg(long)]
        expect_rx_sha256: Option<String>,
        /// Transaction journal path (auto-generated if omitted)
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Full settings backup path (auto-generated if omitted)
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Commit destructive programming (default: safe dry-run)
        #[arg(long)]
        write: bool,
    },
}

#[derive(Subcommand)]
enum AdapterCommand {
    /// Cycle adapter-supplied motor power and verify application return (SM-PCE02 bench only)
    PowerCycle,
    /// Query SM-PCE02 adapter power supply status and connected voltage
    Status,
}

#[derive(Subcommand)]
enum BleCommand {
    /// Scan for nearby Shimano E-TUBE Bluetooth gateways
    Scan {
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=30))]
        seconds: u64,
        #[arg(long)]
        all: bool,
    },
    /// Authenticate with Bluetooth gateway and verify radio link
    TestLink,
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

    // Environment variable overrides
    if cli.port.is_none() {
        if let Ok(p) = std::env::var("CRANKLINK_PORT") {
            if !p.trim().is_empty() {
                cli.port = Some(p.trim().to_string());
            }
        }
    }
    if cli.expect_serial.is_none() {
        if let Ok(s) = std::env::var("CRANKLINK_SERIAL") {
            if !s.trim().is_empty() {
                cli.expect_serial = Some(s.trim().to_string());
            }
        }
    }
    if cli.passkey.is_none() && cli.passkey_file.is_none() {
        if let Ok(pk) = std::env::var("CRANKLINK_PASSKEY") {
            if !pk.trim().is_empty() {
                cli.passkey = Some(pk.trim().to_string());
            }
        }
    }
    let mut format = cli.format;
    if cli.json {
        format = OutputFormat::Json;
    } else if let Ok(f) = std::env::var("CRANKLINK_FORMAT") {
        if f.eq_ignore_ascii_case("json") {
            format = OutputFormat::Json;
        } else if f.eq_ignore_ascii_case("text") {
            format = OutputFormat::Text;
        }
    }
    let is_json = format.is_json();

    // Sanitize serial number if provided
    if let Some(ser) = &cli.expect_serial {
        cli.expect_serial = Some(sanitize_serial(ser)?);
    }

    // Safety check: read-only rejection
    ensure!(
        !cli.read_only || read_only_command_allowed(&cli.command),
        "--read-only rejects mutating commands (settings set/restore, firmware flash, power-cycle)"
    );

    // Component validation
    if let Some(component) = &cli.component {
        ensure!(
            !component.trim().is_empty()
                && matches!(
                    cli.command,
                    Command::Probe
                        | Command::Settings {
                            command: SettingsCommand::Read { .. } | SettingsCommand::Batch { .. }
                        }
                ),
            "--component applies only to probe, settings read or batch"
        );
    }

    // Load batch plan if applicable
    if let Command::Settings {
        command:
            SettingsCommand::Batch {
                plan,
                operations,
                backup_prefix,
            },
    } = &mut cli.command
    {
        let prefix = backup_prefix.get_or_insert_with(|| {
            PathBuf::from(format!("batch-{}", crate::platform::now_ms()))
        });
        ensure!(
            !prefix.as_os_str().is_empty(),
            "backup prefix must not be empty"
        );
        *operations = crate::settings::batch::load(plan)?;
    }

    // BLE dispatch vs native wired dispatch
    if cli.port.as_deref().is_some_and(crate::ble::is_port)
        && !matches!(
            cli.command,
            Command::Firmware {
                command: FirmwareCommand::Inspect { .. }
            }
        )
    {
        return execute_ble(cli, is_json);
    }

    // Require BLE port for BLE-only operations
    ensure!(
        cli.passkey.is_none() && cli.passkey_file.is_none(),
        "--passkey and --passkey-file require a Bluetooth port (--port ble:DEVICE_ID)"
    );
    ensure!(
        !cli.ble_master,
        "--ble-master requires a Bluetooth port (--port ble:DEVICE_ID)"
    );

    let port = cli.port;
    let slot = cli.slot;
    let trace = cli.trace;
    let expect_serial = cli.expect_serial;
    let component = cli.component;
    let no_auth = cli.no_auth;
    let unit_auth = cli.unit_auth;
    let json = is_json;

    match cli.command {
        Command::Session => {
            session::run_session(port, trace, expect_serial)
        }
        #[allow(unused_variables)]
        Command::Scan { ble, seconds, all } => {
            let ports = crate::platform::available_ports()?;
            let ports_json: Vec<_> = ports
                .iter()
                .map(|p| serde_json::json!({ "name": p.port_name, "type": p.port_type }))
                .collect();
            if !ble {
                if json {
                    out_println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({ "ports": ports_json }))?
                    );
                } else {
                    out_println!("Attached Adapters ({} found):", ports.len());
                    for p in &ports {
                        out_println!("  {:<20} {}", p.port_name, p.port_type);
                    }
                    out_println!(
                        "\nTip: Run 'cranklink scan --ble' to discover wireless Bluetooth gateways."
                    );
                }
                return Ok(());
            }

            #[cfg(feature = "ble")]
            {
                notice_println!("Scanning for Bluetooth LE gateways ({}s)...", seconds);
                let ble_devices = crate::ble::scan(seconds, all)?;
                if json {
                    out_println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ports": ports_json,
                            "ble": ble_devices,
                        }))?
                    );
                } else {
                    out_println!("Attached Adapters ({} found):", ports.len());
                    for p in &ports {
                        out_println!("  {:<20} {}", p.port_name, p.port_type);
                    }
                    out_println!("\nBluetooth Gateways ({} found):", ble_devices.len());
                    for d in &ble_devices {
                        out_println!("  {:<20} {} (RSSI: {} dBm)", d.id, d.name, d.rssi);
                    }
                }
                Ok(())
            }
            #[cfg(not(feature = "ble"))]
            {
                anyhow::bail!("Bluetooth discovery requires cranklink built with --features ble");
            }
        }
        Command::Units => {
            let mut session =
                Session::open(port.as_deref().unwrap_or("auto"), trace.as_deref())?;
            let power = session.initialize()?;
            err_println!("Adapter ready: {}", serde_json::to_string(&power)?);
            let units = session.units()?;
            if is_json {
                out_println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ "units": units }))?
                );
            } else {
                print_units_table(&units);
            }
            Ok(())
        }
        Command::Probe => {
            let mut session =
                Session::open(port.as_deref().unwrap_or("auto"), trace.as_deref())?;
            let power = session.initialize()?;
            err_println!("Adapter ready: {}", serde_json::to_string(&power)?);
            let result = session
                .probe_component(slot, component.as_deref())
                .and_then(|identity| {
                    check_motor_serial(&identity.serial, expect_serial.as_deref())?;
                    Ok(identity)
                });
            let stopped = session.normal_stop();
            let identity =
                finish_operation(result, stopped.context("PC-link Stop acknowledgement"))?;
            out_println!("{}", serde_json::to_string_pretty(&identity)?);
            Ok(())
        }
        Command::Settings {
            command: SettingsCommand::Schema { name, model },
        } => {
            let model_enum = settings_model_name(&model);
            let specs = crate::settings::schema_for_model(model_enum);
            if let Some(target) = name {
                let spec = specs
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(&target))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "unknown setting '{target}' for model {}",
                            model_enum.name()
                        )
                    })?;
                out_println!("{}", serde_json::to_string_pretty(&spec)?);
            } else {
                out_println!("{}", serde_json::to_string_pretty(&specs)?);
            }
            Ok(())
        }
        Command::Settings { command } => {
            let mut s = Session::open(port.as_deref().unwrap_or("auto"), trace.as_deref())?;
            s.initialize()?;
            let result = s
                .normal_start_component(slot, component.as_deref())
                .and_then(|id| {
                    let serial_raw = s.command(id.slot, 1, 0x3c, &[0], 0x3e)?;
                    let motor_serial = hex::encode(crate::device::serial_bytes(&serial_raw)?);
                    check_motor_serial(&motor_serial, expect_serial.as_deref())?;
                    err_println!("Motor ready: {}", serde_json::to_string(&id)?);

                    // Check if setting write targets an OEM field requiring authentication
                    let model = resolve_settings_model(&mut s, &id)?;
                    let is_oem = unit_auth || match &command {
                        SettingsCommand::Set { name, .. } => {
                            let specs = crate::settings::schema_for_model(model);
                            specs.iter().any(|spec| spec.name.eq_ignore_ascii_case(name) && spec.oem)
                        }
                        SettingsCommand::Batch { operations, .. } => {
                            let specs = crate::settings::schema_for_model(model);
                            operations.iter().any(|op| {
                                if let crate::settings::batch::Operation::Set { name, .. } = op {
                                    specs.iter().any(|spec| spec.name.eq_ignore_ascii_case(name) && spec.oem)
                                } else {
                                    false
                                }
                            })
                        }
                        _ => false,
                    };

                    if is_oem {
                        ensure!(!no_auth, "setting requires drive-unit authentication, but --no-auth was specified");
                        let receipt = s.authenticate_unit_settings(&motor_serial)?;
                        err_println!("Drive-unit authentication: {}", receipt);
                    }

                    execute_settings(&mut s, &id, command)
                });
            let stopped = s.normal_stop();
            let value = finish_operation(result, stopped.context("PC-link Stop acknowledgement"))?;
            out_println!("{}", serde_json::to_string_pretty(&value)?);
            Ok(())
        }
        Command::Firmware { command } => execute_firmware_command(port.as_deref(), trace.as_deref(), slot, expect_serial.as_deref(), command),
        Command::Adapter { command } => execute_adapter_command(port.as_deref(), trace.as_deref(), slot, command),
        Command::Ble { .. } => {
            anyhow::bail!("Bluetooth commands require selecting a Bluetooth port (--port ble:DEVICE_ID)")
        }
    }
}

fn execute_firmware_command(
    port: Option<&str>,
    trace: Option<&std::path::Path>,
    slot: Option<u8>,
    expect_serial: Option<&str>,
    command: FirmwareCommand,
) -> Result<()> {
    match command {
        FirmwareCommand::Inspect {
            file,
            companion,
            decode_to,
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
            Ok(())
        }
        FirmwareCommand::Probe {
            target,
            recovery,
            reset,
        } => {
            let mut s = Session::open(port.unwrap_or("auto"), trace)?;
            s.initialize()?;
            match target.to_lowercase().as_str() {
                "dcas" => {
                    if !recovery {
                        let i = s.normal_start(slot)?;
                        err_println!("Application: {}", serde_json::to_string(&i)?);
                    }
                    let identity = s.boot_start_at(!recovery, slot)?;
                    out_println!("{}", serde_json::to_string_pretty(&identity)?);
                    if !recovery || reset {
                        s.boot_reset()?;
                        err_println!("Application reset acknowledged");
                    }
                }
                "rx" => {
                    let before = start_rx_session(&mut s, slot, recovery)?;
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
                other => anyhow::bail!("unknown probe target '{other}'; supported targets are 'dcas' and 'rx'"),
            }
            Ok(())
        }
        FirmwareCommand::Flash { command } => match command {
            FlashCommand::Dcas {
                file,
                companion,
                expect_sha256,
                journal,
                recovery,
                write,
            } => {
                let fw = Firmware::load(&file)?;
                ensure!(
                    fw.metadata.kind == FirmwareKind::DcasX,
                    "flash dcas requires a D image"
                );
                let comp = Firmware::load(&companion)?;
                fw.validate_companion(&comp)?;
                if let Some(expected) = &expect_sha256 {
                    ensure!(
                        fw.metadata.source_sha256.eq_ignore_ascii_case(expected),
                        "source SHA-256 does not match --expect-sha256"
                    );
                }

                let serial_str = expect_serial
                    .ok_or_else(|| anyhow::anyhow!("flashing requires motor serial (--expect-serial <12-HEX>)"))?;
                let serial = hex::decode(serial_str)?;
                ensure!(serial.len() == 6, "expected six-byte serial");

                let journal_path = journal.unwrap_or_else(|| {
                    PathBuf::from(format!("cranklink-flash-dcas-{}-{}.journal.jsonl", serial_str, crate::platform::now_ms()))
                });
                let mut jnl = Journal::new(&journal_path)?;
                jnl.record("inputs", &fw.metadata)?;

                let mut s = Session::open(port.unwrap_or("auto"), trace)?;
                s.initialize()?;
                let before = if recovery {
                    None
                } else {
                    Some(s.normal_start(slot)?)
                };
                if let Some(i) = &before {
                    jnl.record("application-before", i)?;
                    fw.validate_companion_version(parse_version(&i.rx_version)?)?;
                }
                let boot = s.boot_start_at(!recovery, slot)?;
                jnl.record("boot-identity", &boot)?;
                let guard = (|| {
                    ensure!(
                        boot.serial.as_slice() == serial.as_slice(),
                        "live serial differs from --expect-serial"
                    );
                    fw.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))
                })();
                if let Err(e) = guard {
                    jnl.record("guard-rejected", &format!("{e:#}"))?;
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
                    jnl.record("dry-run-complete", &boot)?;
                    out_println!("\n[DRY RUN - VALIDATION ONLY]");
                    out_println!("- Target Bootloader: Verified (DCASX v{}.0.0.0, Board {})", boot.version[0], boot.board);
                    out_println!("- Motor Serial: Verified ({})", serial_str);
                    out_println!("- Source SHA-256: {}", fw.metadata.source_sha256);
                    out_println!("- Flash State: UNMODIFIED (No bytes written)");
                    out_println!(">> To execute live flash programming, add the --write flag.\n");
                    return Ok(());
                }

                jnl.record("destructive-start", &fw.metadata)?;
                let receipt = match s.flash_dcas(&fw.data, &boot, progress_reporter()) {
                    Ok(r) => r,
                    Err(e) => {
                        jnl.record("uncertain", &format!("{e:#}"))?;
                        err_println!("\n================================================================================");
                        err_println!("CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.");
                        err_println!("DO NOT POWER CYCLE THE MOTOR.");
                        err_println!("Journal preserved at: {}", journal_path.display());
                        err_println!("To recover, re-run with:");
                        err_println!("  cranklink -s {} firmware flash dcas {} --companion {} --recovery --write",
                            serial_str, file.display(), companion.display());
                        err_println!("================================================================================\n");
                        return Err(e);
                    }
                };
                jnl.record("finish-and-reset", &receipt)?;
                let after = s.normal_start(if recovery { None } else { slot })?;
                let stop = s.normal_stop();
                jnl.record("application-after", &after)?;
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
                jnl.record("verified", &after)?;
                out_println!("{}", serde_json::to_string_pretty(&after)?);
                Ok(())
            }
            FlashCommand::Rx {
                file,
                companion,
                expect_sha256,
                journal,
                recovery,
                write,
            } => {
                let fw = Firmware::load(&file)?;
                let d = Firmware::load(&companion)?;
                ensure!(
                    fw.metadata.kind == FirmwareKind::Renesas,
                    "flash rx requires an M image"
                );
                fw.validate_companion(&d)?;
                if let Some(expected) = &expect_sha256 {
                    ensure!(
                        fw.metadata.source_sha256.eq_ignore_ascii_case(expected),
                        "source SHA-256 differs from --expect-sha256"
                    );
                }
                let serial_str = expect_serial
                    .ok_or_else(|| anyhow::anyhow!("flashing requires motor serial (--expect-serial <12-HEX>)"))?;
                let serial = hex::decode(serial_str)?;
                ensure!(serial.len() == 6, "expected six-byte serial");

                let journal_path = journal.unwrap_or_else(|| {
                    PathBuf::from(format!("cranklink-flash-rx-{}-{}.journal.jsonl", serial_str, crate::platform::now_ms()))
                });
                let mut jnl = Journal::new(&journal_path)?;
                jnl.record("inputs", &fw.metadata)?;

                let mut s = Session::open(port.unwrap_or("auto"), trace)?;
                s.initialize()?;
                let before = start_rx_session(&mut s, slot, recovery)?;
                if !recovery {
                    ensure!(
                        before.dcas_version == d.metadata.version.to_string(),
                        "live D version differs from supplied companion; flash D first"
                    );
                }
                jnl.record("application-before", &before)?;
                let boot = s.boot_start(true)?;
                jnl.record("dcas-identity", &boot)?;
                let guard = (|| {
                    ensure!(
                        boot.serial.as_slice() == serial.as_slice(),
                        "live serial differs from expected"
                    );
                    d.validate_target(boot.series, boot.unit, boot.board, Version(boot.version))
                })();
                s.boot_reset()?;
                guard?;
                let ready = start_rx_session(&mut s, Some(before.slot), recovery)?;

                if !write {
                    let rx = crate::rx::read_boot(&mut s, ready.slot)?;
                    jnl.record("rx-boot", &rx)?;
                    crate::rx::reset_fast(&mut s, before.slot)?;
                    let after = start_rx_session(&mut s, Some(before.slot), recovery)?;
                    s.normal_stop()?;
                    jnl.record("dry-run-complete", &after)?;
                    out_println!("\n[DRY RUN - VALIDATION ONLY]");
                    out_println!("- RX Bootloader: Verified (Sequence {})", rx.capability_sequence);
                    out_println!("- Motor Serial: Verified ({})", serial_str);
                    out_println!("- Source SHA-256: {}", fw.metadata.source_sha256);
                    out_println!("- Flash State: UNMODIFIED (No bytes written)");
                    out_println!(">> To execute live flash programming, add the --write flag.\n");
                    return Ok(());
                }

                jnl.record("destructive-start", &fw.metadata)?;
                let receipt = match crate::rx::flash(&mut s, ready.slot, &fw, progress_reporter()) {
                    Ok(r) => r,
                    Err(e) => {
                        jnl.record("uncertain", &format!("{e:#}"))?;
                        err_println!("\n================================================================================");
                        err_println!("CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.");
                        err_println!("DO NOT POWER CYCLE THE MOTOR.");
                        err_println!("Journal preserved at: {}", journal_path.display());
                        err_println!("To recover, re-run with:");
                        err_println!("  cranklink -s {} firmware flash rx {} --companion {} --recovery --write",
                            serial_str, file.display(), companion.display());
                        err_println!("================================================================================\n");
                        return Err(e);
                    }
                };
                jnl.record("finish-and-reset", &receipt)?;
                let after = s.normal_start(Some(before.slot))?;
                s.normal_stop()?;
                jnl.record("application-after", &after)?;
                ensure!(
                    after.cargo == before.cargo && after.dcas_version == d.metadata.version.to_string(),
                    "cargo or D version changed after RX update"
                );
                ensure!(
                    after.rx_version == fw.metadata.version.to_string(),
                    "post-flash RX version mismatch"
                );
                jnl.record("verified", &after)?;
                out_println!("{}", serde_json::to_string_pretty(&after)?);
                Ok(())
            }
            FlashCommand::BlePair { .. } => {
                anyhow::bail!("flash ble-pair requires a Bluetooth port (--port ble:DEVICE_ID)")
            }
        },
    }
}

fn execute_adapter_command(
    port: Option<&str>,
    trace: Option<&std::path::Path>,
    slot: Option<u8>,
    command: AdapterCommand,
) -> Result<()> {
    match command {
        AdapterCommand::Status => {
            let mut s = Session::open(port.unwrap_or("auto"), trace)?;
            let power = s.initialize()?;
            out_println!("{}", serde_json::to_string_pretty(&power)?);
            Ok(())
        }
        AdapterCommand::PowerCycle => {
            use crate::platform::sleep;
            use std::time::Duration;
            let mut s = Session::open(port.unwrap_or("auto"), trace)?;
            let power = s.initialize()?;
            ensure!(
                power.adapter_supply,
                "power-cycle requires an adapter-supplied motor on bench without an external battery"
            );
            let before = s.normal_start(slot)?;
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
            Ok(())
        }
    }
}

fn execute_ble(cli: Cli, is_json: bool) -> Result<()> {
    if matches!(
        cli.command,
        Command::Firmware {
            command: FirmwareCommand::Flash {
                command: FlashCommand::BlePair { .. }
            }
        }
    ) {
        return execute_ble_pair(cli);
    }

    ensure!(
        ble_command_allowed(&cli.command),
        "Command unavailable in a BLE settings session. Use wired connection for bootloader probes and recovery"
    );

    let passkey = read_ble_passkey(&cli)?;
    notice_println!("Experimental Bluetooth transport: hardware validation pending.");
    let mut session =
        crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?;
    session.set_master_routing(cli.ble_master)?;
    let result = (|| -> Result<Option<serde_json::Value>> {
        session.authenticate()?;
        match cli.command {
            Command::Ble {
                command: BleCommand::TestLink,
            } => Ok(Some(
                serde_json::json!({"authenticated":true,"protocol_reply_verified":true,"hardware_validated":false}),
            )),
            Command::Units => {
                let units = session.units()?;
                if is_json {
                    Ok(Some(serde_json::json!({"units": units})))
                } else {
                    print_units_table(&units);
                    Ok(None)
                }
            }
            Command::Probe => {
                let (id, serial) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.expect_serial.as_deref(),
                )?;
                let mut value = serde_json::to_value(&id)?;
                value["serial"] = serde_json::json!(serial);
                let model = crate::device::UnitCommands::model_identity(&mut session, id.slot)?;
                ensure!(model.len() >= 3, "truncated motor model reply");
                value["model_series"] = serde_json::json!(model[1]);
                value["model_unit"] = serde_json::json!(model[2]);
                Ok(Some(value))
            }
            Command::Settings { command } => {
                let (id, serial) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.expect_serial.as_deref(),
                )?;
                let model = resolve_settings_model(&mut session, &id)?;
                let is_oem = cli.unit_auth || match &command {
                    SettingsCommand::Set { name, .. } => {
                        let specs = crate::settings::schema_for_model(model);
                        specs.iter().any(|spec| spec.name.eq_ignore_ascii_case(name) && spec.oem)
                    }
                    SettingsCommand::Batch { operations, .. } => {
                        let specs = crate::settings::schema_for_model(model);
                        operations.iter().any(|op| {
                            if let crate::settings::batch::Operation::Set { name, .. } = op {
                                specs.iter().any(|spec| spec.name.eq_ignore_ascii_case(name) && spec.oem)
                            } else {
                                false
                            }
                        })
                    }
                    _ => false,
                };
                if is_oem {
                    ensure!(!cli.no_auth, "setting requires drive-unit authentication, but --no-auth was specified");
                    let receipt = session.authenticate_unit_settings(&serial)?;
                    notice_println!("Drive-unit authentication: {}", receipt);
                }
                Ok(Some(execute_settings(&mut session, &id, command)?))
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
    if let Some(val) = value {
        out_println!("{}", serde_json::to_string_pretty(&val)?);
    }
    Ok(())
}

fn execute_ble_pair(cli: Cli) -> Result<()> {
    let Command::Firmware {
        command:
            FirmwareCommand::Flash {
                command:
                    FlashCommand::BlePair {
                        dcas,
                        rx,
                        expect_dcas_sha256,
                        expect_rx_sha256,
                        journal,
                        backup,
                        write,
                    },
            },
    } = &cli.command
    else {
        unreachable!()
    };

    let serial_str = cli
        .expect_serial
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("BLE firmware flashing requires --expect-serial <12-HEX>"))?;
    let serial: [u8; 6] = hex::decode(serial_str)?.try_into().unwrap();

    let d = Firmware::load(dcas)?;
    let m = Firmware::load(rx)?;
    ensure!(
        d.metadata.kind == FirmwareKind::DcasX && m.metadata.kind == FirmwareKind::Renesas,
        "flash ble-pair requires D and M images"
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

    if let Some(exp) = expect_dcas_sha256 {
        ensure!(
            d.metadata.source_sha256.eq_ignore_ascii_case(exp),
            "DCAS firmware SHA-256 mismatch"
        );
    }
    if let Some(exp) = expect_rx_sha256 {
        ensure!(
            m.metadata.source_sha256.eq_ignore_ascii_case(exp),
            "RX firmware SHA-256 mismatch"
        );
    }

    let backup_path = backup.clone().unwrap_or_else(|| {
        PathBuf::from(format!("backup-firmware-preupdate-{}-{}.json", serial_str, crate::platform::now_ms()))
    });
    ensure!(!backup_path.exists(), "backup file already exists");

    let journal_path = journal.clone().unwrap_or_else(|| {
        PathBuf::from(format!("cranklink-flash-blepair-{}-{}.journal.jsonl", serial_str, crate::platform::now_ms()))
    });
    let mut jnl = Journal::new(&journal_path)?;
    jnl.record(
        "inputs",
        &serde_json::json!({
            "dcas": d.metadata,
            "rx": m.metadata,
            "write": write,
            "serial": serial_str,
            "transport": "ble"
        }),
    )?;

    let passkey = read_ble_passkey(&cli)?;
    let mut session =
        crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?;
    session.set_master_routing(true)?;

    let result = (|| -> Result<serde_json::Value> {
        session.authenticate()?;
        let (before, _) = ble_checked_start(&mut session, cli.slot, None, Some(serial_str))?;
        ensure!(
            parse_version(&before.dcas_version)? <= d.metadata.version
                && parse_version(&before.rx_version)? <= m.metadata.version,
            "flash ble-pair only permits upgrades, not downgrades"
        );
        jnl.record("application-before", &before)?;
        let settings_before = ble_firmware_backup(&mut session, &before, serial_str, &backup_path)?;
        jnl.record(
            "settings-backup",
            &serde_json::json!({"path": backup_path, "serial": serial_str}),
        )?;

        let mut link = session.motor_firmware(&serial)?;
        let rx_boot = link.enter_rx()?;
        jnl.record("rx-boot-before", &rx_boot)?;
        link.reset_rx()?;
        link.application_return()?;
        let (checked, _) = ble_checked_start(&mut session, None, None, Some(serial_str))?;
        ensure!(
            checked.cargo == before.cargo
                && checked.dcas_version == before.dcas_version
                && checked.rx_version == before.rx_version,
            "RX probe did not restore the unchanged application"
        );

        let mut link = session.motor_firmware(&serial)?;
        let boot = link.enter_dcas()?;
        jnl.record("dcas-boot-before", &boot)?;
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

        if !*write {
            link.application_return()?;
            jnl.record("dry-run-complete", &boot)?;
            out_println!("\n[DRY RUN - VALIDATION ONLY]");
            out_println!("- DCAS & RX Bootloader Handshakes: Verified");
            out_println!("- Motor Serial: Verified ({})", serial_str);
            out_println!("- Pre-Update Settings Backup: Saved ({})", backup_path.display());
            out_println!("- Flash State: UNMODIFIED (No bytes written)");
            out_println!(">> To execute live flash programming, add the --write flag.\n");
            return Ok(serde_json::json!({
                "transport": "ble",
                "write": false,
                "serial": serial_str,
                "dry_run": true,
            }));
        }

        jnl.record("dcas-destructive-start", &d.metadata)?;
        let receipt = link.flash_dcas(&d.data, &boot, progress_reporter())?;
        jnl.record("dcas-finish-and-reset", &receipt)?;
        link.application_return()?;

        let incomplete_pair = before.rx_version != m.metadata.version.to_string();
        if !incomplete_pair {
            let (middle, _) = ble_checked_start(&mut session, None, None, Some(serial_str))?;
            ensure!(
                middle.cargo == before.cargo && middle.rx_version == before.rx_version,
                "identity or RX changed after D operation"
            );
            ensure!(
                middle.dcas_version == d.metadata.version.to_string(),
                "D version readback failed"
            );
            jnl.record("application-between", &middle)?;
        }

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
        jnl.record("rx-destructive-start", &m.metadata)?;
        let receipt = link.flash_rx(&m, boot, progress_reporter())?;
        jnl.record("rx-finish-and-reset", &receipt)?;
        link.application_return()?;

        let (after, _) = ble_checked_start(&mut session, None, None, Some(serial_str))?;
        ensure!(
            after.cargo == before.cargo,
            "cargo identity changed after pair operation"
        );
        ensure!(
            after.dcas_version == d.metadata.version.to_string(),
            "final D version mismatch"
        );
        ensure!(
            after.rx_version == m.metadata.version.to_string(),
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
        jnl.record("settings-after", &settings_after)?;
        ensure!(
            crate::settings::decoded_snapshots_equal(&settings_before, &settings_after),
            "settings changed after firmware operation; inspect backup and journal"
        );
        jnl.record("verified", &after)?;
        Ok(serde_json::json!({
            "transport": "ble",
            "write": true,
            "serial": serial_str,
            "application_before": before,
            "application_after": after,
            "settings_unchanged": true
        }))
    })();

    if let Err(e) = &result {
        jnl.record("uncertain", &format!("{e:#}"))?;
    }
    let stopped = session.stop();
    let closed = session.close();
    let result = finish_operation(result, stopped.context("BLE PC-link cleanup not confirmed"));
    let result = finish_operation(result, closed.context("BLE disconnect failed"));
    if result.is_ok() {
        jnl.record("cleanup-confirmed", &true)?;
    }
    let value = result?;
    out_println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

fn read_ble_passkey(cli: &Cli) -> Result<Option<String>> {
    if let Some(pk) = &cli.passkey {
        ensure!(
            pk.len() == 6 && pk.bytes().all(|b| b.is_ascii_digit()),
            "BLE passkey must contain six ASCII digits"
        );
        return Ok(Some(pk.clone()));
    }
    cli.passkey_file
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

fn sanitize_serial(input: &str) -> Result<String> {
    let clean: String = input.chars().filter(|c| *c != '-' && *c != ':').collect();
    ensure!(
        clean.len() == 12 && clean.bytes().all(|b| b.is_ascii_hexdigit()),
        "motor serial must contain exactly 12 hexadecimal digits (got '{input}')"
    );
    Ok(clean.to_lowercase())
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
    match command {
        Command::Scan { .. } | Command::Units | Command::Probe | Command::Session => true,
        Command::Settings { command } => matches!(
            command,
            SettingsCommand::Read { .. } | SettingsCommand::Schema { .. }
        ),
        Command::Firmware { command } => matches!(
            command,
            FirmwareCommand::Inspect { .. }
        ),
        Command::Adapter { command } => matches!(
            command,
            AdapterCommand::Status
        ),
        Command::Ble { command } => matches!(
            command,
            BleCommand::Scan { .. } | BleCommand::TestLink
        ),
    }
}

fn ble_command_allowed(command: &Command) -> bool {
    match command {
        Command::Probe | Command::Units | Command::Session => true,
        Command::Settings { .. } => true,
        Command::Firmware { command } => matches!(
            command,
            FirmwareCommand::Flash {
                command: FlashCommand::BlePair { .. }
            }
        ),
        Command::Ble { .. } => true,
        _ => false,
    }
}

fn settings_model_name(name: &str) -> crate::settings::SettingsModel {
    match name {
        "ep600" => crate::settings::SettingsModel::Ep600,
        "ep801" => crate::settings::SettingsModel::Ep801,
        _ => crate::settings::SettingsModel::Ep800Crg,
    }
}

fn resolve_settings_model(
    session: &mut dyn crate::device::UnitCommands,
    id: &crate::device::Identity,
) -> Result<crate::settings::SettingsModel> {
    let identity = session.model_identity(id.slot)?;
    ensure!(
        identity.len() >= 3 && identity[2] == 0,
        "unsupported settings motor identity"
    );
    match identity[1] {
        0x23 if id.cargo == "00-CRG" => Ok(crate::settings::SettingsModel::Ep800Crg),
        0x30 if id.dcas_version == "4.6.0.1" => Ok(crate::settings::SettingsModel::Ep600),
        0x35 if id.dcas_version == "4.6.0.1" => Ok(crate::settings::SettingsModel::Ep801),
        _ => anyhow::bail!("settings schema is not verified for this model/firmware"),
    }
}

fn execute_settings(
    session: &mut dyn crate::device::UnitCommands,
    id: &crate::device::Identity,
    command: SettingsCommand,
) -> Result<serde_json::Value> {
    let model = resolve_settings_model(session, id)?;
    let serial_raw = session.command(id.slot, 1, 0x3c, &[0], 0x3e);
    let serial_str = serial_raw
        .ok()
        .and_then(|r| crate::device::serial_bytes(&r).ok())
        .map(hex::encode)
        .unwrap_or_else(|| format!("slot{}", id.slot));

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
            dry_run,
        } => {
            if dry_run {
                let specs = crate::settings::schema_for_model(model);
                let spec = specs
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| anyhow::anyhow!("unknown setting: {name}"))?;
                let current = crate::settings::read_for_model(session, id.slot, &name, model)?;
                return Ok(serde_json::json!({
                    "dry_run": true,
                    "valid": true,
                    "setting": name,
                    "current_value": current.value,
                    "planned_value": value,
                    "oem": spec.oem,
                    "unit": spec.unit,
                }));
            }
            let backup_path = backup.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "backup-{}-{}-{}.json",
                    name.replace('.', "_"),
                    serial_str,
                    crate::platform::now_ms()
                ))
            });
            let receipt = crate::settings::set_for_model(
                session, id.slot, &name, &value, &backup_path, model,
            )?;
            let mut v = serde_json::to_value(receipt)?;
            v["backup_file"] = serde_json::json!(backup_path);
            Ok(v)
        }
        SettingsCommand::Restore { file, backup } => {
            let backup_path = backup.unwrap_or_else(|| {
                PathBuf::from(format!(
                    "backup-restore-{}-{}.json",
                    serial_str,
                    crate::platform::now_ms()
                ))
            });
            let receipt = crate::settings::restore_for_model(
                session, id.slot, &file, &backup_path, model,
            )?;
            let mut v = serde_json::to_value(receipt)?;
            v["current_backup_file"] = serde_json::json!(backup_path);
            Ok(v)
        }
        SettingsCommand::Batch {
            operations,
            backup_prefix,
            ..
        } => {
            let prefix = backup_prefix.unwrap_or_else(|| {
                PathBuf::from(format!("batch-{}", crate::platform::now_ms()))
            });
            crate::settings::batch::run(session, id.slot, &operations, &prefix, model)
        }
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

fn print_units_table(units: &[crate::device::UnitIdentity]) {
    if units.is_empty() {
        out_println!("No connected E-TUBE components discovered.");
        return;
    }
    out_println!("Connected E-TUBE Components ({} found):\n", units.len());
    out_println!(
        "  {:<4}  {:<12}  {:<20}  {:<11}  {}",
        "SLOT",
        "TYPE",
        "MODEL",
        "SERIES:UNIT",
        "STATUS"
    );
    for u in units {
        let type_desc = match u.model_unit {
            0 if u.is_motor() => "Drive Unit",
            1 => "Switch",
            2 => "Display",
            3 | 4 => "Derailleur",
            _ => "Component",
        };
        let model_desc = if let Some(m) = u.model {
            m.to_string()
        } else if !u.model_candidates.is_empty() {
            u.model_candidates.join(" / ")
        } else {
            "Unknown".to_string()
        };
        let status = if u.model.is_some() {
            if let Some(p) = u.part {
                format!("Identified (Part: 0x{p:02X})")
            } else {
                "Identified".to_string()
            }
        } else if !u.model_candidates.is_empty() {
            format!("{} candidates", u.model_candidates.len())
        } else {
            "Unrecognized".to_string()
        };
        out_println!(
            "  {:<4}  {:<12}  {:<20}  0x{:02X}:0x{:02X}    {}",
            u.slot,
            type_desc,
            model_desc,
            u.model_series,
            u.model_unit,
            status
        );
    }
    out_println!(
        "\nTip: Run 'cranklink probe' to inspect drive-unit firmware versions (DCAS/RX) and serial number."
    );
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
mod cli_tests {
    use super::*;

    #[test]
    fn serial_sanitization_accepts_clean_and_formatted_serials() {
        assert_eq!(
            sanitize_serial("010203040506").unwrap(),
            "010203040506"
        );
        assert_eq!(
            sanitize_serial("01-02-03-04-05-06").unwrap(),
            "010203040506"
        );
        assert_eq!(
            sanitize_serial("01:02:03:04:05:06").unwrap(),
            "010203040506"
        );
        assert_eq!(
            sanitize_serial("A1B2C3D4E5F6").unwrap(),
            "a1b2c3d4e5f6"
        );
        assert!(sanitize_serial("short").is_err());
        assert!(sanitize_serial("01020304050G").is_err());
    }

    #[test]
    fn read_only_rejects_mutating_commands() {
        for args in [
            vec!["settings", "set", "lamp", "1"],
            vec!["settings", "restore", "backup.json"],
            vec!["firmware", "flash", "dcas", "fw.dat", "--companion", "comp.dat"],
            vec!["adapter", "power-cycle"],
        ] {
            let mut command = vec!["cranklink", "--read-only"];
            command.extend(args);
            let error = execute(command.into_iter().map(String::from).collect()).unwrap_err();
            assert!(error.to_string().contains("--read-only rejects"), "{error:#}");
        }

        // Read-only allows discovery and reads
        for args in [
            vec!["scan"],
            vec!["units"],
            vec!["probe"],
            vec!["settings", "read"],
            vec!["settings", "schema"],
            vec!["firmware", "inspect", "fw.dat"],
        ] {
            let mut command = vec!["cranklink", "--read-only"];
            command.extend(args);
            if let Ok(cli) = Cli::try_parse_from(command) {
                assert!(read_only_command_allowed(&cli.command));
            }
        }
    }

    #[test]
    fn scan_options_parsing() {
        let cli = Cli::try_parse_from(["cranklink", "scan"]).unwrap();
        match cli.command {
            Command::Scan { ble, .. } => assert!(!ble),
            _ => panic!("expected Scan command"),
        }

        let cli = Cli::try_parse_from(["cranklink", "scan", "--ble", "--seconds", "10"]).unwrap();
        match cli.command {
            Command::Scan { ble, seconds, .. } => {
                assert!(ble);
                assert_eq!(seconds, 10);
            }
            _ => panic!("expected Scan command with ble"),
        }
    }

    #[test]
    fn settings_dry_run_flag() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "-s",
            "010203040506",
            "settings",
            "set",
            "profile1.boost.torque",
            "85",
            "--dry-run",
        ])
        .unwrap();

        match cli.command {
            Command::Settings {
                command: SettingsCommand::Set { dry_run, .. },
            } => assert!(dry_run),
            _ => panic!("expected Settings Set"),
        }
    }

    #[test]
    fn targeted_schema_query_parsing() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "settings",
            "schema",
            "destination",
            "--model",
            "ep800-crg",
        ])
        .unwrap();

        match cli.command {
            Command::Settings {
                command: SettingsCommand::Schema { name, model },
            } => {
                assert_eq!(name.as_deref(), Some("destination"));
                assert_eq!(model, "ep800-crg");
            }
            _ => panic!("expected Schema command"),
        }
    }

    #[test]
    fn firmware_flash_auto_sha_and_optional_journal() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "-s",
            "010203040506",
            "firmware",
            "flash",
            "dcas",
            "dcas.dat",
            "--companion",
            "rx.dat",
        ])
        .unwrap();

        match cli.command {
            Command::Firmware {
                command:
                    FirmwareCommand::Flash {
                        command:
                            FlashCommand::Dcas {
                                file,
                                companion,
                                expect_sha256,
                                journal,
                                write,
                                ..
                            },
                    },
            } => {
                assert_eq!(file, PathBuf::from("dcas.dat"));
                assert_eq!(companion, PathBuf::from("rx.dat"));
                assert!(expect_sha256.is_none());
                assert!(journal.is_none());
                assert!(!write); // default is dry-run!
            }
            _ => panic!("expected Firmware Flash Dcas"),
        }
    }

    #[test]
    fn output_format_options() {
        let cli_default = Cli::try_parse_from(["cranklink", "units"]).unwrap();
        assert_eq!(cli_default.format, OutputFormat::Auto);

        let cli_text = Cli::try_parse_from(["cranklink", "--format", "text", "units"]).unwrap();
        assert_eq!(cli_text.format, OutputFormat::Text);
        assert!(!cli_text.format.is_json());

        let cli_json = Cli::try_parse_from(["cranklink", "--format", "json", "units"]).unwrap();
        assert_eq!(cli_json.format, OutputFormat::Json);
        assert!(cli_json.format.is_json());
    }
}
