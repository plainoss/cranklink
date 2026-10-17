# cranklink

Open-source diagnostic, tuning, and recovery toolkit for compatible Shimano STEPS e-bike drive units.

Recover supported EP800 motors left unresponsive in bootloader mode after an interrupted update, inspect bus components, and safely customize ride settings—using an SM-PCE02 USB adapter or Bluetooth LE. No vendor accounts, .NET runtimes, or proprietary DLLs required.

- **Safe by default**: All firmware flashing runs as a dry-run unless `--write` is specified. Settings modifications automatically save timestamped pre-write backups with readback verification.
- **Bootloader recovery**: Direct offline flashing and bootloader probing for compatible EP800 motors (`23/00`, DCASX board 1).
- **Diagnostics & tuning**: Discover all E-TUBE bus components, inspect firmware revisions, and adjust assist profiles, torque, wheel geometry, and lighting.
- **Dual interfaces**: Fast native CLI plus a browser-based [Web Instrument](https://plainoss.github.io/cranklink/) running the same engine via WebUSB / WebBluetooth.

## CLI command overview

```text
cranklink [OPTIONS] <COMMAND>
├── scan        Discover attached USB adapters (<5ms) and nearby BLE gateways (--ble)
├── units       Inventory connected E-TUBE bus components (motor, display, shifter)
├── probe       Probe active drive unit (model, serial, MCU firmware versions)
├── settings    Read, export, and configure motor settings
│   ├── schema  Inspect available settings, units, ranges, and permissions
│   ├── read    Read a specific setting or snapshot all settings to JSON
│   ├── set     Modify a setting with auto-backup and readback verification
│   ├── restore Revert a setting from a serial-bound backup file
│   └── batch   Execute an ordered JSON configuration plan in one session
├── firmware    Inspect, verify, probe, and recover motor firmware
│   ├── inspect Decode and validate firmware containers (.dat) offline
│   ├── probe   Inspect MCU bootloader state (DCAS or RX)
│   └── flash   Program firmware safely (requires --write to commit)
│       ├── dcas      Program D / DCASX MCU over wired adapter
│       ├── rx        Program M / Renesas RX MCU over wired adapter
│       └── ble-pair  Dual-MCU atomic update via BLE relay
├── adapter     SM-PCE02 hardware adapter diagnostics and power cycling
├── ble         Bluetooth LE gateway discovery and link testing
└── session     Persistent interactive JSON-RPC driver for automation & AI agents
```

## Quickstart & common workflows

### 1. Discover & connect

List attached adapters or discover connected bike hardware on the bus:

```sh
# Fast local scan for attached USB adapters
cranklink scan
```
```text
Attached Adapters (7 found):
  usb                  Direct SM-PCE02 FTDI USB (1 device(s); no COM driver required)

Tip: Run 'cranklink scan --ble' to discover wireless Bluetooth gateways.
```

```sh
# Inventory all connected E-TUBE components on the bus
cranklink --port usb units
```
```text
Connected E-TUBE Components (1 found):

  SLOT  TYPE          MODEL                 SERIES:UNIT  STATUS
  13    Drive Unit    DU-EP800-CRG          0x23:0x00    Identified

Tip: Run 'cranklink probe' to inspect drive-unit firmware versions (DCAS/RX) and serial number.
```

```sh
# Probe drive unit model, serial, and dual MCU firmware versions
cranklink --port usb probe
```
```text
Drive Unit Probe:
  Model:       DU-EP800-CRG (Slot 13)
  Serial:      8ff77c766533
  Cargo ID:    00-CRG
  DCAS (D):    v4.2.0.0
  Renesas (M): v4.1.11.0
```

> [!NOTE]
> **Adapter connection tips:**
> - **macOS**: Use `--port usb` for direct SM-PCE02 access.
> - **Linux**: Grant user permissions for USB device `1e44:7151` or your serial port (`/dev/ttyUSB0`).
> - **Windows**: Prefer the FTDI COM port (`--port COM3`). Direct USB requires WinUSB.
> - Close other adapter applications before connecting.

### 2. View & tune ride settings

Inspect schemas, export complete snapshots, or modify settings safely. Every write creates an automatic pre-write backup and validates the result:

```sh
# Read a single setting
cranklink --port usb -s 8ff77c766533 settings read profile1.boost.torque
```
```text
profile1.boost.torque: 85 Nm
```

```sh
# Test a change safely without writing to EEPROM (--dry-run)
cranklink --port usb -s 8ff77c766533 settings set profile1.boost.torque 85 --dry-run
```
```text
[DRY RUN - VALIDATION ONLY]
Setting: profile1.boost.torque
  Current:  85 Nm
  Planned:  85
  State:    UNMODIFIED (No bytes written)
>> To write this change, re-run without the --dry-run flag.
```

```sh
# Commit a change (automatically saves a pre-write backup before writing)
cranklink --port usb -s <SERIAL> settings set profile1.eco.character 4 --backup eco-before.json

# Restore from a previous backup
cranklink --port usb -s <SERIAL> settings restore eco-before.json

# Export a complete settings snapshot to JSON
cranklink --port usb -s <SERIAL> settings read --output settings.json
```

Replace `<SERIAL>` with the motor's 12-digit wire-order hex serial (e.g. `8ff77c766533`, returned by `cranklink probe`).
See [settings and batch plans](docs/settings-protocol.md), [Bluetooth setup and commands](docs/bluetooth.md), and [component selection](docs/unit-identification.md).

### 3. Recover an interrupted firmware update

Firmware writes support the EP800 firmware family (`23/00`), DCASX board 1. Recovery requires a reachable, intact bootloader; it cannot repair hardware damage or an erased bootloader. RX recovery also requires a responding D application.

> [!IMPORTANT]
> Firmware containers (.dat) are user-supplied and not bundled. Keep the motor stationary and power stable.

First, validate the container files offline without connecting to the bike:

```sh
cranklink firmware inspect DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat
```
```text
Firmware Image Metadata:
  Kind:                DCASX
  Version:             v4.2.0.0 (Compatibility: v4.1.11.0)
  Target Board/Series: Board 1 (Series 0x23, Unit 0x00)
  Checksum:            0x5A
  Source SHA-256:      9f8e7d...
```

Probe the bootloader to confirm the motor is responding and retrieve its serial:

```sh
cranklink --port usb firmware probe --target dcas --recovery
```
```text
DCAS Bootloader Probe:
  Slot:       13
  Version:    v5.0.6.1
  Board:      1
  Series:     0x23
```

Run a dry-run flash simulation to check compatibility and SHA-256 integrity:

```sh
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat \
  --recovery
```

Once validated, commit the flash with `--write`:

```sh
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat \
  --journal D.journal.jsonl \
  --recovery --write
```

For RX recovery, use `cranklink --port usb firmware probe --target rx --recovery`, then `firmware flash rx` with the M file as input and the D file as companion.

### 4. Hardware adapter diagnostics

Inspect SM-PCE02 connection status and power state:

```sh
cranklink --port usb adapter status
```
```text
SM-PCE02 Adapter:
  Bus Power: Active (Powered)
  Raw State: 0x02
```

## Browser instrument

The [web instrument](https://plainoss.github.io/cranklink/) compiles the same core engine to WebAssembly, providing an interactive UI for phones, tablets, and desktops:

- Hardware connection via WebUSB, Web Serial, or Web Bluetooth (Chrome, Edge).
- Component discovery, firmware inspection, and settings backup & tuning sliders.
- Built-in simulator for testing without connected hardware.

See the [web instrument documentation](web-instrument/README.md).

## Installation & builds

Prebuilt binaries for Linux, macOS (Intel & Apple Silicon), and Windows are attached to tagged GitHub releases.

### Building from source

Requires Rust 1.90+:

```sh
git clone https://github.com/plainoss/cranklink.git
cd cranklink
cargo build --release --locked --bin cranklink --features ble
./target/release/cranklink --help
```

- Release binary is located at `target/release/cranklink` (`cranklink.exe` on Windows).
- Omit `--features ble` for a USB-only build without Bluetooth dependencies.
- Linux BLE requires a C compiler and a running BlueZ/D-Bus service. macOS requires Bluetooth permission for the terminal.
- Linux, Windows, and macOS Intel/Apple Silicon archives are attached to tagged GitHub releases when available. CI uses Linux runners only; macOS cross-builds use the official cargo-zigbuild container with bundled SDK.

## Scripting & automation

Cranklink provides structured output and interactive session modes for scripts, test harnesses, and AI agents:

```sh
# Machine-readable JSON output for any command
cranklink --json scan
cranklink --json --port usb -s <SERIAL> settings read

# Persistent interactive JSON-RPC driver session
cranklink session --json
```

## Hardware compatibility & notes

- **EP800 (DU-EP800)**: Full diagnostics, settings read/write, offline firmware inspection, and bootloader recovery.
- **EP801 / EP600**: Component discovery, diagnostics, and dedicated settings schemas. Flashing is not enabled.
- **EP500**: Named settings and flashing are unsupported.
- Every operation enforces model and firmware gates to protect hardware.

## License

[MIT](LICENSE). Product names identify compatible hardware only. This project is independent of Shimano and is not endorsed by or affiliated with Shimano.
