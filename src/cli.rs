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
use std::path::{Path, PathBuf};

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default, serde::Serialize)]
#[value(rename_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum BleRoutingPolicy {
    #[default]
    Auto,
    #[value(alias = "false", alias = "slave")]
    Normal,
    #[value(alias = "true")]
    Master,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct BleRoutingDecision {
    pub route: &'static str,
    pub policy: BleRoutingPolicy,
    pub reason: String,
}

impl BleRoutingDecision {
    pub fn is_master(&self) -> bool {
        self.route == "master"
    }
}

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
                    // In WebAssembly, commands are invoked by the web instrument / JS driver
                    // which expects structured JSON by default.
                    true
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
    long_about = "Shimano STEPS e-bike diagnostic, recovery, and configuration CLI with BLE & USB/serial support.\nFirmware flashing is a dry run unless --write is given; settings writes are backed up first. Unofficial; can brick hardware.",
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
        help = "Adapter (default: auto); usb, COM3, ble:DEVICE_ID, /dev/ttyUSB0, sim:/absolute/socket, ble-sim:/absolute/socket"
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
        help = "Require this 12-hex-digit unit serial from probe (auto-sanitized from colons/hyphens)"
    )]
    expect_serial: Option<String>,

    #[arg(
        long,
        short = 'c',
        global = true,
        help_heading = "Target Verification",
        help = "Target component model filter (e.g. DU-EP800-CRG)"
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
        value_name = "POLICY",
        num_args = 0..=1,
        default_missing_value = "master",
        require_equals = true,
        value_enum,
        help = "BLE gateway routing policy (default: auto; --ble-master forces master routing)"
    )]
    ble_master: Option<BleRoutingPolicy>,

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
    /// Probe the drive unit, or the component chosen with --component/--slot (model, serial, firmware versions)
    Probe,
    /// Persistent interactive JSON-RPC session driver for AI agents & test runners
    Session,
    /// Read, export, and configure drive-unit and catalogued component settings
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

#[derive(Subcommand, Clone)]
enum SettingsCommand {
    /// List setting names, units, accepted ranges, and OEM flags
    Schema {
        /// Specific setting name to inspect (optional)
        name: Option<String>,
        /// Motor family alias or catalogued component model (e.g. ST-6870-L)
        #[arg(long, default_value = "ep800-crg", value_parser = parse_settings_model)]
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
        #[arg(allow_negative_numbers = true)]
        value: String,
        /// Pre-write backup path (auto-generated if omitted)
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Run the write's input and live checks without writing (no unit authentication)
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
        #[arg(skip)]
        parsed: Option<crate::settings::ParsedRestore>,
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
    /// Read the 64 KiB bootloader from a working, powered EP800-CRG (no motor writes)
    DumpBootloader {
        /// New directory for resident.bin, wire trace and result receipt
        #[arg(long)]
        output: PathBuf,
    },
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
        /// Allow live flashing on protocol-verified non-EP800 drive unit models
        #[arg(long, global = true, help_heading = "Safety")]
        allow_unverified_model: bool,
        #[command(subcommand)]
        command: FlashCommand,
    },
}

#[derive(Subcommand)]
enum FlashCommand {
    /// Program D / DCAS microcontroller image over wired connection
    Dcas {
        /// Path to DCAS container file (.dat)
        file: PathBuf,
        /// Required matching RX companion container file (.dat)
        #[arg(long)]
        companion: PathBuf,
        /// Expected file SHA-256; required to write a plaintext image, which has no container checksum
        #[arg(long)]
        expect_sha256: Option<String>,
        /// Transaction journal path (auto-generated if omitted)
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Recover from an already-interrupted bootloader state
        #[arg(long)]
        recovery: bool,
        /// Recover the EP800 Key-2 stall with the documented temporary resistor bridge
        #[arg(long, conflicts_with = "recovery")]
        bridge_recovery: bool,
        /// Maximum connection attempts before any flash write, 1-20 (bridge recovery only; default: 20)
        #[arg(long, requires = "bridge_recovery", value_parser = clap::value_parser!(u8).range(1..=20))]
        attempts: Option<u8>,
        /// Seconds to establish the bridge before connecting (default: 10)
        #[arg(long, requires = "bridge_recovery", value_parser = clap::value_parser!(u64).range(0..=300))]
        countdown: Option<u64>,
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
        /// Expected file SHA-256; required to write a plaintext image, which has no container checksum
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
        /// Expected DCAS file SHA-256; required to write a plaintext D image (no container checksum)
        #[arg(long)]
        expect_dcas_sha256: Option<String>,
        /// Expected RX file SHA-256; required to write a plaintext M image (no container checksum)
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
        Err(error)
            if error.kind() == clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand =>
        {
            err_println!("{}", error.render().to_string().trim_end());
            anyhow::bail!("a command is required");
        }
        // Callers prefix "error: " themselves; clap's rendering already has it.
        Err(error) => {
            let rendered = error.render().to_string();
            anyhow::bail!("{}", rendered.trim_end().trim_start_matches("error: "));
        }
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
    // Only a Bluetooth port uses the passkey; an exported one must not break wired use.
    if cli.port.as_deref().is_some_and(crate::ble::is_port)
        && cli.passkey.is_none()
        && cli.passkey_file.is_none()
    {
        if let Ok(pk) = std::env::var("CRANKLINK_PASSKEY") {
            if !pk.trim().is_empty() {
                cli.passkey = Some(pk.trim().to_string());
            }
        }
    }
    let mut format = cli.format;
    if cli.json {
        format = OutputFormat::Json;
    } else if cli.format == OutputFormat::Auto {
        // An explicit --format wins over the environment.
        if let Ok(f) = std::env::var("CRANKLINK_FORMAT") {
            if f.eq_ignore_ascii_case("json") {
                format = OutputFormat::Json;
            } else if f.eq_ignore_ascii_case("text") {
                format = OutputFormat::Text;
            }
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
        "--read-only rejects mutating commands (settings set/restore/batch, firmware flash/probe, adapter power-cycle)"
    );
    ensure!(
        !(cli.unit_auth && cli.no_auth),
        "--unit-auth and --no-auth cannot be used together"
    );

    // Component validation
    if let Some(component) = &cli.component {
        ensure!(
            !component.trim().is_empty()
                && matches!(
                    cli.command,
                    Command::Probe
                        | Command::Settings {
                            command: SettingsCommand::Read { .. }
                                | SettingsCommand::Set { .. }
                                | SettingsCommand::Restore { .. }
                                | SettingsCommand::Batch { .. }
                        }
                ),
            "--component applies only to probe or settings read, set, restore or batch"
        );
    }

    // Normalize standalone restore or batch plan before opening a session
    match &mut cli.command {
        Command::Settings {
            command: SettingsCommand::Restore { file, parsed, .. },
        } => {
            *parsed = Some(crate::settings::parse_restore_file(file)?);
        }
        Command::Settings {
            command:
                SettingsCommand::Batch {
                    plan,
                    operations,
                    backup_prefix,
                },
        } => {
            let prefix = backup_prefix.get_or_insert_with(|| {
                PathBuf::from(format!("batch-{}", crate::platform::now_ms()))
            });
            ensure!(
                !prefix.as_os_str().is_empty(),
                "backup prefix must not be empty"
            );
            let mut loaded = crate::settings::batch::load(plan)?;
            crate::settings::batch::preload_restores(&mut loaded)?;
            *operations = loaded;
        }
        _ => {}
    }

    require_write_serial(&cli.command, cli.expect_serial.as_deref())?;

    // `ble scan` is the documented spelling of `scan --ble`; neither needs a port.
    if let Command::Ble {
        command: BleCommand::Scan { seconds, all },
    } = cli.command
    {
        cli.command = Command::Scan {
            ble: true,
            seconds,
            all,
        };
    }

    // BLE dispatch vs native wired dispatch
    if cli.port.as_deref().is_some_and(crate::ble::is_port)
        && !matches!(
            cli.command,
            Command::Scan { .. }
                | Command::Firmware {
                    command: FirmwareCommand::Inspect { .. }
                }
                | Command::Settings {
                    command: SettingsCommand::Schema { .. }
                }
        )
    {
        return execute_ble(cli, is_json);
    }

    // Require BLE port for BLE-only operations. Offline commands and scans
    // stay off BLE dispatch even with a Bluetooth port, and ignore its options.
    if !cli.port.as_deref().is_some_and(crate::ble::is_port) {
        ensure!(
            cli.passkey.is_none() && cli.passkey_file.is_none(),
            "--passkey and --passkey-file require a Bluetooth port (--port ble:DEVICE_ID)"
        );
        ensure!(
            cli.ble_master.is_none(),
            "--ble-master requires a Bluetooth port (--port ble:DEVICE_ID)"
        );
    }

    let port = cli.port;
    let slot = cli.slot;
    let trace = cli.trace;
    let expect_serial = cli.expect_serial;
    let component = cli.component;
    let no_auth = cli.no_auth;
    let unit_auth = cli.unit_auth;
    let json = is_json;

    match cli.command {
        Command::Session => session::run_session(port, trace, expect_serial),
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
                        let name = d.name.as_deref().unwrap_or("Unknown");
                        let rssi = d
                            .rssi
                            .map(|r| format!("{} dBm", r))
                            .unwrap_or_else(|| "N/A".to_string());
                        out_println!("  {:<20} {} (RSSI: {})", d.id, name, rssi);
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
            let mut session = Session::open(port.as_deref().unwrap_or("auto"), trace.as_deref())?;
            session.initialize()?;
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
            let mut session = Session::open(port.as_deref().unwrap_or("auto"), trace.as_deref())?;
            session.initialize()?;
            let result = session
                .probe_component(slot, component.as_deref())
                .and_then(|identity| {
                    check_motor_serial(&identity.serial, expect_serial.as_deref())?;
                    Ok(identity)
                });
            let stopped = session.normal_stop();
            let identity =
                finish_operation(result, stopped.context("PC-link Stop acknowledgement"))?;
            if is_json {
                out_println!("{}", serde_json::to_string_pretty(&identity)?);
            } else {
                print_probe_card(
                    identity.identity.slot,
                    identity.model_series,
                    identity.model_unit,
                    &identity.identity.cargo,
                    &identity.serial,
                    &identity.identity.dcas_version,
                    &identity.identity.rx_version,
                );
            }
            Ok(())
        }
        Command::Settings {
            command: SettingsCommand::Schema { name, model },
        } => {
            let model_enum = settings_model_name(&model)?;
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
                if is_json {
                    out_println!("{}", serde_json::to_string_pretty(&spec)?);
                } else {
                    print_setting_schema(spec);
                }
            } else if is_json {
                out_println!("{}", serde_json::to_string_pretty(&specs)?);
            } else {
                print_settings_schema_table(&specs);
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

                    let model = resolve_settings_model(&mut s, &id)?;
                    validate_settings_preflight(&command, model, &motor_serial)?;
                    ensure!(
                        !(no_auth && write_requires_unit_auth(&command, model, unit_auth)),
                        "setting requires drive-unit authentication, but --no-auth was specified"
                    );
                    if command_requires_unit_auth(&command, model, unit_auth) {
                        let _receipt = s.authenticate_unit_settings(&motor_serial)?;
                    }

                    let val = execute_settings(&mut s, &id, &command, Some(&motor_serial))?;
                    Ok((val, model))
                });
            let stopped = s.normal_stop();
            finish_reporting(
                result,
                stopped.context("PC-link Stop acknowledgement"),
                |(value, model)| print_settings_result(value, &command, *model, is_json),
            )
        }
        Command::Firmware { command } => execute_firmware_command(
            port.as_deref(),
            trace.as_deref(),
            slot,
            expect_serial.as_deref(),
            command,
            is_json,
        ),
        Command::Adapter { command } => {
            execute_adapter_command(port.as_deref(), trace.as_deref(), slot, command, is_json)
        }
        Command::Ble { .. } => {
            anyhow::bail!(
                "Bluetooth commands require selecting a Bluetooth port (--port ble:DEVICE_ID)"
            )
        }
    }
}

/// Single-quote anything a shell would split or expand (Windows paths with
/// spaces are common); POSIX shells and PowerShell both accept this form.
fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:\\".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// The exact command to resume an interrupted write on the same adapter and slot.
fn recovery_command(
    target: &str,
    port: Option<&str>,
    slot: Option<u8>,
    serial: &str,
    file: &Path,
    companion: &Path,
    expect_sha256: Option<&str>,
    allow_unverified_model: bool,
) -> String {
    let mut command = String::from("cranklink");
    if let Some(port) = port {
        command.push_str(&format!(" --port {}", shell_quote(port)));
    }
    if let Some(slot) = slot {
        command.push_str(&format!(" --slot {slot}"));
    }
    command.push_str(&format!(
        " -s {serial} firmware flash {target} {} --companion {}",
        shell_quote(&file.display().to_string()),
        shell_quote(&companion.display().to_string())
    ));
    if let Some(sha256) = expect_sha256 {
        command.push_str(&format!(" --expect-sha256 {sha256}"));
    }
    command.push_str(" --recovery --write");
    if allow_unverified_model {
        command.push_str(" --allow-unverified-model");
    }
    command
}

/// Entry from the application leaves the bootloader at the application's slot
/// once 01/C0 is acknowledged, so a failure after that may leave it there. A
/// failure before it leaves the application running, and the recovery reset
/// sends the key sequence, so the read-only probe comes first.
fn bootloader_guidance(port: Option<&str>, slot: u8, failure: &str) -> String {
    let mut probe = String::from("cranklink");
    if let Some(port) = port {
        probe.push_str(&format!(" --port {}", shell_quote(port)));
    }
    let mut reset = probe.clone();
    if slot != 31 {
        reset.push_str(&format!(" --slot {slot}"));
    }
    probe.push_str(" probe");
    reset.push_str(" firmware probe --target dcas --recovery --reset");
    format!(
        "{failure}; nothing was erased or written.\n\
         The motor may remain in its DCAS bootloader. Check whether its application answers with:\n  {probe}\n\
         Only if it does not, return it to its application with:\n  {reset}"
    )
}

/// A plaintext image has no container checksum, so corruption is undetectable;
/// a write must be bound to a hash the user verified. Callers compare the hash.
fn ensure_plaintext_hash(
    fw: &Firmware,
    file: &Path,
    expected: Option<&str>,
    flag: &str,
) -> Result<()> {
    ensure!(
        fw.metadata.encrypted || expected.is_some(),
        "{} is a plaintext image without container integrity data; pass {flag} <SHA-256 of the file you verified> to write it",
        file.display()
    );
    Ok(())
}

fn integrity_label(meta: &crate::firmware::FirmwareMetadata) -> &'static str {
    if meta.encrypted {
        "MD5-verified container"
    } else {
        "none (plaintext image; corruption cannot be detected)"
    }
}

