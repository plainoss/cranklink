# cranklink

Open-source diagnostic, tuning, and recovery toolkit for compatible Shimano STEPS e-bike drive units, over an SM-PCE02 USB adapter or Bluetooth LE.

It started as a successful attempt to recover a bricked EP800-CRG that official software and dealer channels could not repair.

- **Bootloader recovery** for EP800 motors stuck in bootloader mode.
- **Diagnostics**: discover E-TUBE bus components, read model, serial, and firmware versions.
- **Tuning**: assist profiles, torque, wheel circumference, lighting, with backups and readback verification.
- **CLI and browser**: native CLI plus a [web instrument](https://plainoss.github.io/cranklink/) running the same engine via WebUSB / Web Bluetooth.

> [!WARNING]
> **Unofficial and experimental. You can brick your hardware.**
> cranklink is not affiliated with or endorsed by Shimano. Writing settings or firmware can permanently damage drive units, displays, or batteries and void warranties. It is meant for technical users who accept full responsibility for the outcome.
>
> Firmware flashing has **only been tested on a DU-EP800-CRG** (`23/00`, DCASX board 1). cranklink checks the container's declared model, board, and integrity, but it cannot prove an image is correct for your exact hardware. A wrong image can leave the motor unrecoverable, even with dealer tools.

## Supported hardware

| Drive unit | Diagnostics | Settings read/write | Firmware flashing |
| --- | --- | --- | --- |
| EP800 / EP800-CRG | ✓ | ✓ | ✓ (tested on EP800-CRG only) |
| EP801 / EP600 | ✓ | ✓ (dedicated schemas) | — |
| EP500 | ✓ | — | — |

Available fields depend on firmware and device permissions; see [settings compatibility](docs/settings-protocol.md#compatibility-and-validation).

## Install

Prebuilt binaries for Linux, macOS (Intel and Apple Silicon), and Windows are attached to tagged [GitHub releases](https://github.com/plainoss/cranklink/releases).

To build from source (Rust 1.90+):

```sh
git clone https://github.com/plainoss/cranklink.git
cd cranklink
cargo build --release --locked --bin cranklink --features ble
./target/release/cranklink --help
```

Omit `--features ble` for a USB-only build. Linux BLE needs a C compiler and BlueZ/D-Bus; macOS needs Bluetooth permission for the terminal.

**Connecting the adapter**

- **macOS**: `--port usb` for direct SM-PCE02 access.
- **Linux**: grant access to USB device `1e44:7151` or the serial port (`/dev/ttyUSB0`).
- **Windows**: prefer the FTDI COM port (`--port COM3`); direct USB requires WinUSB.
- Close other adapter applications first. If the adapter's supercapacitor needs charging, cranklink waits up to 90 s ([details](docs/usb.md)).

## Quickstart

### Connect and identify

```sh
cranklink scan                 # attached USB adapters (add --ble for Bluetooth gateways)
cranklink --port usb units     # components on the E-TUBE bus
cranklink --port usb probe     # drive unit model, serial, firmware versions
```

```text
Drive Unit Probe:
  Model:       DU-EP800-CRG (Slot 13)
  Serial:      010203040506
  DCAS (D):    v4.2.0.0
  Renesas (M): v4.1.11.0
```

Commands that touch settings or firmware take `-s <SERIAL>` (the 12-digit hex serial from `probe`) so they only ever act on the motor you meant.

### Read and change settings

```sh
# Snapshot all settings
cranklink --port usb -s <SERIAL> settings read --output settings.json

# Preview a change without writing
cranklink --port usb -s <SERIAL> settings set profile1.boost.torque 85 --dry-run

# Write it (saves a pre-write backup, then verifies by readback)
cranklink --port usb -s <SERIAL> settings set profile1.boost.torque 85 --backup before.json

# Roll back
cranklink --port usb -s <SERIAL> settings restore before.json
```

OEM-protected settings authenticate with the drive unit automatically; over Bluetooth, settings that need it are routed through the gateway automatically. See [settings and batch plans](docs/settings-protocol.md), [Bluetooth](docs/bluetooth.md), and [component selection](docs/unit-identification.md).

### Recover a motor stuck in bootloader mode

Requirements: EP800 family (`23/00`, DCASX board 1), an intact bootloader, stable power, and your own firmware containers (`.dat`, not bundled). Recovery cannot fix hardware damage or an erased bootloader. RX (M) recovery also needs a responding D application.

```sh
# 1. Inspect the containers offline
cranklink firmware inspect DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat

# 2. Confirm the bootloader responds
cranklink --port usb firmware probe --target dcas --recovery

# 3. Dry run (default): validates target and integrity, writes nothing
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat --recovery

# 4. Write
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat --journal D.journal.jsonl --recovery --write
```

For RX, probe with `--target rx`, then run `firmware flash rx` with the M file as input and the D file as companion.

## Web instrument

[plainoss.github.io/cranklink](https://plainoss.github.io/cranklink/) runs the same core compiled to WebAssembly in Chrome or Edge on desktop and Android. It connects over WebUSB, Web Serial, or Web Bluetooth, and covers discovery, firmware inspection, backups, and settings. Every save goes through a review with per-change undo. See the [web instrument docs](web-instrument/README.md).

## Command reference

```text
cranklink [OPTIONS] <COMMAND>
├── scan        Discover USB adapters and BLE gateways (--ble)
├── units       Inventory E-TUBE bus components
├── probe       Drive unit model, serial, firmware versions
├── settings
│   ├── schema  Available settings, units, ranges, permissions
│   ├── read    Read one setting or snapshot all to JSON
│   ├── set     Change a setting (backup + readback verification)
│   ├── restore Revert from a serial-bound backup
│   └── batch   Apply an ordered JSON plan in one session
├── firmware
│   ├── inspect Decode and validate .dat containers offline
│   ├── probe   Bootloader state (DCAS or RX)
│   └── flash   Program firmware (dry run unless --write)
│       ├── dcas      D / DCASX MCU over wired adapter
│       ├── rx        M / Renesas RX MCU over wired adapter
│       └── ble-pair  Both MCUs via BLE relay
├── adapter     SM-PCE02 status and power cycling
├── ble         Bluetooth gateway discovery and link testing
└── session     Persistent JSON-RPC driver for scripts and agents
```

Add `--json` to any command for machine-readable output; `cranklink session --json` keeps one connection open for automation.

## Offline simulator

A headless simulator runs stock display, gateway, and motor firmware on a virtual bus, so the CLI (`--port sim:/path/to/socket`) and Bluetooth client (`ble-sim:/path/to/socket`) can be tested without hardware. Firmware images are fetched from official distribution endpoints and cached locally, never committed. See the [simulator guide](docs/firmware-simulator.md).

## License

[MIT](LICENSE). Product names identify compatible hardware only. This project is independent of Shimano and is not endorsed by or affiliated with Shimano.
