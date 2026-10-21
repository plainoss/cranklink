# cranklink

[![CLI](https://github.com/plainoss/cranklink/actions/workflows/build-cli.yml/badge.svg)](https://github.com/plainoss/cranklink/actions/workflows/build-cli.yml)
[![Web](https://github.com/plainoss/cranklink/actions/workflows/web.yml/badge.svg)](https://github.com/plainoss/cranklink/actions/workflows/web.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Diagnostics, settings, and bootloader recovery for Shimano STEPS drive units over the E-TUBE bus, via an SM-PCE02 adapter or a Bluetooth LE gateway. Available as a web app and a CLI built on the same Rust core.

> [!WARNING]
> Unofficial, experimental, and not affiliated with or endorsed by Shimano. Writing settings or firmware can permanently damage drive units, displays, or batteries and voids warranties. Raising the assist speed limit can make the bike non-compliant with local e-bike law. Use at your own risk.
>
> Firmware flashing has only been tested on a DU-EP800-CRG (`23/00`, DCASX board 1). cranklink checks the container's declared series, unit, board, and integrity, but cannot prove an image is correct for a given unit. A wrong image can leave the motor unrecoverable, including by dealer tools.

The project began as the recovery of a bricked EP800-CRG that official tools and dealer service could not restore.

## Web app

**[cranklink.plainoss.workers.dev](https://cranklink.plainoss.workers.dev/)**

[![Web app, assist profile settings](docs/images/web-instrument.png)](https://cranklink.plainoss.workers.dev/)

The core compiled to WebAssembly; all protocol work runs in the browser. Requires Chrome or Edge on desktop or Android (Safari and iOS lack WebUSB and Web Bluetooth), plus one of:

- **SM-PCE02** over WebUSB or Web Serial. Required for firmware recovery. On Android, use WebUSB.
- **Bluetooth**, via the bike's E-TUBE gateway (e.g. SC-EM800). Power the bike on shortly before connecting; if it is not listed, power-cycle it or enable the gateway's connection mode. Close other apps connected to the bike.

Firmware images are not bundled.

## Supported motors

| Drive unit | Settings | Firmware recovery | Hardware-tested |
| --- | --- | --- | --- |
| EP800, EP800-CRG | Profiles, speed, wheel, lamp | Yes | Yes |
| EP600, EP801, EP500 (incl. CRG) | Profiles, speed, wheel, lamp | Opt-in¹ | No |
| E5100 | Common settings, speed | Opt-in¹ | No |
| E8000, E7000, E6100, E6000 families | Wheel, mounting angle, lamp | Opt-in¹ | No |

All families support component discovery and identification. Field availability depends on firmware and device permissions; see [settings](docs/settings.md).

¹ CLI only, with `--allow-unverified-model`. Covered by protocol tests, not run on hardware.

## Command line

### Install

Tagged [releases](https://github.com/plainoss/cranklink/releases) attach prebuilt binaries for Linux, macOS, and Windows. To build from source (Rust 1.90+):

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
- Close other adapter applications first. If the adapter's supercapacitor needs charging, cranklink waits up to 90 s; keep USB connected.

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

Commands that touch settings or firmware take `-s <SERIAL>` (the 12-digit hex serial from `probe`) and fail if the connected unit reports a different serial.

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

See [settings](docs/settings.md) for protected fields, backups, and batch plans, and [Bluetooth](docs/bluetooth.md) for wireless use.

### Recover a motor stuck in bootloader mode

Requires an intact bootloader, stable power, and your own firmware containers (`.dat`, not bundled). Only EP800 (`23/00`, DCASX board 1) is tested; other families need `--allow-unverified-model`. Recovery cannot fix hardware damage or an erased bootloader. RX (M) recovery also needs a responding D application.

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

`cranklink --help` and `cranklink <command> --help` list all commands and options. `--json` gives machine-readable output; `cranklink session --json` keeps one connection open for scripts.

## Documentation

- [Settings](docs/settings.md): protected writes, backups, batch plans.
- [Bluetooth](docs/bluetooth.md): CLI over a BLE gateway.
- [Firmware simulator](docs/firmware-simulator.md): offline testing against stock firmware.
- [Firmware patcher](docs/ep800-speed-limit-patcher.md): create and check supported motor images offline with `tools/patch_motor_speed.py`.
- [Web app development](web-instrument/README.md): build, test, deploy.

## License

[MIT](LICENSE). Product names identify compatible hardware only. This project is independent of Shimano and is not endorsed by or affiliated with Shimano.