fn execute_firmware_command(
    port: Option<&str>,
    trace: Option<&std::path::Path>,
    slot: Option<u8>,
    expect_serial: Option<&str>,
    command: FirmwareCommand,
    is_json: bool,
) -> Result<()> {
    match command {
        FirmwareCommand::DumpBootloader { output } => {
            ensure!(
                slot.is_none(),
                "bootloader extraction selects a unique motor; use -s to select by serial"
            );
            ensure!(
                trace.is_none(),
                "bootloader extraction saves its wire trace in --output"
            );
            ensure!(
                port.is_none() || port == Some("usb"),
                "bootloader extraction requires the native SM-PCE02 USB connection"
            );
            #[cfg(target_arch = "wasm32")]
            {
                let _ = output;
                anyhow::bail!("bootloader extraction requires the native CLI");
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                let receipt = crate::bootloader_dump::dump("usb", expect_serial, &output)?;
                if is_json {
                    out_println!("{}", serde_json::to_string_pretty(&receipt)?);
                } else {
                    out_println!(
                        "Read {} bootloader bytes to {}",
                        receipt.bytes,
                        output.join("resident.bin").display()
                    );
                    out_println!("Motor serial: {}", receipt.serial);
                    out_println!("SHA256: {}", receipt.sha256);
                    out_println!("Matches known donor image: {}", receipt.matches_known_dump);
                }
                Ok(())
            }
        }
        FirmwareCommand::Inspect {
            file,
            companion,
            decode_to,
        } => {
            let fw = Firmware::load(file)?;
            if let Some(c) = companion {
                fw.validate_companion(&Firmware::load(c)?)?;
            }
            if let Some(out) = &decode_to {
                use std::io::Write;
                let mut f = crate::storage::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(out)
                    .with_context(|| {
                        format!("create {} (must not already exist)", out.display())
                    })?;
                f.write_all(&fw.data)?;
                f.sync_all()?;
            }
            if is_json {
                out_println!("{}", serde_json::to_string_pretty(&fw.metadata)?);
            } else {
                print_firmware_metadata(&fw.metadata, decode_to.as_deref());
            }
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
                    let application = if !recovery {
                        let application = s.normal_start(slot)?;
                        if let Some(expected) = expect_serial {
                            let checked = hex::decode(expected)
                                .map_err(anyhow::Error::from)
                                .and_then(|serial| {
                                    check_application_serial(&mut s, application.slot, &serial)
                                })
                                .context("bootloader not entered");
                            if let Err(e) = checked {
                                s.normal_stop()
                                    .context("serial check failed; PC-link stop failed")?;
                                return Err(e);
                            }
                        }
                        Some(application)
                    } else {
                        None
                    };
                    let identity = match s.boot_start_at(!recovery, slot) {
                        Ok(identity) => identity,
                        Err(e) => {
                            if let Some(application) = &application {
                                err_println!(
                                    "{}",
                                    bootloader_guidance(
                                        port,
                                        application.slot,
                                        "Bootloader entry did not complete"
                                    )
                                );
                            }
                            return Err(e);
                        }
                    };
                    if !is_json {
                        out_println!("DCAS Bootloader Probe:");
                        out_println!("  Slot:       {}", identity.slot);
                        out_println!(
                            "  Version:    v{}.{}.{}.{}",
                            identity.version[0],
                            identity.version[1],
                            identity.version[2],
                            identity.version[3]
                        );
                        out_println!("  Board:      {}", identity.board);
                        out_println!("  Series:     0x{:02X}", identity.series);
                    }
                    if let Some(application) = &application {
                        verify_application_return(&mut s, port, slot, application, "DCAS probe")?;
                    } else if reset {
                        s.boot_reset()?;
                    }
                    // JSON is a success receipt; print it only after the checks pass.
                    if is_json {
                        out_println!("{}", serde_json::to_string_pretty(&identity)?);
                    } else if application.is_some() {
                        out_println!("  Status:     Application return verified");
                    }
                }
                "rx" => {
                    let before = start_rx_session(&mut s, slot, recovery)?;
                    if let (false, Some(expected)) = (recovery, expect_serial) {
                        let checked = hex::decode(expected)
                            .map_err(anyhow::Error::from)
                            .and_then(|serial| {
                                check_application_serial(&mut s, before.slot, &serial)
                            })
                            .context("RX update mode not entered");
                        if let Err(e) = checked {
                            s.normal_stop()
                                .context("serial check failed; PC-link stop failed")?;
                            return Err(e);
                        }
                    }
                    let boot = match crate::rx::read_boot(&mut s, before.slot) {
                        Ok(boot) => boot,
                        Err(e) if e.is::<crate::rx::RefusedBeforeErase>() => {
                            let restarted =
                                return_after_rx_refusal(&mut s, &e, before.slot, recovery, None);
                            return finish_operation(Err(e), restarted);
                        }
                        Err(e) => return Err(e),
                    };
                    if !is_json {
                        out_println!("Renesas RX Bootloader Probe:");
                        out_println!("  Capability: {}", boot.capability_sequence);
                    }
                    crate::rx::reset_fast(&mut s, before.slot)?;
                    let after = start_rx_session(&mut s, Some(before.slot), recovery)?;
                    s.normal_stop()?;
                    ensure!(
                        after.cargo == before.cargo
                            && after.dcas_version == before.dcas_version
                            && after.rx_version == before.rx_version,
                        "application changed after RX probe"
                    );
                    if is_json {
                        out_println!("{}", serde_json::to_string_pretty(&boot)?);
                    } else {
                        out_println!(
                            "  Status:     {}",
                            if recovery {
                                "Cargo session restored (firmware remains in RX recovery)"
                            } else {
                                "Application return verified"
                            }
                        );
                    }
                }
                other => anyhow::bail!(
                    "unknown probe target '{other}'; supported targets are 'dcas' and 'rx'"
                ),
            }
            Ok(())
        }
        FirmwareCommand::Flash {
            allow_unverified_model,
            command,
        } => match command {
            FlashCommand::Dcas {
                file,
                companion,
                expect_sha256,
                journal,
                recovery,
                bridge_recovery,
                attempts,
                countdown,
                write,
            } => {
                if bridge_recovery {
                    ensure!(
                        !allow_unverified_model,
                        "--bridge-recovery accepts only the documented EP800 target; remove --allow-unverified-model"
                    );
                    ensure!(
                        slot.is_none() || slot == Some(31),
                        "--bridge-recovery requires bootloader slot 31"
                    );
                    #[cfg(target_arch = "wasm32")]
                    {
                        let _ = (attempts, countdown);
                        anyhow::bail!(
                            "--bridge-recovery requires the native CLI and an SM-PCE02 USB adaptor"
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        if let Some(expected) = expect_sha256 {
                            let fw = Firmware::load(&file)?;
                            ensure!(
                                fw.metadata.source_sha256.eq_ignore_ascii_case(&expected),
                                "source SHA-256 does not match --expect-sha256"
                            );
                        }
                        let journal = journal.unwrap_or_else(|| {
                            PathBuf::from(format!(
                                "cranklink-flash-dcas-{}-{}.journal.jsonl",
                                expect_serial.unwrap_or("unknown-serial"),
                                crate::platform::now_ms()
                            ))
                        });
                        let report = crate::recovery::recover_bridge(
                            crate::recovery::BridgeOptions {
                                port: port.as_deref().unwrap_or("usb"),
                                dcas: &file,
                                companion: &companion,
                                serial: expect_serial,
                                journal: &journal,
                                trace: trace.as_deref(),
                                countdown: countdown.unwrap_or(10),
                                attempts: attempts.unwrap_or(20),
                                write,
                            },
                            |message| notice_println!("{message}"),
                        )?;
                        if is_json {
                            out_println!(
                                "{}",
                                serde_json::json!({
                                    "mode": "bridge-recovery",
                                    "status": if report.transferred { "transfer-finished" } else { "preflight-passed" },
                                    "serial": report.serial,
                                    "application_verified": false,
                                    "journal": if write { Some(&journal) } else { None },
                                })
                            );
                        }
                        return Ok(());
                    }
                }
                let fw = Firmware::load(&file)?;
                ensure!(fw.metadata.kind.is_dcas(), "flash dcas requires a D image");
                let comp = Firmware::load(&companion)?;
                fw.validate_companion(&comp)?;
                if let Some(expected) = &expect_sha256 {
                    ensure!(
                        fw.metadata.source_sha256.eq_ignore_ascii_case(expected),
                        "source SHA-256 does not match --expect-sha256"
                    );
                }
                if write {
                    ensure_plaintext_hash(&fw, &file, expect_sha256.as_deref(), "--expect-sha256")?;
                }

                let serial_str = expect_serial.ok_or_else(|| {
                    anyhow::anyhow!("flashing requires motor serial (--expect-serial <12-HEX>)")
                })?;
                let serial = hex::decode(serial_str)?;
                ensure!(serial.len() == 6, "expected six-byte serial");

                let journal_path = journal.unwrap_or_else(|| {
                    PathBuf::from(format!(
                        "cranklink-flash-dcas-{}-{}.journal.jsonl",
                        serial_str,
                        crate::platform::now_ms()
                    ))
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
                    let preflight = check_application_serial(&mut s, i.slot, &serial)
                        .context("bootloader not entered")
                        .and_then(|()| {
                            fw.validate_companion_version_for_pair(
                                &comp,
                                parse_version(&i.rx_version)?,
                            )
                        });
                    if let Err(e) = preflight {
                        let e = journal_failure(&mut jnl, "preflight-rejected", e);
                        let stop = s
                            .normal_stop()
                            .context("preflight rejected image; PC-link stop failed");
                        return finish_operation(Err(e), stop);
                    }
                }
                let boot = match s.boot_start_at(!recovery, slot) {
                    Ok(boot) => boot,
                    Err(e) => {
                        let e = journal_failure(&mut jnl, "boot-entry-failed", e);
                        if let Some(i) = &before {
                            err_println!(
                                "{}",
                                bootloader_guidance(
                                    port,
                                    i.slot,
                                    "Bootloader entry did not complete"
                                )
                            );
                        }
                        return Err(e);
                    }
                };
                // A dry run applies the same model policy as the write it previews.
                let allow_model = allow_unverified_model;
                // A failed journal write refuses like the guard: still reset.
                let guard = jnl.record("boot-identity", &boot).and_then(|()| {
                    ensure!(
                        boot.serial.as_slice() == serial.as_slice(),
                        "live serial differs from --expect-serial"
                    );
                    fw.validate_target_with_policy(
                        boot.series,
                        boot.unit,
                        boot.board,
                        Version(boot.version),
                        allow_model,
                    )
                });
                let profile = match guard {
                    Ok(p) => p,
                    Err(e) => {
                        let e = journal_failure(&mut jnl, "guard-rejected", e);
                        if let Some(i) = &before {
                            let reset = s
                                .boot_reset()
                                .context("guard rejected image; application reset failed");
                            if reset.is_err() {
                                err_println!(
                                    "{}",
                                    bootloader_guidance(
                                        port,
                                        i.slot,
                                        "The bootloader reset was not acknowledged"
                                    )
                                );
                            }
                            return finish_operation(Err(e), reset);
                        }
                        return Err(e);
                    }
                };

                if !write {
                    if let Some(before) = &before {
                        let returned =
                            verify_application_return(&mut s, port, slot, before, "DCAS dry run");
                        let after = returned.map_err(|e| {
                            journal_failure(&mut jnl, "application-return-failed", e)
                        })?;
                        jnl.record("application-return-verified", &after)?;
                    }
                    jnl.record("dry-run-complete", &boot)?;
                    let needs_hash = !fw.metadata.encrypted && expect_sha256.is_none();
                    let needs_opt_in = profile.verification_tier
                        != crate::firmware::VerificationTier::HardwareVerified;
                    if is_json {
                        out_println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "dry_run": true,
                                "target": "dcas",
                                "family": profile.family_name,
                                "verification_tier": profile.verification_tier,
                                "boot": boot,
                                "serial": serial_str,
                                "source_sha256": fw.metadata.source_sha256,
                                "encrypted": fw.metadata.encrypted,
                                "flash_state": "unmodified",
                                "application_return_verified": before.is_some(),
                                // Each requires_* flag names an option --write must carry.
                                "requires_expect_sha256": !fw.metadata.encrypted,
                                "requires_allow_unverified_model": needs_opt_in,
                            }))?
                        );
                        return Ok(());
                    }
                    out_println!("\n[DRY RUN - VALIDATION ONLY]");
                    out_println!(
                        "- Target Bootloader: Verified ({} {:?} v{}.{}.{}.{}, Board {})",
                        profile.family_name,
                        profile.dcas_kind,
                        boot.version[0],
                        boot.version[1],
                        boot.version[2],
                        boot.version[3],
                        boot.board
                    );
                    out_println!("- Motor Serial: Verified ({})", serial_str);
                    out_println!("- Source SHA-256: {}", fw.metadata.source_sha256);
                    out_println!("- Integrity: {}", integrity_label(&fw.metadata));
                    out_println!("- Flash State: UNMODIFIED (No bytes written)");
                    if before.is_some() {
                        out_println!("- Application Return: Verified");
                    }
                    if needs_hash {
                        out_println!(">> Writing this plaintext image also requires --expect-sha256 <SHA-256 of the file you verified>.");
                    }
                    if needs_opt_in {
                        out_println!(">> To execute live flash programming on {}, add --write (keeping --allow-unverified-model).\n", profile.family_name);
                    } else {
                        out_println!(
                            ">> To execute live flash programming, add the --write flag.\n"
                        );
                    }
                    return Ok(());
                }

                jnl.record("destructive-start", &fw.metadata)?;
                let receipt = match s.flash_dcas(&fw.data, &boot, progress_reporter()) {
                    Ok(r) => r,
                    Err(e) => {
                        let e = journal_failure(&mut jnl, "uncertain", e);
                        err_println!("\n================================================================================");
                        err_println!("CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.");
                        err_println!("DO NOT POWER CYCLE THE MOTOR.");
                        err_println!("Journal preserved at: {}", journal_path.display());
                        err_println!("To recover, re-run with:");
                        // The bootloader keeps the slot it was entered at; 31 is the recovery default.
                        err_println!(
                            "  {}",
                            recovery_command(
                                "dcas",
                                port,
                                Some(boot.slot).filter(|slot| *slot != 31),
                                serial_str,
                                &file,
                                &companion,
                                (!fw.metadata.encrypted)
                                    .then_some(fw.metadata.source_sha256.as_str()),
                                allow_unverified_model
                            )
                        );
                        err_println!("================================================================================\n");
                        return Err(e);
                    }
                };
                // The transfer has finished: a journal failure must not skip the
                // application return and verification below.
                let finished = jnl.record("finish-and-reset", &receipt);
                let after = match s
                    .normal_start(if recovery { None } else { slot })
                    .context("transfer finished, but the application did not return")
                {
                    Ok(after) => after,
                    Err(e) => return Err(journal_failure(&mut jnl, "verification-failed", e)),
                };
                let serial_check = check_application_serial(&mut s, after.slot, &serial);
                let stop = s.normal_stop();
                let journaled = finished.and_then(|()| jnl.record("application-after", &after));
                let verified = serial_check.and_then(|()| {
                    ensure!(
                        after.dcas_version == fw.metadata.version.to_string(),
                        "post-flash DCAS version mismatch"
                    );
                    ensure!(
                        after.rx_version == fw.metadata.compatibility_version.to_string()
                            || after.rx_version == comp.metadata.version.to_string(),
                        "post-flash RX version mismatch"
                    );
                    if let Some(before) = &before {
                        ensure!(
                            before.cargo == after.cargo,
                            "cargo identity changed after flashing"
                        );
                    }
                    Ok(())
                });
                let verified = verified
                    .map_err(|e| journal_failure(&mut jnl, "verification-failed", e))
                    .and_then(|()| {
                        journaled.context(
                            "transfer finished and verified, but the flash journal is incomplete",
                        )
                    });
                finish_operation(verified, stop)?;
                jnl.record("verified", &after)
                    .context("flash verified, but the flash journal is incomplete")?;
                if is_json {
                    out_println!("{}", serde_json::to_string_pretty(&after)?);
                } else {
                    out_println!("DCAS flash programming completed successfully:");
                    out_println!("  Model Cargo: {}", after.cargo);
                    out_println!("  DCAS (D):    v{}", after.dcas_version);
                    out_println!("  Renesas (M): v{}", after.rx_version);
                }
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
                if write {
                    ensure_plaintext_hash(&fw, &file, expect_sha256.as_deref(), "--expect-sha256")?;
                }
                let serial_str = expect_serial.ok_or_else(|| {
                    anyhow::anyhow!("flashing requires motor serial (--expect-serial <12-HEX>)")
                })?;
                let serial = hex::decode(serial_str)?;
                ensure!(serial.len() == 6, "expected six-byte serial");

                let journal_path = journal.unwrap_or_else(|| {
                    PathBuf::from(format!(
                        "cranklink-flash-rx-{}-{}.journal.jsonl",
                        serial_str,
                        crate::platform::now_ms()
                    ))
                });
                let mut jnl = Journal::new(&journal_path)?;
                jnl.record("inputs", &fw.metadata)?;

                let mut s = Session::open(port.unwrap_or("auto"), trace)?;
                s.initialize()?;
                let before = start_rx_session(&mut s, slot, recovery)?;
                if !recovery {
                    let preflight = check_application_serial(&mut s, before.slot, &serial)
                        .context("bootloader not entered")
                        .and_then(|()| {
                            ensure!(
                                before.dcas_version == d.metadata.version.to_string(),
                                "live D version differs from supplied companion; flash D first"
                            );
                            Ok(())
                        });
                    if let Err(e) = preflight {
                        let e = journal_failure(&mut jnl, "preflight-rejected", e);
                        let stop = s
                            .normal_stop()
                            .context("preflight rejected image; PC-link stop failed");
                        return finish_operation(Err(e), stop);
                    }
                }
                jnl.record("application-before", &before)?;
                let boot = match s.boot_start(true) {
                    Ok(boot) => boot,
                    Err(e) => {
                        let e = journal_failure(&mut jnl, "boot-entry-failed", e);
                        err_println!(
                            "{}",
                            bootloader_guidance(
                                port,
                                before.slot,
                                "Bootloader entry did not complete"
                            )
                        );
                        return Err(e);
                    }
                };
                // A dry run applies the same model policy as the write it previews.
                let allow_model = allow_unverified_model;
                // A failed journal write refuses like the guard: still reset.
                let guard = jnl.record("dcas-identity", &boot).and_then(|()| {
                    ensure!(
                        boot.serial.as_slice() == serial.as_slice(),
                        "live serial differs from expected"
                    );
                    d.validate_target_with_policy(
                        boot.series,
                        boot.unit,
                        boot.board,
                        Version(boot.version),
                        allow_model,
                    )
                });
                let reset = s.boot_reset();
                if reset.is_err() {
                    err_println!(
                        "{}",
                        bootloader_guidance(
                            port,
                            before.slot,
                            "The bootloader reset was not acknowledged"
                        )
                    );
                }
                let profile = finish_operation(guard, reset)?;
                let ready = start_rx_session(&mut s, Some(before.slot), recovery)?;

                if !write {
                    let rx = match crate::rx::read_boot(&mut s, ready.slot) {
                        Ok(rx) => rx,
                        Err(e) if e.is::<crate::rx::RefusedBeforeErase>() => {
                            let e = journal_failure(&mut jnl, "dry-run-refused", e);
                            let restarted = return_after_rx_refusal(
                                &mut s,
                                &e,
                                before.slot,
                                recovery,
                                Some(&mut jnl),
                            );
                            return finish_operation(Err(e), restarted);
                        }
                        Err(e) => return Err(e),
                    };
                    jnl.record("rx-boot", &rx)?;
                    crate::rx::reset_fast(&mut s, before.slot)?;
                    let after = start_rx_session(&mut s, Some(before.slot), recovery)?;
                    s.normal_stop()?;
                    // Predict the write: it refuses a retry-capable bootloader without the flag.
                    if let Err(e) =
                        crate::rx::validate_fast_capabilities(&rx, allow_unverified_model)
                    {
                        jnl.record("dry-run-refused", &format!("{e:#}"))?;
                        return Err(e);
                    }
                    jnl.record("dry-run-complete", &after)?;
                    let needs_hash = !fw.metadata.encrypted && expect_sha256.is_none();
                    let needs_opt_in = profile.verification_tier
                        != crate::firmware::VerificationTier::HardwareVerified
                        || rx.retry_capable;
                    if is_json {
                        out_println!(
                            "{}",
                            serde_json::to_string_pretty(&serde_json::json!({
                                "dry_run": true,
                                "target": "rx",
                                "family": profile.family_name,
                                "verification_tier": profile.verification_tier,
                                "rx_boot": rx,
                                "serial": serial_str,
                                "source_sha256": fw.metadata.source_sha256,
                                "encrypted": fw.metadata.encrypted,
                                "flash_state": "unmodified",
                                // Each requires_* flag names an option --write must carry.
                                "requires_expect_sha256": !fw.metadata.encrypted,
                                "requires_allow_unverified_model": needs_opt_in,
                            }))?
                        );
                        return Ok(());
                    }
                    out_println!("\n[DRY RUN - VALIDATION ONLY]");
                    out_println!(
                        "- RX Bootloader: Verified ({}, Sequence {})",
                        profile.family_name,
                        rx.capability_sequence
                    );
                    out_println!("- Motor Serial: Verified ({})", serial_str);
                    out_println!("- Source SHA-256: {}", fw.metadata.source_sha256);
                    out_println!("- Integrity: {}", integrity_label(&fw.metadata));
                    out_println!("- Flash State: UNMODIFIED (No bytes written)");
                    if needs_hash {
                        out_println!(">> Writing this plaintext image also requires --expect-sha256 <SHA-256 of the file you verified>.");
                    }
                    if needs_opt_in {
                        out_println!(">> To execute live flash programming on {}, add --write (keeping --allow-unverified-model).\n", profile.family_name);
                    } else {
                        out_println!(
                            ">> To execute live flash programming, add the --write flag.\n"
                        );
                    }
                    return Ok(());
                }

                jnl.record("destructive-start", &fw.metadata)?;
                let receipt = match crate::rx::flash_with_policy(
                    &mut s,
                    ready.slot,
                    &fw,
                    allow_unverified_model,
                    progress_reporter(),
                ) {
                    Ok(r) => r,
                    Err(e) if e.is::<crate::rx::RefusedBeforeErase>() => {
                        let e = journal_failure(&mut jnl, "refused-before-erase", e);
                        let restarted = return_after_rx_refusal(
                            &mut s,
                            &e,
                            before.slot,
                            recovery,
                            Some(&mut jnl),
                        );
                        return finish_operation(Err(e), restarted);
                    }
                    Err(e) => {
                        let e = journal_failure(&mut jnl, "uncertain", e);
                        err_println!("\n================================================================================");
                        err_println!("CRITICAL: FLASH WRITE INTERRUPTED. Motor may be in an unbootable recovery state.");
                        err_println!("DO NOT POWER CYCLE THE MOTOR.");
                        err_println!("Journal preserved at: {}", journal_path.display());
                        err_println!("To recover, re-run with:");
                        err_println!(
                            "  {}",
                            recovery_command(
                                "rx",
                                port,
                                slot,
                                serial_str,
                                &file,
                                &companion,
                                (!fw.metadata.encrypted)
                                    .then_some(fw.metadata.source_sha256.as_str()),
                                allow_unverified_model
                            )
                        );
                        err_println!("================================================================================\n");
                        return Err(e);
                    }
                };
                // The transfer has finished: a journal failure must not skip the
                // application return and verification below.
                let finished = jnl.record("finish-and-reset", &receipt);
                let after = match s
                    .normal_start(Some(before.slot))
                    .context("transfer finished, but the application did not return")
                {
                    Ok(after) => after,
                    Err(e) => return Err(journal_failure(&mut jnl, "verification-failed", e)),
                };
                let serial_check = check_application_serial(&mut s, after.slot, &serial);
                let stop = s.normal_stop();
                let journaled = finished.and_then(|()| jnl.record("application-after", &after));
                let verified = serial_check.and_then(|()| {
                    ensure!(
                        after.cargo == before.cargo
                            && after.dcas_version == d.metadata.version.to_string(),
                        "cargo or D version changed after RX update"
                    );
                    ensure!(
                        after.rx_version == fw.metadata.version.to_string(),
                        "post-flash RX version mismatch"
                    );
                    Ok(())
                });
                let verified = verified
                    .map_err(|e| journal_failure(&mut jnl, "verification-failed", e))
                    .and_then(|()| {
                        journaled.context(
                            "transfer finished and verified, but the flash journal is incomplete",
                        )
                    });
                finish_operation(verified, stop)?;
                jnl.record("verified", &after)
                    .context("flash verified, but the flash journal is incomplete")?;
                if is_json {
                    out_println!("{}", serde_json::to_string_pretty(&after)?);
                } else {
                    out_println!("Renesas flash programming completed successfully:");
                    out_println!("  Model Cargo: {}", after.cargo);
                    out_println!("  DCAS (D):    v{}", after.dcas_version);
                    out_println!("  Renesas (M): v{}", after.rx_version);
                }
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
    is_json: bool,
) -> Result<()> {
    match command {
        AdapterCommand::Status => {
            let mut s = Session::open(port.unwrap_or("auto"), trace)?;
            let power = s.initialize()?;
            if is_json {
                out_println!("{}", serde_json::to_string_pretty(&power)?);
            } else {
                print_adapter_status(&power);
            }
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
            if is_json {
                out_println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"power_stop_acknowledged":true,"motor_unresponsive_while_off":true,"application":after})
                    )?
                );
            } else {
                out_println!("Adapter power-cycle verified:");
                out_println!("  Power-Stop:  Acknowledged");
                out_println!("  Motor State: Offline while power removed");
                out_println!(
                    "  Application: Restored (Cargo: {}, DCAS: v{}, Renesas: v{})",
                    after.cargo,
                    after.dcas_version,
                    after.rx_version
                );
            }
            Ok(())
        }
    }
}

fn execute_ble(cli: Cli, is_json: bool) -> Result<()> {
    if matches!(
        cli.command,
        Command::Firmware {
            command: FirmwareCommand::Flash {
                command: FlashCommand::BlePair { .. },
                ..
            }
        }
    ) {
        return execute_ble_pair(cli, is_json);
    }

    ensure!(
        ble_command_allowed(&cli.command),
        "Command unavailable in a BLE settings session. Use wired connection for bootloader probes and recovery"
    );

    let routing_decision = plan_ble_routing(cli.ble_master, &cli.command);
    let passkey = read_ble_passkey(&cli)?;
    notice_println!("Experimental Bluetooth transport: hardware validation pending.");
    #[cfg(target_arch = "wasm32")]
    let cached = crate::wasm::take_ble_session_with_trace(cli.trace.as_deref())?;
    #[cfg(not(target_arch = "wasm32"))]
    let cached: Option<crate::ble::BleSession> = None;
    #[cfg(target_arch = "wasm32")]
    let routing_decision = if cached.as_ref().is_some_and(|s| s.is_master_routing())
        && routing_decision.policy == BleRoutingPolicy::Auto
    {
        BleRoutingDecision {
            route: "master",
            policy: BleRoutingPolicy::Auto,
            reason: "browser master routing retained until disconnect".into(),
        }
    } else {
        routing_decision
    };
    let reused = cached.is_some();
    let mut session = match cached {
        Some(session) => session,
        None => crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?,
    };
    let result = (|| -> Result<Option<serde_json::Value>> {
        if reused {
            #[cfg(target_arch = "wasm32")]
            session.resume_browser_command(routing_decision.is_master())?;
        } else {
            session.authenticate()?;
        }
        if !reused
            && !matches!(
                cli.command,
                Command::Ble {
                    command: BleCommand::TestLink
                }
            )
        {
            session.set_master_routing(routing_decision.is_master())?;
        }
        match cli.command {
            Command::Ble {
                command: BleCommand::TestLink,
            } => {
                let val = serde_json::json!({"authenticated":true,"protocol_reply_verified":true,"hardware_validated":false});
                if is_json {
                    Ok(Some(val))
                } else {
                    out_println!("Bluetooth Link Test: Authenticated and verified");
                    Ok(None)
                }
            }
            Command::Units => {
                let units = session.units()?;
                if is_json {
                    Ok(Some(serde_json::json!({
                        "units": units,
                        "ble_routing": routing_decision,
                    })))
                } else {
                    out_println!(
                        "BLE Routing: {} ({})",
                        routing_decision.route,
                        routing_decision.reason
                    );
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
                value["ble_routing"] = serde_json::to_value(&routing_decision)?;
                if is_json {
                    Ok(Some(value))
                } else {
                    out_println!(
                        "BLE Routing: {} ({})",
                        routing_decision.route,
                        routing_decision.reason
                    );
                    print_probe_card(
                        id.slot,
                        model[1],
                        model[2],
                        &id.cargo,
                        &serial,
                        &id.dcas_version,
                        &id.rx_version,
                    );
                    Ok(None)
                }
            }
            Command::Settings { command } => {
                let (id, serial) = ble_checked_start(
                    &mut session,
                    cli.slot,
                    cli.component.as_deref(),
                    cli.expect_serial.as_deref(),
                )?;
                let model = resolve_settings_model(&mut session, &id)?;
                validate_settings_preflight(&command, model, &serial)?;
                ensure!(
                    !(cli.no_auth && write_requires_unit_auth(&command, model, cli.unit_auth)),
                    "setting requires drive-unit authentication, but --no-auth was specified"
                );
                if command_requires_unit_auth(&command, model, cli.unit_auth) {
                    let _receipt = session.authenticate_unit_settings(&serial)?;
                }
                let mut val = execute_settings(&mut session, &id, &command, Some(&serial))?;
                if let Some(obj) = val.as_object_mut() {
                    obj.insert(
                        "ble_routing".to_string(),
                        serde_json::to_value(&routing_decision)?,
                    );
                }
                if is_json {
                    Ok(Some(val))
                } else {
                    out_println!(
                        "BLE Routing: {} ({})",
                        routing_decision.route,
                        routing_decision.reason
                    );
                    print_settings_result(&val, &command, model, false)?;
                    Ok(None)
                }
            }
            _ => anyhow::bail!("command is not available over Bluetooth"),
        }
    })();
    #[cfg(target_arch = "wasm32")]
    let result: Result<Option<serde_json::Value>> = match result {
        Ok(value) => match session.pause_browser_command() {
            Ok(()) => {
                crate::wasm::retain_ble_session(session);
                if let Some(value) = value {
                    out_println!("{}", serde_json::to_string_pretty(&value)?);
                }
                return Ok(());
            }
            Err(error) => Err(error),
        },
        Err(error) => Err(error),
    };
    let stopped = session.stop();
    let closed = session.close();
    let cleanup = finish_operation(
        stopped.context("BLE PC-link stop was not confirmed"),
        closed.context("BLE disconnect failed"),
    );
    finish_reporting(result, cleanup, |value| {
        if let Some(val) = value {
            out_println!("{}", serde_json::to_string_pretty(val)?);
        }
        Ok(())
    })
}

fn execute_ble_pair(cli: Cli, is_json: bool) -> Result<()> {
    ensure!(
        cli.ble_master != Some(BleRoutingPolicy::Normal),
        "flash ble-pair requires BLE master routing; --ble-master=normal is not supported for firmware flashing"
    );
    let Command::Firmware {
        command:
            FirmwareCommand::Flash {
                allow_unverified_model,
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

    let serial_str = cli.expect_serial.as_deref().ok_or_else(|| {
        anyhow::anyhow!("BLE firmware flashing requires --expect-serial <12-HEX>")
    })?;
    let serial: [u8; 6] = hex::decode(serial_str)?.try_into().unwrap();

    let d = Firmware::load(dcas)?;
    let m = Firmware::load(rx)?;
    ensure!(
        d.metadata.kind.is_dcas() && m.metadata.kind == FirmwareKind::Renesas,
        "flash ble-pair requires D and M images"
    );
    // A dry run applies the same model policy as the write it previews.
    let allow_model = *allow_unverified_model;
    if !allow_model {
        ensure!(
            d.metadata.series == 0x23 && d.metadata.unit == 0 && d.metadata.board == Some(1),
            "BLE pair updates support EP800 board 1 only without --allow-unverified-model"
        );
        ensure!(
            crate::firmware::EP800_BLE_TARGET_VERSIONS.contains(&d.metadata.version),
            "BLE pair updater requires a supported EP800 target version (4.1.11 or 4.2.0) without --allow-unverified-model"
        );
    } else {
        ensure!(
            crate::firmware::is_supported_rx_target(d.metadata.series, d.metadata.unit)
                && d.metadata
                    .board
                    .is_some_and(crate::firmware::is_allowed_motor_board),
            "BLE pair updates require a supported motor firmware family and board"
        );
    }
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
    if *write {
        ensure_plaintext_hash(
            &d,
            dcas,
            expect_dcas_sha256.as_deref(),
            "--expect-dcas-sha256",
        )?;
        ensure_plaintext_hash(&m, rx, expect_rx_sha256.as_deref(), "--expect-rx-sha256")?;
    }

    let backup_path = backup.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "backup-firmware-preupdate-{}-{}.json",
            serial_str,
            crate::platform::now_ms()
        ))
    });
    ensure!(!backup_path.exists(), "backup file already exists");

    let journal_path = journal.clone().unwrap_or_else(|| {
        PathBuf::from(format!(
            "cranklink-flash-blepair-{}-{}.journal.jsonl",
            serial_str,
            crate::platform::now_ms()
        ))
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
    #[cfg(target_arch = "wasm32")]
    let cached = crate::wasm::take_ble_session_with_trace(cli.trace.as_deref())?;
    #[cfg(not(target_arch = "wasm32"))]
    let cached: Option<crate::ble::BleSession> = None;
    let reused = cached.is_some();
    let mut session = match cached {
        Some(session) => session,
        None => crate::ble::open(cli.port.as_deref().unwrap(), cli.trace.as_deref(), passkey)?,
    };
    if !reused {
        session.set_master_routing(true)?;
    }

    let result = (|| -> Result<serde_json::Value> {
        if reused {
            #[cfg(target_arch = "wasm32")]
            session.resume_browser_command(true)?;
        } else {
            session.authenticate()?;
        }
        let (before, _) = ble_checked_motor_start(&mut session, cli.slot, Some(serial_str))?;
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

        let mut link = session.motor_firmware_with_policy(&serial, allow_model)?;
        // enter_rx resets a refusing RX bootloader; also restore normal routing.
        let rx_boot = match link.enter_rx() {
            Ok(boot) => boot,
            Err(e) => return finish_operation(Err(e), link.application_return()),
        };
        jnl.record("rx-boot-before", &rx_boot)?;
        link.reset_rx()?;
        link.application_return()?;
        let (checked, _) = ble_checked_motor_start(&mut session, None, Some(serial_str))?;
        ensure!(
            checked.cargo == before.cargo
                && checked.dcas_version == before.dcas_version
                && checked.rx_version == before.rx_version,
            "RX probe did not restore the unchanged application"
        );

        let mut link = session.motor_firmware_with_policy(&serial, allow_model)?;
        let boot = link.enter_dcas()?;
        jnl.record("dcas-boot-before", &boot)?;
        let guard = (|| -> Result<_> {
            ensure!(boot.serial == serial, "boot serial changed");
            d.validate_target_with_policy(
                boot.series,
                boot.unit,
                boot.board,
                Version(boot.version),
                allow_model,
            )
        })();
        if guard.is_err() || !*write {
            link.reset_dcas()?;
        }
        let profile = match guard {
            Ok(p) => p,
            Err(e) => {
                link.application_return()?;
                return Err(e);
            }
        };

        if !*write {
            link.application_return()?;
            jnl.record("dry-run-complete", &boot)?;
            let needs_opt_in =
                profile.verification_tier != crate::firmware::VerificationTier::HardwareVerified;
            if !is_json {
                out_println!("\n[DRY RUN - VALIDATION ONLY]");
                out_println!(
                    "- DCAS & RX Bootloader Handshakes: Verified ({})",
                    profile.family_name
                );
                out_println!("- Motor Serial: Verified ({})", serial_str);
                out_println!(
                    "- Pre-Update Settings Backup: Saved ({})",
                    backup_path.display()
                );
                out_println!("- DCAS Integrity: {}", integrity_label(&d.metadata));
                out_println!("- RX Integrity: {}", integrity_label(&m.metadata));
                out_println!("- Flash State: UNMODIFIED (No bytes written)");
                for (name, meta, expected, flag) in [
                    ("D", &d.metadata, expect_dcas_sha256, "--expect-dcas-sha256"),
                    ("M", &m.metadata, expect_rx_sha256, "--expect-rx-sha256"),
                ] {
                    if !meta.encrypted && expected.is_none() {
                        out_println!(">> Writing the plaintext {name} image also requires {flag} <SHA-256 of the file you verified>.");
                    }
                }
                if needs_opt_in {
                    out_println!(">> To execute live flash programming on {}, add --write (keeping --allow-unverified-model).\n", profile.family_name);
                } else {
                    out_println!(">> To execute live flash programming, add the --write flag.\n");
                }
            }
            return Ok(serde_json::json!({
                "transport": "ble",
                "write": false,
                "serial": serial_str,
                "dry_run": true,
                "family": profile.family_name,
                "verification_tier": profile.verification_tier,
                "backup": backup_path,
                "flash_state": "unmodified",
                // Each requires_* flag names an option --write must carry.
                "requires_expect_dcas_sha256": !d.metadata.encrypted,
                "requires_expect_rx_sha256": !m.metadata.encrypted,
                "requires_allow_unverified_model": needs_opt_in,
            }));
        }

        jnl.record("dcas-destructive-start", &d.metadata)?;
        let receipt = link.flash_dcas(&d.data, &boot, progress_reporter())?;
        jnl.record("dcas-finish-and-reset", &receipt)?;
        link.application_return()?;

        let incomplete_pair = before.rx_version != m.metadata.version.to_string();
        if !incomplete_pair {
            let (middle, _) = ble_checked_motor_start(&mut session, None, Some(serial_str))?;
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
            session.motor_firmware_with_policy(&serial, allow_model)?
        };
        let boot = match link.enter_rx() {
            Ok(boot) => boot,
            Err(e) => return finish_operation(Err(e), link.application_return()),
        };
        if boot.version != rx_boot.version
            || boot.capability_sequence != rx_boot.capability_sequence
        {
            // Nothing in M is erased yet; return RX and the gateway to the
            // application before reporting.
            let cleanup = link.reset_rx().and_then(|()| link.application_return());
            return finish_operation(
                Err(anyhow::anyhow!("RX boot capability changed before erase")),
                cleanup,
            );
        }
        jnl.record("rx-destructive-start", &m.metadata)?;
        let receipt = link.flash_rx(&m, boot, progress_reporter())?;
        jnl.record("rx-finish-and-reset", &receipt)?;
        link.application_return()?;

        let (after, _) = ble_checked_motor_start(&mut session, None, Some(serial_str))?;
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
            &SettingsCommand::Read {
                name: None,
                output: None,
            },
            None,
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

    let result = result.map_err(|e| journal_failure(&mut jnl, "uncertain", e));
    let stopped = session.stop();
    let closed = session.close();
    let result = finish_operation(result, stopped.context("BLE PC-link cleanup not confirmed"));
    let result = finish_operation(result, closed.context("BLE disconnect failed"));
    if result.is_ok() {
        jnl.record("cleanup-confirmed", &true)?;
    }
    let value = result?;
    if let Some(summary) = ble_pair_summary(&value, is_json)? {
        out_println!("{summary}");
    }
    Ok(())
}

/// The closing stdout of `flash ble-pair`. A dry run printed its text report
/// already and must never read as a flash.
fn ble_pair_summary(value: &serde_json::Value, is_json: bool) -> Result<Option<String>> {
    if is_json {
        return Ok(Some(serde_json::to_string_pretty(value)?));
    }
    if value.get("dry_run") == Some(&serde_json::Value::Bool(true)) {
        return Ok(None);
    }
    let app = value.get("application_after");
    let field = |name: &str| {
        app.and_then(|a| a.get(name))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    };
    Ok(Some(format!(
        "BLE firmware pair flashed successfully:\n  Model Cargo: {}\n  DCAS (D):    v{}\n  Renesas (M): v{}",
        field("cargo"),
        field("dcas_version"),
        field("rx_version")
    )))
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
        &SettingsCommand::Read {
            name: None,
            output: None,
        },
        None,
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
    ble_checked_serial(session, id, expected)
}

/// Firmware paths keep the motor gate even with an explicit slot.
fn ble_checked_motor_start(
    session: &mut crate::ble::BleSession,
    slot: Option<u8>,
    expected: Option<&str>,
) -> Result<(crate::device::Identity, String)> {
    let id = session.start(slot)?;
    ble_checked_serial(session, id, expected)
}

fn ble_checked_serial(
    session: &mut crate::ble::BleSession,
    id: crate::device::Identity,
    expected: Option<&str>,
) -> Result<(crate::device::Identity, String)> {
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

/// Report a completed operation even when its cleanup fails: a verified
/// write must still print its receipt and backup path, or a retry would back
/// up the already changed value as the original.
fn finish_reporting<T>(
    result: Result<T>,
    cleanup: Result<()>,
    report: impl FnOnce(&T) -> Result<()>,
) -> Result<()> {
    let value = match result {
        Ok(value) => value,
        Err(error) => return finish_operation(Err(error), cleanup),
    };
    finish_operation(
        report(&value),
        cleanup
            .context("the operation completed and its result is shown above, but cleanup failed"),
    )
}

fn read_only_command_allowed(command: &Command) -> bool {
    match command {
        Command::Scan { .. } | Command::Units | Command::Probe | Command::Session => true,
        // A settings dry run only reads; firmware dry runs enter bootloaders.
        Command::Settings { command } => matches!(
            command,
            SettingsCommand::Read { .. }
                | SettingsCommand::Schema { .. }
                | SettingsCommand::Set { dry_run: true, .. }
        ),
        Command::Firmware { command } => matches!(
            command,
            FirmwareCommand::Inspect { .. } | FirmwareCommand::DumpBootloader { .. }
        ),
        Command::Adapter { command } => matches!(command, AdapterCommand::Status),
        Command::Ble { command } => {
            matches!(command, BleCommand::Scan { .. } | BleCommand::TestLink)
        }
    }
}

/// Commands execute_ble implements; anything else is refused before connecting.
fn ble_command_allowed(command: &Command) -> bool {
    match command {
        Command::Probe | Command::Units => true,
        Command::Settings { .. } => true,
        Command::Firmware { command } => matches!(
            command,
            FirmwareCommand::Flash {
                command: FlashCommand::BlePair { .. },
                ..
            }
        ),
        Command::Ble { command } => matches!(command, BleCommand::TestLink),
        _ => false,
    }
}

fn parse_settings_model(name: &str) -> std::result::Result<String, String> {
    settings_model_name(name)
        .map(|model| model.slug().to_string())
        .map_err(|e| e.to_string())
}

fn settings_model_name(name: &str) -> Result<crate::settings::SettingsModel> {
    crate::settings::SettingsModel::from_name(name)
}

fn resolve_settings_model(
    session: &mut dyn crate::device::UnitCommands,
    id: &crate::device::Identity,
) -> Result<crate::settings::SettingsModel> {
    let identity =
        crate::device::UnitIdentity::from_reply(id.slot, session.model_identity(id.slot)?)?;
    if !identity.is_motor() {
        let resolved = crate::unit_models::read_identification(
            session,
            id.slot,
            identity.model_series,
            identity.model_unit,
        )?;
        let name = resolved
            .model
            .context("component settings require a unique catalogued model identity")?;
        return crate::settings::SettingsModel::from_name(name);
    }
    let params = crate::motor_support::read_params(session, id.slot, identity.model_series)?;
    crate::motor_support::for_motor_params(identity.model_series, identity.model_unit, &params)?
        .settings
        .context("named settings are not implemented for this motor")
}

fn plan_ble_routing(policy: Option<BleRoutingPolicy>, command: &Command) -> BleRoutingDecision {
    let resolved_policy = policy.unwrap_or(BleRoutingPolicy::Auto);
    match resolved_policy {
        BleRoutingPolicy::Master => BleRoutingDecision {
            route: "master",
            policy: resolved_policy,
            reason: "explicit --ble-master override".to_string(),
        },
        BleRoutingPolicy::Normal => BleRoutingDecision {
            route: "normal",
            policy: resolved_policy,
            reason: "explicit --ble-master=normal override".to_string(),
        },
        BleRoutingPolicy::Auto => match command {
            Command::Settings { command } => match command {
                SettingsCommand::Set {
                    name,
                    dry_run: false,
                    ..
                } if crate::settings::requires_ble_master_routing(name) => BleRoutingDecision {
                    route: "master",
                    policy: resolved_policy,
                    reason: format!(
                        "setting '{}' write requires PC-link owner zero (master routing)",
                        name
                    ),
                },
                SettingsCommand::Restore {
                    parsed: Some(parsed),
                    ..
                } if crate::settings::requires_ble_master_routing(&parsed.setting) => {
                    BleRoutingDecision {
                        route: "master",
                        policy: resolved_policy,
                        reason: format!(
                            "setting '{}' restore requires PC-link owner zero (master routing)",
                            parsed.setting
                        ),
                    }
                }
                SettingsCommand::Batch { operations, .. } => {
                    if let Some(setting) = operations.iter().find_map(|op| match op {
                        crate::settings::batch::Operation::Set { name, .. }
                            if crate::settings::requires_ble_master_routing(name) =>
                        {
                            Some(name.as_str())
                        }
                        crate::settings::batch::Operation::Restore {
                            parsed: Some(parsed),
                            ..
                        } if crate::settings::requires_ble_master_routing(&parsed.setting) => {
                            Some(parsed.setting.as_str())
                        }
                        _ => None,
                    }) {
                        BleRoutingDecision {
                            route: "master",
                            policy: resolved_policy,
                            reason: format!(
                                "batch includes '{}' write requiring PC-link owner zero (master routing)",
                                setting
                            ),
                        }
                    } else {
                        BleRoutingDecision {
                            route: "normal",
                            policy: resolved_policy,
                            reason: "batch operations do not require master routing".to_string(),
                        }
                    }
                }
                SettingsCommand::Set { dry_run: true, .. } => BleRoutingDecision {
                    route: "normal",
                    policy: resolved_policy,
                    reason: "dry-run validation uses ordinary BLE routing".to_string(),
                },
                _ => BleRoutingDecision {
                    route: "normal",
                    policy: resolved_policy,
                    reason: "operation uses ordinary BLE routing".to_string(),
                },
            },
            _ => BleRoutingDecision {
                route: "normal",
                policy: resolved_policy,
                reason: "operation uses ordinary BLE routing".to_string(),
            },
        },
    }
}

fn command_requires_unit_auth(
    command: &SettingsCommand,
    model: crate::settings::SettingsModel,
    explicit_unit_auth: bool,
) -> bool {
    !matches!(command, SettingsCommand::Set { dry_run: true, .. })
        && write_requires_unit_auth(command, model, explicit_unit_auth)
}

/// Whether the write `command` runs, or a dry run previews, needs drive-unit
/// authentication; a dry run skips it but refuses `--no-auth` as the write does.
fn write_requires_unit_auth(
    command: &SettingsCommand,
    model: crate::settings::SettingsModel,
    explicit_unit_auth: bool,
) -> bool {
    if explicit_unit_auth {
        return true;
    }
    let specs = crate::settings::schema_for_model(model);
    let is_oem_setting = |name: &str| {
        specs
            .iter()
            .any(|spec| spec.name.eq_ignore_ascii_case(name) && spec.oem)
    };
    match command {
        SettingsCommand::Set { name, .. } => is_oem_setting(name),
        SettingsCommand::Restore {
            parsed: Some(parsed),
            ..
        } => is_oem_setting(&parsed.setting),
        SettingsCommand::Batch { operations, .. } => operations.iter().any(|op| match op {
            crate::settings::batch::Operation::Set { name, .. } => is_oem_setting(name),
            crate::settings::batch::Operation::Restore {
                parsed: Some(parsed),
                ..
            } => is_oem_setting(&parsed.setting),
            _ => false,
        }),
        _ => false,
    }
}

/// Settings writes are bound to the unit the user named: auto-selection, a slot
/// or a model name cannot tell two bikes, or sibling components, apart.
/// Restores are already bound to the serial in their backup.
fn require_write_serial(command: &Command, expect_serial: Option<&str>) -> Result<()> {
    let writes = match command {
        Command::Settings {
            command: SettingsCommand::Set { dry_run, .. },
        } => !dry_run,
        Command::Settings {
            command: SettingsCommand::Batch { operations, .. },
        } => operations
            .iter()
            .any(|op| matches!(op, crate::settings::batch::Operation::Set { .. })),
        _ => false,
    };
    ensure!(
        !writes || expect_serial.is_some(),
        "settings writes require -s/--expect-serial <SERIAL>; read the serial with probe first"
    );
    Ok(())
}

fn validate_settings_preflight(
    command: &SettingsCommand,
    model: crate::settings::SettingsModel,
    expected_serial: &str,
) -> Result<()> {
    let specs = crate::settings::schema_for_model(model);
    match command {
        SettingsCommand::Set { name, value, .. } => {
            let spec = specs
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "setting {name} is unsupported for {}; use model settings schema",
                        model.name()
                    )
                })?;
            crate::settings::validate_input(spec, value)?;
        }
        SettingsCommand::Restore { file, parsed, .. } => {
            let parsed_owned;
            let parsed_ref = match parsed {
                Some(p) => p,
                None => {
                    parsed_owned = crate::settings::parse_restore_file(file)?;
                    &parsed_owned
                }
            };
            parsed_ref.validate_for_model(model, expected_serial)?;
        }
        SettingsCommand::Batch { operations, .. } => {
            for op in operations {
                match op {
                    crate::settings::batch::Operation::Set { name, value } => {
                        let spec = specs
                            .iter()
                            .find(|s| s.name.eq_ignore_ascii_case(name))
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "setting {name} is unsupported for {}; use model settings schema",
                                    model.name()
                                )
                            })?;
                        crate::settings::validate_input(spec, value)?;
                    }
                    crate::settings::batch::Operation::Restore { file, parsed } => {
                        let parsed_owned;
                        let parsed_ref = match parsed {
                            Some(p) => p,
                            None => {
                                parsed_owned =
                                    crate::settings::parse_restore_file(Path::new(file))?;
                                &parsed_owned
                            }
                        };
                        parsed_ref.validate_for_model(model, expected_serial)?;
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// `verified_serial` is the serial checked when the session started; every
/// set and batch write re-checks it right before its SET.
fn execute_settings(
    session: &mut dyn crate::device::UnitCommands,
    id: &crate::device::Identity,
    command: &SettingsCommand,
    verified_serial: Option<&str>,
) -> Result<serde_json::Value> {
    let model = resolve_settings_model(session, id)?;
    let serial_raw = session.command(id.slot, 1, 0x3c, &[0], 0x3e);
    let serial = serial_raw
        .ok()
        .and_then(|r| crate::device::serial_bytes(&r).ok())
        .map(hex::encode);
    let serial_str = serial.clone().unwrap_or_else(|| format!("slot{}", id.slot));

    match command {
        SettingsCommand::Read { name, output } => {
            let v = if let Some(name) = name {
                serde_json::to_value(crate::settings::read_for_model(
                    session, id.slot, name, model,
                )?)?
            } else {
                let mut snapshot = serde_json::to_value(crate::settings::read_all_for_model(
                    session, id.slot, model,
                ))?;
                snapshot["capabilities"] = crate::settings::capabilities_for_model(model);
                // Include the identity verified in this session so browser
                // callers can load settings without a separate probe connection.
                let mut identity = serde_json::to_value(id)?;
                identity["serial"] = serde_json::json!(serial);
                identity["model_series"] = serde_json::json!(model.series());
                identity["settings_model"] = serde_json::json!(model.slug());
                let (model_name, model_unit) =
                    if let crate::settings::SettingsModel::Component(name) = model {
                        let entry = crate::unit_models::UNIT_MODELS
                            .iter()
                            .find(|entry| entry.name == name)
                            .expect("catalog model");
                        (name, entry.unit)
                    } else {
                        let name =
                            crate::motor_support::read_params(session, id.slot, model.series())
                                .ok()
                                .and_then(|params| {
                                    crate::unit_models::identify(
                                        model.series(),
                                        0,
                                        None,
                                        Some(&params),
                                    )
                                });
                        (name.unwrap_or(model.name()), 0)
                    };
                identity["model_name"] = serde_json::json!(model_name);
                identity["model_unit"] = serde_json::json!(model_unit);
                snapshot["identity"] = identity;
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
            if *dry_run {
                return crate::settings::dry_run_for_model(session, id.slot, name, value, model);
            }
            let backup_path = backup.clone().unwrap_or_else(|| {
                PathBuf::from(format!(
                    "backup-{}-{}-{}.json",
                    name.replace('.', "_"),
                    serial_str,
                    crate::platform::now_ms()
                ))
            });
            let receipt = crate::settings::set_for_model_bound(
                session,
                id.slot,
                name,
                value,
                &backup_path,
                model,
                verified_serial,
            )?;
            let mut v = serde_json::to_value(receipt)?;
            v["backup_file"] = serde_json::json!(backup_path);
            Ok(v)
        }
        SettingsCommand::Restore {
            file,
            backup,
            parsed,
        } => {
            let backup_path = backup.clone().unwrap_or_else(|| {
                PathBuf::from(format!(
                    "backup-restore-{}-{}.json",
                    serial_str,
                    crate::platform::now_ms()
                ))
            });
            let receipt = if let Some(parsed) = parsed {
                crate::settings::restore_parsed_for_model(
                    session,
                    id.slot,
                    parsed,
                    &backup_path,
                    model,
                )?
            } else {
                crate::settings::restore_for_model(session, id.slot, file, &backup_path, model)?
            };
            let mut v = serde_json::to_value(receipt)?;
            v["current_backup_file"] = serde_json::json!(backup_path);
            Ok(v)
        }
        SettingsCommand::Batch {
            operations,
            backup_prefix,
            ..
        } => {
            let prefix = backup_prefix
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("batch-{}", crate::platform::now_ms())));
            crate::settings::batch::run_bound(
                session,
                id.slot,
                operations,
                &prefix,
                model,
                verified_serial,
            )
        }
        SettingsCommand::Schema { .. } => unreachable!(),
    }
}

/// Compare the application's serial with -s: before entering any bootloader,
/// so a wrong motor is never switched into boot mode, and again after a flash.
fn check_application_serial(session: &mut Session, slot: u8, expected: &[u8]) -> Result<()> {
    let live = crate::device::serial_bytes(&session.command(slot, 1, 0x3c, &[0], 0x3e)?)?;
    ensure!(
        live.as_slice() == expected,
        "live serial {} differs from --expect-serial {}",
        hex::encode(live),
        hex::encode(expected)
    );
    Ok(())
}

/// Reset the DCAS bootloader entered from the application and confirm the
/// application came back unchanged, with the same restart the write path uses.
/// Until it answers, the motor may remain in its bootloader at `before.slot`.
fn verify_application_return(
    session: &mut Session,
    port: Option<&str>,
    slot: Option<u8>,
    before: &crate::device::Identity,
    step: &str,
) -> Result<crate::device::Identity> {
    let after = session
        .boot_reset()
        .context("bootloader reset not acknowledged")
        .and_then(|()| session.normal_start(slot))
        .with_context(|| format!("application did not return after {step}"));
    let after = match after {
        Ok(after) => after,
        Err(e) => {
            err_println!(
                "{}",
                bootloader_guidance(
                    port,
                    before.slot,
                    &format!("The application did not return after the {step}")
                )
            );
            return Err(e);
        }
    };
    session.normal_stop()?;
    ensure!(
        after.cargo == before.cargo
            && after.dcas_version == before.dcas_version
            && after.rx_version == before.rx_version,
        "application changed after {step}"
    );
    Ok(after)
}

/// Return to the application after `rx::RefusedBeforeErase`, as the dry run
/// does; nothing was erased, so the refusal is not an interrupted write.
fn return_after_rx_refusal(
    session: &mut Session,
    refusal: &anyhow::Error,
    slot: u8,
    recovery: bool,
    jnl: Option<&mut Journal>,
) -> Result<()> {
    let reset = refusal
        .downcast_ref::<crate::rx::RefusedBeforeErase>()
        .is_some_and(|r| r.reset.is_ok());
    if !reset {
        err_println!("Nothing was erased, but the RX boot bridge did not confirm its return to the application.");
        err_println!(
            "Check the application with `cranklink firmware probe --target rx` before retrying."
        );
        return Ok(());
    }
    let after = start_rx_session(session, Some(slot), recovery)
        .context("RX refused before erase; application restart failed")?;
    session
        .normal_stop()
        .context("RX refused before erase; PC-link stop failed")?;
    if let Some(jnl) = jnl {
        jnl.record("application-after", &after)?;
    }
    Ok(())
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

fn print_probe_card(
    slot: u8,
    model_series: u8,
    model_unit: u8,
    cargo: &str,
    serial: &str,
    dcas_version: &str,
    rx_version: &str,
) {
    let mut cargo_bytes = cargo.as_bytes().to_vec();
    while cargo_bytes.len() < 7 {
        cargo_bytes.push(0);
    }
    let model_candidates = crate::unit_models::candidates(model_series, model_unit, None, None);
    // Component sessions report their resolved catalog model in `cargo`.
    if model_candidates.contains(&cargo) {
        out_println!("Component Probe:");
        out_println!("  Model:       {} (Slot {})", cargo, slot);
        out_println!("  Serial:      {}", serial);
        out_println!("  Application: v{}", dcas_version.trim_start_matches('v'));
        return;
    }
    let resolved = crate::unit_models::identify(model_series, model_unit, None, Some(&cargo_bytes));
    let model_name = if let Some(m) = resolved {
        m.to_string()
    } else if !model_candidates.is_empty() {
        model_candidates.join(" / ")
    } else {
        "Unknown".to_string()
    };
    let dcas_str = if dcas_version.starts_with('v') {
        dcas_version.to_string()
    } else {
        format!("v{dcas_version}")
    };
    let rx_str = if rx_version.starts_with('v') || rx_version.starts_with("unavail") {
        rx_version.to_string()
    } else {
        format!("v{rx_version}")
    };

    out_println!("Drive Unit Probe:");
    out_println!("  Model:       {} (Slot {})", model_name, slot);
    out_println!("  Serial:      {}", serial);
    out_println!("  Cargo ID:    {}", cargo);
    out_println!("  DCAS (D):    {}", dcas_str);
    out_println!("  Renesas (M): {}", rx_str);
}

fn destination_label(val: i64) -> Option<&'static str> {
    match val {
        0 => Some("Japan / Region 0"),
        1 => Some("Europe / Australia"),
        2 => Some("Japan / 24 km/h"),
        3 => Some("Europe / Restricted"),
        4 => Some("Australia"),
        5 => Some("US Class 3 (45 km/h)"),
        _ => None,
    }
}

fn format_setting_value(
    name: &str,
    val: &serde_json::Value,
    specs: &[crate::settings::SettingSpec],
) -> String {
    if val.is_null() {
        return "unset".to_string();
    }
    let spec = specs.iter().find(|s| s.name.eq_ignore_ascii_case(name));
    if let Some(spec) = spec {
        if name.eq_ignore_ascii_case("destination") {
            if let Some(n) = val.as_i64() {
                if let Some(lbl) = destination_label(n) {
                    return format!("{n} ({lbl})");
                }
            }
        }
        if name.eq_ignore_ascii_case("lamp") {
            if let Some(n) = val.as_i64() {
                let lbl = match n {
                    0 => "off",
                    1 => "on",
                    2 => "auto",
                    _ => "",
                };
                if !lbl.is_empty() {
                    return format!("{n} ({lbl})");
                }
            }
        }
        if spec.unit == "bool" {
            if let Some(b) = val.as_bool() {
                return if b {
                    "1 (enabled)".to_string()
                } else {
                    "0 (disabled)".to_string()
                };
            } else if let Some(n) = val.as_i64() {
                return if n != 0 {
                    format!("{n} (enabled)")
                } else {
                    format!("{n} (disabled)")
                };
            }
        }
        if let Some(s) = val.as_str() {
            return s.to_string();
        }
        let is_clean_unit = matches!(
            spec.unit.as_str(),
            "Nm" | "mm"
                | "km/h"
                | "rpm"
                | "W"
                | "km"
                | "teeth"
                | "gears"
                | "minutes"
                | "degrees"
                | "s"
                | "%"
        );
        if is_clean_unit {
            return format!("{val} {}", spec.unit);
        }
        if spec.unit == "0.1 percent" {
            return format!("{val} %");
        }
    }
    match val {
        serde_json::Value::Null => "unset".to_string(),
        serde_json::Value::String(s) => s.clone(),
        _ => val.to_string(),
    }
}

fn print_setting_schema(spec: &crate::settings::SettingSpec) {
    out_println!("Setting: {}", spec.name);
    let unit_desc = if !spec.choices.is_empty() {
        format!(
            "{} (choices: {})",
            spec.unit,
            spec.choices
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        spec.unit.clone()
    };
    out_println!("  Unit:       {}", unit_desc);
    if spec.scale != 1.0 && spec.scale != 0.0 {
        out_println!(
            "  Range:      {} .. {}",
            spec.min as f64 / spec.scale,
            spec.max as f64 / spec.scale
        );
    } else {
        out_println!("  Range:      {} .. {}", spec.min, spec.max);
    }
    let oem_desc = if spec.oem {
        "Protected (requires drive-unit authentication)"
    } else {
        "Standard (unprotected)"
    };
    out_println!("  OEM:        {}", oem_desc);
    if spec.read_only {
        out_println!("  Access:     Read only");
    }
}

fn print_settings_schema_table(specs: &[crate::settings::SettingSpec]) {
    out_println!("Available Settings ({} defined):\n", specs.len());
    out_println!(
        "  {:<32}  {:<12}  {:<14}  {}",
        "NAME",
        "UNIT",
        "RANGE",
        "PROTECTION"
    );
    for s in specs {
        let range_str = if s.scale != 1.0 && s.scale != 0.0 {
            format!("{} .. {}", s.min as f64 / s.scale, s.max as f64 / s.scale)
        } else {
            format!("{} .. {}", s.min, s.max)
        };
        let prot = if s.read_only {
            "Read only"
        } else if s.oem {
            "OEM Protected"
        } else {
            "Standard"
        };
        out_println!(
            "  {:<32}  {:<12}  {:<14}  {}",
            s.name,
            s.unit,
            range_str,
            prot
        );
    }
}

fn print_settings_result(
    value: &serde_json::Value,
    command: &SettingsCommand,
    model: crate::settings::SettingsModel,
    is_json: bool,
) -> Result<()> {
    if is_json {
        out_println!("{}", serde_json::to_string_pretty(value)?);
        return Ok(());
    }

    let specs = crate::settings::schema_for_model(model);
    match command {
        SettingsCommand::Read { name, output } => {
            if let Some(target) = name {
                let val = value.get("value").unwrap_or(value);
                let formatted = format_setting_value(target, val, &specs);
                out_println!("{}: {}", target, formatted);
            } else {
                out_println!("Drive Unit Settings ({}):\n", model.name());
                if let Some(values_obj) = value.get("values").and_then(|v| v.as_object()) {
                    let mut names: Vec<_> = values_obj.keys().collect();
                    names.sort();
                    for n in names {
                        if let Some(val_obj) = values_obj.get(n) {
                            let val = val_obj.get("value").unwrap_or(&serde_json::Value::Null);
                            let formatted = format_setting_value(n, val, &specs);
                            out_println!("  {:<32} {}", format!("{n}:"), formatted);
                        }
                    }
                }
                if let Some(unavail_obj) = value.get("unavailable").and_then(|u| u.as_object()) {
                    if !unavail_obj.is_empty() {
                        out_println!("\nUnavailable Settings ({}):", unavail_obj.len());
                        let mut unavail_names: Vec<_> = unavail_obj.keys().collect();
                        unavail_names.sort();
                        for n in unavail_names {
                            if let Some(reason) = unavail_obj.get(n).and_then(|r| r.as_str()) {
                                out_println!("  {:<32} {}", format!("{n}:"), reason);
                            }
                        }
                    }
                }
            }
            if let Some(path) = output {
                out_println!("\nSettings snapshot saved to: {}", path.display());
            }
        }
        SettingsCommand::Set { name, dry_run, .. } => {
            if *dry_run {
                let curr = value
                    .get("current_value")
                    .unwrap_or(&serde_json::Value::Null);
                let planned = value
                    .get("planned_value")
                    .unwrap_or(&serde_json::Value::Null);
                let curr_fmt = format_setting_value(name, curr, &specs);
                let planned_fmt = format_setting_value(name, planned, &specs);
                out_println!("[DRY RUN - VALIDATION ONLY]");
                out_println!("Setting: {}", name);
                out_println!("  Current:  {}", curr_fmt);
                out_println!("  Planned:  {}", planned_fmt);
                out_println!("  State:    UNMODIFIED (No bytes written)");
                out_println!(">> To write this change, re-run without the --dry-run flag.");
            } else {
                let setting = value
                    .get("setting")
                    .and_then(|s| s.as_str())
                    .unwrap_or(name);
                let before = value.get("before").unwrap_or(&serde_json::Value::Null);
                let after = value.get("after").unwrap_or(&serde_json::Value::Null);
                let backup = value
                    .get("backup_file")
                    .and_then(|b| b.as_str())
                    .or_else(|| value.get("backup").and_then(|b| b.as_str()))
                    .unwrap_or("");
                let before_fmt = format_setting_value(setting, before, &specs);
                let after_fmt = format_setting_value(setting, after, &specs);
                out_println!("Setting updated: {}", setting);
                out_println!("  Previous: {}", before_fmt);
                out_println!("  New:      {}", after_fmt);
                if !backup.is_empty() {
                    out_println!("  Backup:   {}", backup);
                }
            }
        }
        SettingsCommand::Restore { file, .. } => {
            let backup = value
                .get("current_backup_file")
                .and_then(|b| b.as_str())
                .unwrap_or("");
            out_println!("Settings restored from backup:");
            out_println!("  Source: {}", file.display());
            if !backup.is_empty() {
                out_println!("  Pre-restore Backup: {}", backup);
            }
        }
        SettingsCommand::Batch { .. } => {
            out_println!("Settings batch operations completed successfully.");
        }
        SettingsCommand::Schema { .. } => unreachable!(),
    }
    Ok(())
}

fn print_firmware_metadata(
    meta: &crate::firmware::FirmwareMetadata,
    decode_to: Option<&std::path::Path>,
) {
    let kind_str = match meta.kind {
        FirmwareKind::DcasX => "DCASX",
        FirmwareKind::Dcas2i => "DCAS2i",
        FirmwareKind::Dcas2 => "DCAS2",
        FirmwareKind::Renesas => "Renesas",
    };
    let board_str = match meta.board {
        Some(b) => format!(
            "Board {} (Series 0x{:02X}, Unit 0x{:02X})",
            b, meta.series, meta.unit
        ),
        None => format!("Series 0x{:02X}, Unit 0x{:02X}", meta.series, meta.unit),
    };
    out_println!("Firmware Image Metadata:");
    out_println!("  Kind:                {}", kind_str);
    out_println!(
        "  Version:             v{} (Compatibility: v{})",
        meta.version,
        meta.compatibility_version
    );
    out_println!("  Target Board/Series: {}", board_str);
    out_println!("  Checksum:            0x{:02X}", meta.checksum);
    out_println!("  Integrity:           {}", integrity_label(meta));
    out_println!("  Source SHA-256:      {}", meta.source_sha256);
    if let Some(out) = decode_to {
        out_println!("  Decoded Output:      {}", out.display());
    }
}

fn print_adapter_status(power: &crate::device::PowerInfo) {
    let bus_power = if power.adapter_supply {
        "Active (Powered)"
    } else {
        "Inactive (External Battery)"
    };
    out_println!("SM-PCE02 Adapter:");
    out_println!("  Bus Power: {}", bus_power);
    out_println!("  Raw State: 0x{:02X}", power.raw_state);
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

/// Journal a failed step; a journal error must never replace it.
fn journal_failure(jnl: &mut Journal, stage: &str, error: anyhow::Error) -> anyhow::Error {
    match jnl.record(stage, &format!("{error:#}")) {
        Ok(()) => error,
        Err(j) => error.context(format!("journal entry '{stage}' was not written: {j:#}")),
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn serial_sanitization_accepts_clean_and_formatted_serials() {
        assert_eq!(sanitize_serial("010203040506").unwrap(), "010203040506");
        assert_eq!(
            sanitize_serial("01-02-03-04-05-06").unwrap(),
            "010203040506"
        );
        assert_eq!(
            sanitize_serial("01:02:03:04:05:06").unwrap(),
            "010203040506"
        );
        assert_eq!(sanitize_serial("A1B2C3D4E5F6").unwrap(), "a1b2c3d4e5f6");
        assert!(sanitize_serial("short").is_err());
        assert!(sanitize_serial("01020304050G").is_err());
    }

    #[test]
    fn application_serial_is_checked_before_bootloader_entry() {
        use crate::device::tests::{frame, scripted};
        let query = frame(0x48, &[13, 1, 0x3c, 0]);
        let reply = frame(0x48, &[0, 1, 0x3e, 1, 2, 3, 4, 5, 6, 0xff]);
        let (mut session, steps) = scripted(vec![(query.clone(), reply.clone())]);
        check_application_serial(&mut session, 13, &[1, 2, 3, 4, 5, 6]).unwrap();
        assert!(steps.borrow().is_empty());
        let (mut session, _) = scripted(vec![(query, reply)]);
        let error = check_application_serial(&mut session, 13, &[9, 9, 9, 9, 9, 9]).unwrap_err();
        assert!(format!("{error:#}").contains("differs from --expect-serial"));
    }

    /// The DCAS bootloader reset, then discovery and a PC-link start at slot
    /// 13 reading cargo 00-CRG, D 4.2.0 and the given M version, then the stop.
    fn application_return_script(
        rx: [u8; 3],
    ) -> Vec<(crate::protocol::Frame, crate::protocol::Frame)> {
        use crate::device::tests::frame;
        let idle = frame(0x88, &[0x70, 0, 0, 0x78]);
        let mut script = vec![
            (frame(0x88, &[0x28, 0, 0, 0]), frame(0x88, &[0x31])),
            (frame(3, &[0x20]), frame(0x23, &[0])),
            (frame(4, &[]), frame(0x24, &[0xa0])),
            (frame(0x1d, &[0]), frame(0x3d, &[0, 0])),
            (frame(0x1b, &[]), frame(0x3b, &[0, 0x20, 0, 0])),
            (frame(6, &[13]), frame(0x26, &[0])),
            (
                frame(0x48, &[13, 1, 0x1c, 0]),
                frame(0x48, &[0, 1, 0x1e, 0x23, 0]),
            ),
            (frame(4, &[]), frame(0x24, &[0xe0])),
            (frame(0x48, &[13, 0x32, 0x10, 1, 0, 0, 0]), idle.clone()),
        ];
        for (index, key) in [
            [0xa2, 0x2b],
            [0x30, 0x0e],
            [0x7a, 0x4d],
            [0x62, 0x2b],
            [0x85, 0xb4],
        ]
        .into_iter()
        .enumerate()
        {
            script.push((
                frame(0x48, &[13, 0x32, 0x30, key[0], key[1], 0, 0]),
                if index == 4 {
                    frame(0x48, &[0, 0x32, 0x12, 1])
                } else {
                    idle.clone()
                },
            ));
        }
        script.extend([
            (
                frame(0x48, &[13, 0x16, 0x7c, 0]),
                frame(
                    0x48,
                    &[0, 0x16, 0x7e, b'0', b'0', b'-', b'C', b'R', b'G', 0],
                ),
            ),
            (
                frame(0x48, &[13, 1, 0x2c, 0]),
                frame(0x48, &[0, 1, 0x2e, 0x42, 0, 0]),
            ),
            (
                frame(0x48, &[13, 1, 0x84, 1]),
                frame(0x48, &[0, 1, 0x86, rx[0], rx[1], rx[2]]),
            ),
            (
                frame(0x48, &[13, 0x32, 0x10, 0, 0, 0, 0]),
                frame(0x48, &[0, 0x32, 0x12, 0]),
            ),
        ]);
        script
    }

    #[test]
    fn dcas_dry_run_requires_the_same_application_back() {
        use crate::device::tests::scripted;
        let before = crate::device::Identity {
            slot: 13,
            cargo: "00-CRG".into(),
            dcas_version: "4.2.0.0".into(),
            rx_version: "4.1.11.0".into(),
        };
        let (mut session, steps) = scripted(application_return_script([0x41, 11, 0]));
        let after =
            verify_application_return(&mut session, None, Some(13), &before, "DCAS dry run")
                .unwrap();
        assert_eq!(after.rx_version, before.rx_version);
        assert!(steps.borrow().is_empty());
        // A different M version after the reset is not the same application.
        let (mut session, _) = scripted(application_return_script([0x41, 10, 0]));
        let error =
            verify_application_return(&mut session, None, Some(13), &before, "DCAS dry run")
                .unwrap_err();
        assert!(
            format!("{error:#}").contains("application changed after DCAS dry run"),
            "{error:#}"
        );
        // No motor answers discovery after the reset: it may still be in boot.
        let mut script = application_return_script([0x41, 11, 0]);
        script.truncate(5);
        script[4].1 = crate::device::tests::frame(0x3b, &[0, 0, 0, 0]);
        let (mut session, steps) = scripted(script);
        let error =
            verify_application_return(&mut session, None, Some(13), &before, "DCAS dry run")
                .unwrap_err();
        assert!(
            format!("{error:#}").contains("application did not return after DCAS dry run"),
            "{error:#}"
        );
        assert!(steps.borrow().is_empty());
    }

    #[test]
    fn ble_pair_dry_run_needs_the_opt_in_for_unverified_models() {
        // Synthetic plaintext EP600 (30/00) D and M images; refused offline.
        let mut d = vec![0xff; 128];
        d[16..19].copy_from_slice(&[0x44, 0, 0]);
        d[24..28].copy_from_slice(&128u32.to_le_bytes());
        d[40..44].copy_from_slice(&[0x30, 0, 4, 1]);
        d[47..50].copy_from_slice(&[0x44, 0, 0]);
        d[126..].copy_from_slice(b"et");
        let mut m = vec![0xff; 128];
        m[..4].copy_from_slice(&128u32.to_le_bytes());
        m[8..12].copy_from_slice(&[0x44, 0, 0, 1]);
        m[14..19].copy_from_slice(&[0x30, 0, 0x44, 0, 0]);
        m[126..].copy_from_slice(b"et");
        let dir = std::env::temp_dir().join(format!(
            "cranklink-ble-pair-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (d_path, m_path) = (dir.join("D.bin"), dir.join("M.bin"));
        std::fs::write(&d_path, d).unwrap();
        std::fs::write(&m_path, m).unwrap();
        // An absent simulator socket: never a radio scan, and the opt-in run
        // fails at connect with its journal and backup kept in `dir`.
        let port = format!("ble-sim:{}", dir.join("absent.sock").display());
        let args = |extra: &[&str]| {
            let mut args = vec![
                "cranklink".to_string(),
                "--port".into(),
                port.clone(),
                "-s".into(),
                "010203040506".into(),
                "firmware".into(),
                "flash".into(),
            ];
            args.extend(extra.iter().map(|s| s.to_string()));
            args.extend([
                "ble-pair".into(),
                "--dcas".into(),
                d_path.display().to_string(),
                "--rx".into(),
                m_path.display().to_string(),
                "--journal".into(),
                dir.join("pair.jsonl").display().to_string(),
                "--backup".into(),
                dir.join("pair.json").display().to_string(),
            ]);
            args
        };
        let error = execute(args(&[])).unwrap_err();
        assert!(
            format!("{error:#}").contains("without --allow-unverified-model"),
            "{error:#}"
        );
        // With the opt-in it gets past the model policy (and then fails to connect).
        let error = execute(args(&["--allow-unverified-model"])).unwrap_err();
        assert!(
            !format!("{error:#}").contains("without --allow-unverified-model"),
            "{error:#}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn ble_pair_dry_run_never_reports_a_flash() {
        let dry_run = serde_json::json!({"transport": "ble", "write": false, "dry_run": true});
        // Text mode printed the dry-run report already; nothing may follow it.
        assert_eq!(ble_pair_summary(&dry_run, false).unwrap(), None);
        let json = ble_pair_summary(&dry_run, true).unwrap().unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&json).unwrap(),
            dry_run
        );
        let written = serde_json::json!({
            "write": true,
            "application_after": {"cargo": "00-CRG", "dcas_version": "4.2.0.0", "rx_version": "4.1.11.0"},
        });
        assert_eq!(
            ble_pair_summary(&written, false).unwrap().unwrap(),
            "BLE firmware pair flashed successfully:\n  Model Cargo: 00-CRG\n  \
             DCAS (D):    v4.2.0.0\n  Renesas (M): v4.1.11.0"
        );
    }

    #[test]
    fn ble_pair_refuses_an_unanalysed_ep800_target_offline() {
        // Synthetic plaintext EP800 (23/00) D4.3.0: live BLE entry does not
        // admit 4.3.0, so the pair would fail after writing D.
        let mut d = vec![0xff; 128];
        d[16..19].copy_from_slice(&[0x43, 0, 0]);
        d[24..28].copy_from_slice(&128u32.to_le_bytes());
        d[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
        d[47..50].copy_from_slice(&[0x41, 11, 0]);
        d[126..].copy_from_slice(b"et");
        let mut m = vec![0xff; 128];
        m[..4].copy_from_slice(&128u32.to_le_bytes());
        m[8..12].copy_from_slice(&[0x41, 11, 0, 1]);
        m[14..19].copy_from_slice(&[0x23, 0, 0x41, 11, 0]);
        m[126..].copy_from_slice(b"et");
        let dir = std::env::temp_dir().join(format!(
            "cranklink-ble-target-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (d_path, m_path) = (dir.join("D.bin"), dir.join("M.bin"));
        std::fs::write(&d_path, d).unwrap();
        std::fs::write(&m_path, m).unwrap();
        let port = format!("ble-sim:{}", dir.join("absent.sock").display());
        // A regressed gate gets further; keep its journal and backup in `dir`.
        let error = execute(
            [
                "cranklink",
                "--port",
                &port,
                "-s",
                "010203040506",
                "firmware",
                "flash",
                "ble-pair",
                "--dcas",
                &d_path.display().to_string(),
                "--rx",
                &m_path.display().to_string(),
                "--journal",
                &dir.join("pair.jsonl").display().to_string(),
                "--backup",
                &dir.join("pair.json").display().to_string(),
            ]
            .map(String::from)
            .to_vec(),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("supported EP800 target version"),
            "{error:#}"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plaintext_images_are_written_only_with_a_verified_hash() {
        // Synthetic plaintext EP800 (23/00) D4.2.0 and M4.1.11: hardware-verified
        // model, so only the missing container checksum can refuse them offline.
        let mut d = vec![0xff; 128];
        d[16..19].copy_from_slice(&[0x42, 0, 0]);
        d[24..28].copy_from_slice(&128u32.to_le_bytes());
        d[40..44].copy_from_slice(&[0x23, 0, 4, 1]);
        d[47..50].copy_from_slice(&[0x41, 11, 0]);
        d[126..].copy_from_slice(b"et");
        let mut m = vec![0xff; 128];
        m[..4].copy_from_slice(&128u32.to_le_bytes());
        m[8..12].copy_from_slice(&[0x41, 11, 0, 1]);
        m[14..19].copy_from_slice(&[0x23, 0, 0x41, 11, 0]);
        m[126..].copy_from_slice(b"et");
        let (d_sha, m_sha) = (
            Firmware::from_bytes(&d).unwrap().metadata.source_sha256,
            Firmware::from_bytes(&m).unwrap().metadata.source_sha256,
        );
        let dir = std::env::temp_dir().join(format!(
            "cranklink-integrity-gate-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let (d_path, m_path) = (dir.join("D.bin"), dir.join("M.bin"));
        std::fs::write(&d_path, d).unwrap();
        std::fs::write(&m_path, m).unwrap();
        let file = |path: &Path| path.display().to_string();
        // The journal opens after every offline check and just before the
        // transport, whose socket does not exist; it shows how far a run got.
        let flash = |port: &str, mut command: Vec<String>, journal: &str| {
            let journal = dir.join(journal);
            command.extend(["--journal".into(), file(&journal)]);
            let mut args: Vec<String> = [
                "cranklink",
                "--port",
                port,
                "-s",
                "010203040506",
                "firmware",
                "flash",
            ]
            .map(String::from)
            .to_vec();
            args.extend(command);
            (
                format!("{:#}", execute(args).unwrap_err()),
                journal.exists(),
            )
        };

        let sim = format!("sim:{}", file(&dir.join("absent.sock")));
        for (target, image, companion, sha) in [
            ("dcas", &d_path, &m_path, d_sha.as_str()),
            ("rx", &m_path, &d_path, m_sha.as_str()),
        ] {
            let wired = |extra: &[&str], journal: &str| {
                let mut command = vec![
                    target.to_string(),
                    file(image),
                    "--companion".into(),
                    file(companion),
                ];
                command.extend(extra.iter().map(|s| s.to_string()));
                flash(&sim, command, &format!("{target}-{journal}.jsonl"))
            };
            let (error, journaled) = wired(&["--write"], "unbound");
            assert!(
                error.contains("plaintext") && error.contains("--expect-sha256") && !journaled,
                "{error}"
            );
            // A dry run, or a write bound to the file's hash, passes the gate.
            for (extra, journal) in [
                (&[][..], "dry-run"),
                (&["--write", "--expect-sha256", sha][..], "bound"),
            ] {
                let (error, journaled) = wired(extra, journal);
                assert!(journaled && !error.contains("plaintext"), "{error}");
            }
        }

        let ble = format!("ble-sim:{}", file(&dir.join("absent-ble.sock")));
        let pair = |extra: &[&str], name: &str| {
            let mut command = vec![
                "ble-pair".to_string(),
                "--dcas".into(),
                file(&d_path),
                "--rx".into(),
                file(&m_path),
                "--backup".into(),
                file(&dir.join(format!("pair-{name}.json"))),
                "--write".into(),
            ];
            command.extend(extra.iter().map(|s| s.to_string()));
            flash(&ble, command, &format!("pair-{name}.jsonl"))
        };
        let (error, journaled) = pair(&[], "unbound");
        assert!(
            error.contains("--expect-dcas-sha256") && !journaled,
            "{error}"
        );
        let (error, journaled) = pair(&["--expect-dcas-sha256", &d_sha], "d-only");
        assert!(
            error.contains("--expect-rx-sha256") && !journaled,
            "{error}"
        );
        let (error, journaled) = pair(
            &["--expect-dcas-sha256", &d_sha, "--expect-rx-sha256", &m_sha],
            "bound",
        );
        assert!(journaled && !error.contains("plaintext"), "{error}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn firmware_pair_start_keeps_the_motor_gate_for_an_explicit_slot() {
        use crate::ble::tests::component_mock;
        // A derailleur answers at slot 4: settings may start it, firmware may not.
        let (mut session, _) = component_mock(2, 4);
        assert!(ble_checked_start(&mut session, Some(4), None, None).is_ok());
        let (mut session, state) = component_mock(2, 4);
        assert!(ble_checked_motor_start(&mut session, Some(4), None).is_err());
        assert!(!state
            .borrow()
            .writes
            .iter()
            .any(|(_, bytes)| bytes.get(1..4) == Some(&[0x32, 0x10, 1][..])));
    }

    #[test]
    fn recovery_command_keeps_the_failing_port_and_slot() {
        let (d, m) = (Path::new("D.dat"), Path::new("M.dat"));
        assert_eq!(
            recovery_command(
                "dcas",
                Some("COM3"),
                Some(13),
                "010203040506",
                d,
                m,
                None,
                true
            ),
            "cranklink --port COM3 --slot 13 -s 010203040506 firmware flash dcas D.dat \
             --companion M.dat --recovery --write --allow-unverified-model"
        );
        // A plaintext image is written only with its hash; the rerun must carry it.
        let command = recovery_command("rx", None, None, "010203040506", m, d, Some("ab12"), false);
        assert_eq!(
            command,
            "cranklink -s 010203040506 firmware flash rx M.dat --companion D.dat \
             --expect-sha256 ab12 --recovery --write"
        );
        let spaced = recovery_command(
            "dcas",
            Some("COM3"),
            Some(13),
            "010203040506",
            Path::new("C:\\Users\\First Last\\D's.dat"),
            m,
            None,
            true,
        );
        assert!(
            spaced.contains("'C:\\Users\\First Last\\D'\\''s.dat'"),
            "{spaced}"
        );
        // Every hint must parse, as a POSIX shell splits it, into the command it describes.
        fn shell_split(line: &str) -> Vec<String> {
            let (mut args, mut current, mut quoted, mut chars) =
                (Vec::new(), String::new(), false, line.chars().peekable());
            while let Some(c) = chars.next() {
                match c {
                    '\'' => quoted = !quoted,
                    '\\' if !quoted => current.extend(chars.next()),
                    ' ' if !quoted => args.push(std::mem::take(&mut current)),
                    _ => current.push(c),
                }
            }
            args.push(current);
            args
        }
        for line in [command, spaced] {
            let cli = Cli::try_parse_from(shell_split(&line)).unwrap();
            assert!(matches!(cli.command, Command::Firmware { .. }), "{line}");
        }
    }

    #[test]
    fn bootloader_guidance_probes_before_resetting_the_retained_slot() {
        let guidance = bootloader_guidance(Some("usb"), 13, "Bootloader entry did not complete");
        assert!(
            guidance
                .starts_with("Bootloader entry did not complete; nothing was erased or written"),
            "{guidance}"
        );
        // The read-only application probe comes before the recovery reset.
        let commands: Vec<&str> = guidance
            .lines()
            .filter_map(|line| line.strip_prefix("  "))
            .collect();
        assert_eq!(
            commands,
            [
                "cranklink --port usb probe",
                "cranklink --port usb --slot 13 firmware probe --target dcas --recovery --reset"
            ]
        );
        let cli = Cli::try_parse_from(commands[0].split(' ')).unwrap();
        assert!(matches!(cli.command, Command::Probe));
        let cli = Cli::try_parse_from(commands[1].split(' ')).unwrap();
        assert_eq!(cli.slot, Some(13));
        // 31 is the recovery default, and the auto port is the default port.
        let guidance = bootloader_guidance(None, 31, "Bootloader entry did not complete");
        assert!(guidance.contains("\n  cranklink probe\n"), "{guidance}");
        assert!(guidance.ends_with("\n  cranklink firmware probe --target dcas --recovery --reset"));
    }

    #[test]
    fn a_failed_uncertain_journal_entry_keeps_the_flash_error() {
        let path = std::env::temp_dir().join(format!(
            "cranklink-uncertain-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        let mut jnl = Journal::new(&path).unwrap();
        let error = journal_failure(&mut jnl, "uncertain", anyhow::anyhow!("flash write NAK"));
        assert_eq!(format!("{error:#}"), "flash write NAK");
        drop(jnl);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains(r#""stage":"uncertain""#));
        // A journal that rejects the write must not hide the flash error.
        let mut jnl = Journal(std::fs::OpenOptions::new().read(true).open(&path).unwrap());
        let error = format!(
            "{:#}",
            journal_failure(&mut jnl, "uncertain", anyhow::anyhow!("flash write NAK"))
        );
        assert!(
            error.contains("flash write NAK") && error.contains("was not written"),
            "{error}"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn offline_commands_accept_a_bluetooth_port_with_its_options() {
        // Offline commands never open the port, so its passkey must not fail them.
        let error = execute(
            [
                "cranklink",
                "--port",
                "ble:AA",
                "--passkey",
                "123456",
                "firmware",
                "inspect",
                "/nonexistent/fw.dat",
            ]
            .map(String::from)
            .to_vec(),
        )
        .unwrap_err();
        assert!(
            !format!("{error:#}").contains("require a Bluetooth port"),
            "{error:#}"
        );
    }

    #[test]
    fn ble_sessions_accept_only_commands_execute_ble_implements() {
        let command = |args: &[&str]| {
            let mut full = vec!["cranklink"];
            full.extend_from_slice(args);
            Cli::try_parse_from(full).unwrap().command
        };
        for args in [
            &["probe"][..],
            &["units"],
            &["ble", "test-link"],
            &["settings", "read"],
        ] {
            assert!(ble_command_allowed(&command(args)), "{args:?}");
        }
        for args in [&["session"][..], &["ble", "scan"], &["adapter", "status"]] {
            assert!(!ble_command_allowed(&command(args)), "{args:?}");
        }
    }

    #[test]
    fn usage_errors_are_not_double_prefixed() {
        let error = execute(
            ["cranklink", "--port", "a", "--port", "b", "scan"]
                .map(String::from)
                .to_vec(),
        )
        .unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.starts_with("the argument '--port <PORT>'"),
            "{message}"
        );
        let error = execute(vec!["cranklink".into()]).unwrap_err();
        assert_eq!(format!("{error:#}"), "a command is required");
    }

    #[test]
    fn settings_writes_require_an_expected_serial() {
        use crate::settings::batch::Operation;
        let command = |args: &[&str]| {
            let mut full = vec!["cranklink"];
            full.extend_from_slice(args);
            Cli::try_parse_from(full).unwrap().command
        };
        let set = command(&["settings", "set", "switch-a-function", "1"]);
        assert!(require_write_serial(&set, None).is_err());
        assert!(require_write_serial(&set, Some("010203040506")).is_ok());
        for args in [
            &["settings", "set", "lamp", "1", "--dry-run"][..],
            &["settings", "read"],
            &["settings", "restore", "backup.json"],
            &["probe"],
        ] {
            assert!(
                require_write_serial(&command(args), None).is_ok(),
                "{args:?}"
            );
        }
        let batch = |operations| Command::Settings {
            command: SettingsCommand::Batch {
                plan: PathBuf::from("plan.json"),
                operations,
                backup_prefix: None,
            },
        };
        let reads = batch(vec![Operation::Read { name: None }]);
        assert!(require_write_serial(&reads, None).is_ok());
        let writes = batch(vec![Operation::Set {
            name: "lamp".into(),
            value: "1".into(),
        }]);
        assert!(require_write_serial(&writes, None).is_err());
        // Rejected offline, before any transport opens (unless the documented
        // CRANKLINK_SERIAL is exported, which satisfies the requirement).
        if std::env::var_os("CRANKLINK_SERIAL").is_some() {
            return;
        }
        let error = execute(
            [
                "cranklink",
                "--port",
                "sim:/nonexistent",
                "settings",
                "set",
                "lamp",
                "1",
            ]
            .map(String::from)
            .to_vec(),
        )
        .unwrap_err();
        assert!(
            format!("{error:#}").contains("--expect-serial"),
            "{error:#}"
        );
    }

    #[test]
    fn read_only_rejects_mutating_commands() {
        for args in [
            vec!["settings", "set", "lamp", "1"],
            vec!["settings", "restore", "backup.json"],
            vec![
                "firmware",
                "flash",
                "dcas",
                "fw.dat",
                "--companion",
                "comp.dat",
            ],
            vec!["adapter", "power-cycle"],
            vec!["firmware", "probe", "--target", "dcas"],
        ] {
            let mut command = vec!["cranklink", "--read-only"];
            command.extend(args);
            let error = execute(command.into_iter().map(String::from).collect()).unwrap_err();
            assert!(
                error.to_string().contains("--read-only rejects"),
                "{error:#}"
            );
        }

        // Read-only allows discovery and reads
        for args in [
            vec!["scan"],
            vec!["units"],
            vec!["probe"],
            vec!["settings", "read"],
            vec!["settings", "schema", "--model", "ep800"],
            vec!["settings", "set", "lamp", "1", "--dry-run"],
            vec!["firmware", "inspect", "fw.dat"],
        ] {
            let mut command = vec!["cranklink", "--read-only"];
            command.extend(args);
            let cli = Cli::try_parse_from(command.clone()).unwrap();
            assert!(read_only_command_allowed(&cli.command), "{command:?}");
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
    fn component_schema_models_resolve_without_motor_fallback() {
        let cli =
            Cli::try_parse_from(["cranklink", "settings", "schema", "--model", "st6870l"]).unwrap();
        match cli.command {
            Command::Settings {
                command: SettingsCommand::Schema { model, .. },
            } => {
                assert_eq!(model, "ST-6870-L");
                assert_eq!(
                    settings_model_name(&model).unwrap(),
                    crate::settings::SettingsModel::Component("ST-6870-L")
                );
            }
            _ => panic!("expected component schema"),
        }
        assert!(
            Cli::try_parse_from(["cranklink", "settings", "schema", "--model", "ST-UNKNOWN"])
                .is_err()
        );
        assert!(settings_model_name("typo").is_err());
    }

    #[test]
    fn offline_component_and_motor_schemas_do_not_open_browser_ble_transport() {
        for (model, setting) in [("ST-6870-L", "switch-a-function"), ("ep801", "lamp")] {
            execute(
                [
                    "cranklink",
                    "--port",
                    "webble",
                    "--json",
                    "settings",
                    "schema",
                    setting,
                    "--model",
                    model,
                ]
                .into_iter()
                .map(String::from)
                .collect(),
            )
            .unwrap();
        }
    }

    #[test]
    fn component_model_and_slot_can_be_combined_for_live_identity_validation() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "--slot",
            "13",
            "--component",
            "MU-UR500",
            "settings",
            "read",
        ])
        .unwrap();
        assert_eq!(cli.slot, Some(13));
        assert_eq!(cli.component.as_deref(), Some("MU-UR500"));
    }

    #[test]
    fn signed_setting_values_parse_without_consuming_option_flags() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "settings",
            "set",
            "hub-adjustment",
            "-3",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Command::Settings {
                command: SettingsCommand::Set { value, dry_run, .. },
            } => {
                assert_eq!(value, "-3");
                assert!(dry_run);
            }
            _ => panic!("expected signed setting"),
        }
        assert!(
            Cli::try_parse_from(["cranklink", "settings", "set", "hub-adjustment", "--typo"])
                .is_err()
        );
    }

    #[test]
    fn live_component_settings_use_part_identity_without_motor_queries() {
        struct Lever;
        impl crate::device::UnitCommands for Lever {
            fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
                assert_eq!(slot, 4);
                Ok(vec![4, 7, 1])
            }
            fn command(
                &mut self,
                slot: u8,
                group: u8,
                command: u8,
                params: &[u8],
                response: u8,
            ) -> Result<Vec<u8>> {
                assert_eq!(
                    (slot, group, command, params, response),
                    (4, 4, 0x0c, &[0][..], 0x0e)
                );
                Ok(vec![1])
            }
        }
        let id = crate::device::Identity {
            slot: 4,
            cargo: "ST-6870-L".into(),
            dcas_version: "3.0.0.0".into(),
            rx_version: "not applicable".into(),
        };
        assert_eq!(
            resolve_settings_model(&mut Lever, &id).unwrap(),
            crate::settings::SettingsModel::Component("ST-6870-L")
        );
    }

    #[test]
    fn full_component_snapshot_retains_the_live_model_and_unit_number() {
        struct PowerUnit;
        impl crate::device::UnitCommands for PowerUnit {
            fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
                Ok(vec![slot, 2, 0])
            }
            fn command(
                &mut self,
                _: u8,
                group: u8,
                command: u8,
                _: &[u8],
                _: u8,
            ) -> Result<Vec<u8>> {
                match (group, command) {
                    (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                    (0x0b, 0x64) => Ok(vec![2, 255]),
                    _ => panic!("unexpected motor/component query {group:02X}/{command:02X}"),
                }
            }
        }
        let id = crate::device::Identity {
            slot: 4,
            cargo: "SM-BMR1".into(),
            dcas_version: "3.0.0.0".into(),
            rx_version: "not applicable".into(),
        };
        let snapshot = execute_settings(
            &mut PowerUnit,
            &id,
            &SettingsCommand::Read {
                name: None,
                output: None,
            },
            None,
        )
        .unwrap();
        assert_eq!(snapshot["identity"]["model_name"], "SM-BMR1");
        assert_eq!(snapshot["identity"]["model_series"], 2);
        assert_eq!(snapshot["identity"]["model_unit"], 0);
        assert_eq!(snapshot["identity"]["settings_model"], "SM-BMR1");
        assert_eq!(snapshot["capabilities"]["settings_available"], true);
        assert_eq!(snapshot["values"]["multi-shift-limit.up"]["value"], 2);
    }

    #[test]
    fn a_completed_operation_is_reported_even_when_cleanup_fails() {
        let mut seen = None;
        let error = finish_reporting(Ok(5), Err(anyhow::anyhow!("stop")), |value| {
            seen = Some(*value);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(seen, Some(5));
        let message = format!("{error:#}");
        assert!(
            message.contains("completed") && message.contains("stop"),
            "{message}"
        );

        let mut reports = 0;
        finish_reporting(Ok(5), Ok(()), |_| {
            reports += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(reports, 1);
        let error = finish_reporting(Err::<i32, _>(anyhow::anyhow!("write")), Ok(()), |_| {
            panic!("a failed operation has no result to report")
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "write");
    }

    #[test]
    fn component_dry_run_rejects_an_original_assignment_outside_restore_choices() {
        struct Lever;
        impl crate::device::UnitCommands for Lever {
            fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
                Ok(vec![slot, 2, 1])
            }
            fn command(
                &mut self,
                _: u8,
                group: u8,
                command: u8,
                _: &[u8],
                _: u8,
            ) -> Result<Vec<u8>> {
                match (group, command) {
                    (1, 0x3c) => Ok(vec![1, 2, 3, 4, 5, 6]),
                    (4, 0x0c) => Ok(vec![1]),
                    (4, 0x14) => Ok(vec![0xa3]),
                    _ => panic!("unexpected command in dry run"),
                }
            }
        }
        let id = crate::device::Identity {
            slot: 4,
            cargo: "ST-6770-L".into(),
            dcas_version: "3.0.0.0".into(),
            rx_version: "not applicable".into(),
        };
        let error = execute_settings(
            &mut Lever,
            &id,
            &SettingsCommand::Set {
                name: "switch-a-function".into(),
                value: "1".into(),
                backup: None,
                dry_run: true,
            },
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("backup cannot be restored"));
    }

    #[test]
    fn settings_writes_recheck_the_serial_verified_at_session_start() {
        struct Lever;
        impl crate::device::UnitCommands for Lever {
            fn model_identity(&mut self, slot: u8) -> Result<Vec<u8>> {
                Ok(vec![slot, 2, 1])
            }
            fn command(
                &mut self,
                _: u8,
                group: u8,
                command: u8,
                _: &[u8],
                _: u8,
            ) -> Result<Vec<u8>> {
                match (group, command) {
                    // Another lever now answers on the verified unit's slot.
                    (1, 0x3c) => Ok(vec![9; 6]),
                    (4, 0x0c) => Ok(vec![1]),
                    (4, 0x14) => Ok(vec![0x23, 0xaf]),
                    _ => panic!("unexpected command {group:02x}/{command:02x}"),
                }
            }
        }
        let id = crate::device::Identity {
            slot: 4,
            cargo: "ST-6770-L".into(),
            dcas_version: "3.0.0.0".into(),
            rx_version: "not applicable".into(),
        };
        let prefix = std::env::temp_dir().join(format!(
            "cranklink-cli-bound-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        let backup = prefix.with_extension("json");
        let operations = serde_json::from_value(serde_json::json!([
            {"op":"set","name":"switch-a-function","value":1}
        ]))
        .unwrap();
        for command in [
            SettingsCommand::Set {
                name: "switch-a-function".into(),
                value: "1".into(),
                backup: Some(backup.clone()),
                dry_run: false,
            },
            SettingsCommand::Batch {
                plan: PathBuf::new(),
                backup_prefix: Some(prefix.clone()),
                operations,
            },
        ] {
            let error =
                execute_settings(&mut Lever, &id, &command, Some("010203040506")).unwrap_err();
            assert!(format!("{error:#}").contains("refusing write"), "{error:#}");
        }
        assert!(!backup.exists());
        assert!(!PathBuf::from(format!("{}.journal.jsonl", prefix.display())).exists());
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
                        allow_unverified_model,
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
                assert!(!allow_unverified_model);
                assert_eq!(file, PathBuf::from("dcas.dat"));
                assert_eq!(companion, PathBuf::from("rx.dat"));
                assert!(expect_sha256.is_none());
                assert!(journal.is_none());
                assert!(!write); // default is dry-run!
            }
            _ => panic!("expected Firmware Flash Dcas"),
        }
    }

    fn bridge_args(extra: &[&str]) -> Vec<String> {
        let mut args = vec![
            "cranklink",
            "-s",
            "010203040506",
            "firmware",
            "flash",
            "dcas",
            "missing-d.dat",
            "--companion",
            "missing-m.dat",
        ];
        args.extend_from_slice(extra);
        args.into_iter().map(String::from).collect()
    }

    #[test]
    fn bridge_recovery_options_are_explicit_and_bounded() {
        let cli = Cli::try_parse_from(bridge_args(&[
            "--bridge-recovery",
            "--attempts",
            "3",
            "--countdown",
            "0",
        ]))
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::Firmware {
                command: FirmwareCommand::Flash {
                    command: FlashCommand::Dcas {
                        bridge_recovery: true,
                        recovery: false,
                        attempts: Some(3),
                        countdown: Some(0),
                        write: false,
                        ..
                    },
                    ..
                }
            }
        ));
        for extra in [
            vec!["--bridge-recovery", "--recovery"],
            vec!["--attempts", "3"],
            vec!["--countdown", "0"],
            vec!["--bridge-recovery", "--attempts", "0"],
            vec!["--bridge-recovery", "--attempts", "21"],
            vec!["--bridge-recovery", "--countdown", "301"],
        ] {
            assert!(
                Cli::try_parse_from(bridge_args(&extra)).is_err(),
                "{extra:?}"
            );
        }
    }

    #[test]
    fn bootloader_dump_is_read_only_and_rejects_unsupported_transport_before_output() {
        let cli = Cli::try_parse_from([
            "cranklink",
            "--read-only",
            "firmware",
            "dump-bootloader",
            "--output",
            "not-created",
        ])
        .unwrap();
        assert!(read_only_command_allowed(&cli.command));
        for (extra, expected) in [
            (vec!["--port", "sim:/not-opened"], "native SM-PCE02 USB"),
            (vec!["--port", "ble:dummy"], "Use wired connection"),
            (vec!["--slot", "13"], "selects a unique motor"),
            (vec!["--trace", "not-created.jsonl"], "saves its wire trace"),
        ] {
            let mut args: Vec<String> = [
                "cranklink",
                "firmware",
                "dump-bootloader",
                "--output",
                "not-created",
            ]
            .into_iter()
            .map(String::from)
            .collect();
            args.extend(extra.into_iter().map(String::from));
            let error = execute(args).unwrap_err();
            assert!(format!("{error:#}").contains(expected), "{error:#}");
        }
    }

    #[test]
    fn bridge_recovery_accepts_unknown_serial_and_validates_files_offline() {
        let mut args = bridge_args(&["--bridge-recovery"]);
        args.drain(1..3); // No -s: read the motor serial after the recovery keys.
        let cli = Cli::try_parse_from(&args).unwrap();
        assert!(cli.expect_serial.is_none());
        // A missing file must fail in offline validation, before opening USB;
        // absence of -s must not fail first or trigger identity probing.
        let error = execute(args).unwrap_err();
        assert!(format!("{error:#}").contains("missing-d.dat"), "{error:#}");
    }

    #[test]
    fn bridge_recovery_rejects_unsupported_context_before_loading_or_connecting() {
        for (extra, expected) in [
            (
                vec!["--bridge-recovery", "--read-only"],
                "--read-only rejects",
            ),
            (vec!["--bridge-recovery", "--slot", "13"], "slot 31"),
            (
                vec!["--bridge-recovery", "--allow-unverified-model"],
                "documented EP800",
            ),
            (
                vec!["--bridge-recovery", "--port", "ble:dummy"],
                "Use wired connection",
            ),
        ] {
            let error = execute(bridge_args(&extra)).unwrap_err();
            assert!(
                format!("{error:#}").contains(expected),
                "{extra:?}: {error:#}"
            );
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

    #[test]
    fn setting_value_formatting() {
        let specs = crate::settings::schema_for_model(crate::settings::SettingsModel::Ep800Crg);
        assert_eq!(
            format_setting_value("destination", &serde_json::json!(1), &specs),
            "1 (Europe / Australia)"
        );
        assert_eq!(
            format_setting_value("destination", &serde_json::json!(0), &specs),
            "0 (Japan / Region 0)"
        );
        assert_eq!(
            format_setting_value("profile1.boost.torque", &serde_json::json!(85), &specs),
            "85 Nm"
        );
        assert_eq!(
            format_setting_value("tire-circumference", &serde_json::json!(2040), &specs),
            "2040 mm"
        );
        assert_eq!(
            format_setting_value("lamp", &serde_json::json!(1), &specs),
            "1 (on)"
        );
        assert_eq!(
            format_setting_value("lamp", &serde_json::json!(0), &specs),
            "0 (off)"
        );
        assert_eq!(
            format_setting_value("maintenance-alert", &serde_json::json!(1), &specs),
            "1 (enabled)"
        );
        assert_eq!(
            format_setting_value("maintenance-alert", &serde_json::json!(0), &specs),
            "0 (disabled)"
        );
        assert_eq!(
            format_setting_value("maintenance-date", &serde_json::Value::Null, &specs),
            "unset"
        );
        assert_eq!(
            format_setting_value("profile1.name", &serde_json::json!("Profile1"), &specs),
            "Profile1"
        );
    }

    #[test]
    fn ble_routing_and_auth_planning_separates_master_ownership_and_oem_auth() {
        let model = crate::settings::SettingsModel::Ep800Crg;
        let make_parsed = |setting: &str, before: serde_json::Value, raw_hex: &str| {
            crate::settings::ParsedRestore {
                serial: "010203040506".into(),
                setting: setting.into(),
                before: before.clone(),
                original_parameters: hex::decode(raw_hex).unwrap(),
                raw_backup: serde_json::json!({
                    "format": "cranklink-setting-backup-v1",
                    "serial": "010203040506",
                    "slot": 13,
                    "setting": setting,
                    "before": before,
                    "original_parameters": raw_hex
                }),
            }
        };

        // 1. destination and tire-circumference require BOTH master routing and drive-unit authentication.
        for (setting, val, raw_hex) in [
            ("destination", serde_json::json!(1), "0101ffff"),
            ("tire-circumference", serde_json::json!(2200), "98080000"),
        ] {
            let set_cmd = SettingsCommand::Set {
                name: setting.into(),
                value: val.to_string(),
                backup: None,
                dry_run: false,
            };
            let decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: set_cmd.clone(),
                },
            );
            assert_eq!(decision.route, "master");
            assert_eq!(decision.policy, BleRoutingPolicy::Auto);
            assert!(decision.reason.contains(setting));
            assert!(command_requires_unit_auth(&set_cmd, model, false));

            let restore_cmd = SettingsCommand::Restore {
                file: PathBuf::from("backup.json"),
                backup: None,
                parsed: Some(make_parsed(setting, val.clone(), raw_hex)),
            };
            let restore_decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: restore_cmd.clone(),
                },
            );
            assert_eq!(restore_decision.route, "master");
            assert!(restore_decision.reason.contains(setting));
            assert!(command_requires_unit_auth(&restore_cmd, model, false));
        }

        // 2. lamp and lighting-time require master routing WITHOUT drive-unit authentication.
        for (setting, val, raw_hex) in [
            ("lamp", serde_json::json!(1), "01"),
            ("lighting-time", serde_json::json!(30), "1e00ffff"),
        ] {
            let set_cmd = SettingsCommand::Set {
                name: setting.into(),
                value: val.to_string(),
                backup: None,
                dry_run: false,
            };
            let decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: set_cmd.clone(),
                },
            );
            assert_eq!(decision.route, "master");
            assert!(!command_requires_unit_auth(&set_cmd, model, false));

            let restore_cmd = SettingsCommand::Restore {
                file: PathBuf::from("backup.json"),
                backup: None,
                parsed: Some(make_parsed(setting, val.clone(), raw_hex)),
            };
            let restore_decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: restore_cmd.clone(),
                },
            );
            assert_eq!(restore_decision.route, "master");
            assert!(!command_requires_unit_auth(&restore_cmd, model, false));
        }

        // 3. walk-assist and light-output require drive-unit authentication WITHOUT master routing.
        for (setting, val, raw_hex) in [
            ("walk-assist", serde_json::json!(2), "02"),
            ("light-output", serde_json::json!(200), "c800ffff"),
        ] {
            let set_cmd = SettingsCommand::Set {
                name: setting.into(),
                value: val.to_string(),
                backup: None,
                dry_run: false,
            };
            let decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: set_cmd.clone(),
                },
            );
            assert_eq!(decision.route, "normal");
            assert!(command_requires_unit_auth(&set_cmd, model, false));

            let restore_cmd = SettingsCommand::Restore {
                file: PathBuf::from("backup.json"),
                backup: None,
                parsed: Some(make_parsed(setting, val.clone(), raw_hex)),
            };
            let restore_decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: restore_cmd.clone(),
                },
            );
            assert_eq!(restore_decision.route, "normal");
            assert!(command_requires_unit_auth(&restore_cmd, model, false));
        }

        // 4. speed-limit and assist-pattern use normal routing and do not require drive-unit authentication.
        for (setting, val, raw_hex) in [
            ("speed-limit", serde_json::json!(25), "c409ffff"),
            ("assist-pattern", serde_json::json!(5), "05000000"),
        ] {
            let set_cmd = SettingsCommand::Set {
                name: setting.into(),
                value: val.to_string(),
                backup: None,
                dry_run: false,
            };
            let decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: set_cmd.clone(),
                },
            );
            assert_eq!(decision.route, "normal");
            assert!(!command_requires_unit_auth(&set_cmd, model, false));

            let restore_cmd = SettingsCommand::Restore {
                file: PathBuf::from("backup.json"),
                backup: None,
                parsed: Some(make_parsed(setting, val.clone(), raw_hex)),
            };
            let restore_decision = plan_ble_routing(
                None,
                &Command::Settings {
                    command: restore_cmd.clone(),
                },
            );
            assert_eq!(restore_decision.route, "normal");
            assert!(!command_requires_unit_auth(&restore_cmd, model, false));
        }

        // 5. Mixed destination + speed-limit batch uses one master-routed, authenticated session.
        let batch_cmd = SettingsCommand::Batch {
            plan: PathBuf::from("batch.json"),
            backup_prefix: None,
            operations: vec![
                crate::settings::batch::Operation::Set {
                    name: "speed-limit".into(),
                    value: "25".into(),
                },
                crate::settings::batch::Operation::Restore {
                    file: "dest.json".into(),
                    parsed: Some(make_parsed("destination", serde_json::json!(1), "0101ffff")),
                },
            ],
        };
        let batch_decision = plan_ble_routing(
            None,
            &Command::Settings {
                command: batch_cmd.clone(),
            },
        );
        assert_eq!(batch_decision.route, "master");
        assert!(batch_decision.reason.contains("destination"));
        assert!(command_requires_unit_auth(&batch_cmd, model, false));

        // 6. Reads, probe, units, and dry runs use normal routing and suppress auto-auth.
        let dry_run_cmd = SettingsCommand::Set {
            name: "destination".into(),
            value: "3".into(),
            backup: None,
            dry_run: true,
        };
        let dry_decision = plan_ble_routing(
            None,
            &Command::Settings {
                command: dry_run_cmd.clone(),
            },
        );
        assert_eq!(dry_decision.route, "normal");
        assert!(dry_decision.reason.contains("dry-run"));
        assert!(!command_requires_unit_auth(&dry_run_cmd, model, true));
        // The dry run still refuses --no-auth where the write would.
        assert!(write_requires_unit_auth(&dry_run_cmd, model, false));
        assert_eq!(plan_ble_routing(None, &Command::Probe).route, "normal");
        assert_eq!(plan_ble_routing(None, &Command::Units).route, "normal");
    }

    #[test]
    fn ble_master_flag_parsing_overrides_and_conflicts() {
        let default_cli = Cli::try_parse_from(["cranklink", "probe"]).unwrap();
        assert_eq!(default_cli.ble_master, None);

        for args in [
            vec!["cranklink", "--ble-master", "probe"],
            vec!["cranklink", "--ble-master=master", "probe"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert_eq!(cli.ble_master, Some(BleRoutingPolicy::Master));
            let decision = plan_ble_routing(cli.ble_master, &cli.command);
            assert_eq!(decision.route, "master");
            assert_eq!(decision.reason, "explicit --ble-master override");
        }

        for args in [vec![
            "cranklink",
            "--ble-master=normal",
            "settings",
            "set",
            "destination",
            "3",
        ]] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert_eq!(cli.ble_master, Some(BleRoutingPolicy::Normal));
            let decision = plan_ble_routing(cli.ble_master, &cli.command);
            assert_eq!(decision.route, "normal");
            assert_eq!(decision.reason, "explicit --ble-master=normal override");
        }

        for args in [vec!["cranklink", "--ble-master=auto", "probe"]] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert_eq!(cli.ble_master, Some(BleRoutingPolicy::Auto));
        }

        // Conflict: --unit-auth and --no-auth
        let conflict = execute(
            ["cranklink", "--unit-auth", "--no-auth", "probe"]
                .into_iter()
                .map(String::from)
                .collect(),
        )
        .unwrap_err();
        assert!(
            conflict
                .to_string()
                .contains("--unit-auth and --no-auth cannot be used together"),
            "{conflict:#}"
        );

        // Conflict: --ble-master on non-BLE port
        let non_ble = execute(
            [
                "cranklink",
                "--port",
                "/dev/cu.usbserial",
                "--ble-master",
                "probe",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        )
        .unwrap_err();
        assert!(
            non_ble
                .to_string()
                .contains("--ble-master requires a Bluetooth port"),
            "{non_ble:#}"
        );

        // Conflict: --ble-master=normal on firmware flash ble-pair
        let flash_normal = execute(
            [
                "cranklink",
                "--port",
                "ble:any",
                "--ble-master=normal",
                "-s",
                "010203040506",
                "firmware",
                "flash",
                "ble-pair",
                "--dcas",
                "d.dat",
                "--rx",
                "m.dat",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
        )
        .unwrap_err();
        assert!(
            flash_normal
                .to_string()
                .contains("--ble-master=normal is not supported for firmware flashing"),
            "{flash_normal:#}"
        );
    }

    #[test]
    fn preloaded_restore_retains_snapshot_and_validates_preflight_before_writes() {
        let dir = std::env::temp_dir().join(format!(
            "cranklink-preload-restore-{}-{}",
            std::process::id(),
            crate::platform::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let backup_file = dir.join("dest-backup.json");
        std::fs::write(
            &backup_file,
            serde_json::to_vec(&serde_json::json!({
                "format": "cranklink-setting-backup-v1",
                "serial": "010203040506",
                "slot": 13,
                "setting": "destination",
                "before": 1,
                "original_parameters": "0101ffff"
            }))
            .unwrap(),
        )
        .unwrap();

        let parsed = crate::settings::parse_restore_file(&backup_file).unwrap();
        // Remove the file on disk to prove subsequent planning/validation uses the retained snapshot.
        std::fs::remove_file(&backup_file).unwrap();

        let restore_cmd = SettingsCommand::Restore {
            file: backup_file.clone(),
            backup: None,
            parsed: Some(parsed),
        };
        let decision = plan_ble_routing(
            None,
            &Command::Settings {
                command: restore_cmd.clone(),
            },
        );
        assert_eq!(decision.route, "master");
        assert!(command_requires_unit_auth(
            &restore_cmd,
            crate::settings::SettingsModel::Ep800Crg,
            false
        ));
        validate_settings_preflight(
            &restore_cmd,
            crate::settings::SettingsModel::Ep800Crg,
            "010203040506",
        )
        .unwrap();
        assert!(validate_settings_preflight(
            &restore_cmd,
            crate::settings::SettingsModel::Ep800Crg,
            "ffffff000000"
        )
        .unwrap_err()
        .to_string()
        .contains("backup belongs to another motor"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
