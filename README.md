# cranklink

Rust CLI for diagnostics, configuration, firmware flashing, and recovery of
compatible Shimano STEPS drive units. Recover supported EP800 motors left in
bootloader mode after an interrupted update, using an SM-PCE02 USB adapter and
user-supplied firmware. No vendor application, .NET runtime, or proprietary DLLs
are required.

- Flash D (DCAS) and M (Renesas RX) firmware independently, with companion-pair,
  model/board, serial, and SHA-256 checks plus durable transaction journals.
- Discover components and read motor identity and both firmware versions.
- Read all schema-supported motor settings; write supported named values over
  USB/serial or BLE, with range checks, backups, readback, and serial-bound restore.
- Configure assist profiles, torque/start behavior, wheel geometry, lighting,
  speed/destination, drivetrain/shifting, and maintenance where supported.
  Ordered batch plans apply multiple reads/writes/restores in one connection.

## Build and connect

Requires Rust 1.90+. Release builds include BLE; for a source build:

```sh
git clone https://github.com/plainoss/cranklink.git
cd cranklink
cargo build --release --locked --bin cranklink --features ble
./target/release/cranklink --help
```

The executable is `target/release/cranklink` (`cranklink.exe` on Windows).
USB-only builds can omit `--features ble`. Linux BLE requires a C compiler and a
running BlueZ/D-Bus service; macOS requires Bluetooth permission for the terminal.
Linux, Windows, and macOS Intel/Apple Silicon archives are attached to tagged
GitHub releases when available. CI uses Linux runners only; macOS cross-builds
use the official cargo-zigbuild container with its bundled SDK. Cross-compilation
does not test device access or execution on the target operating system.

Close other adapter owners before connecting. On macOS, use `--port usb` for
direct SM-PCE02 access. On Linux, grant your user access to USB device
`1e44:7151` or the selected serial port. On Windows, prefer the existing FTDI COM
port (`--port COM3`); direct USB requires WinUSB, which replaces the COM binding
until the original driver is restored.

```sh
cranklink ports
cranklink --port usb --read-only units
cranklink --port usb --read-only probe
cranklink --port usb settings schema
cranklink --port usb --expect-motor-serial SERIAL settings read --output settings.json
cranklink --port usb --expect-motor-serial SERIAL settings set profile1.eco.character 4 --backup eco-before.json
cranklink --port usb --expect-motor-serial SERIAL settings restore eco-before.json --backup eco-current.json
```

Replace `SERIAL` with the motor's six-byte serial in 12-digit wire-order hex.
See [settings and batch plans](docs/settings-protocol.md),
[Bluetooth setup and commands](docs/bluetooth.md), and
[component selection](docs/unit-identification.md).

## Recovering an interrupted firmware update

Firmware writes support the EP800 firmware family (`23/00`), DCASX board 1.
Recovery needs a reachable, intact bootloader; it cannot repair hardware damage
or an erased bootloader. RX recovery also requires a responding D application.
EP600/EP801 have separate settings schemas but are not enabled for flashing;
EP500 named settings and flashing are unsupported. Model and firmware gates apply
to every operation. Other hardware combinations require separate validation.

Obtain compatible original firmware containers; none are included. Keep the
motor stationary and power stable. Inspect the pair offline:

```sh
cranklink inspect DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat
cranklink --port usb boot-probe --recovery
```

Use the serial returned by the boot probe and the SHA-256 of the exact input file:

```sh
cranklink --port usb flash-dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat \
  --expect-serial SERIAL --expect-sha256 SHA256 \
  --journal D.journal.jsonl --recovery --write
```

For RX recovery, use `rx-probe --recovery`, then `flash-rx` with the M file as
input, the D file as companion, the M file's SHA-256, and a fresh journal.
Omit `--recovery` for updates from a running application. Omit `--write` to
validate without erasing/programming; validation may enter bootloader mode or
reset the motor. Every invocation requires a new journal filename.

Writes verify the returned application identity and both MCU versions. A timeout
can leave an uncertain device state: preserve the journal, establish that state,
and do not automatically retry. Prefer wired firmware operations; experimental
BLE flashing requires `--ble-firmware`, and BLE recovery is unavailable.

## Browser instrument

The [web instrument](https://plainoss.github.io/cranklink/) runs the same Rust
core as the CLI through WASM. It provides motor discovery, firmware inspection,
settings reads/writes with backups and restore, and a terminal for ordered batch
plans. USB/serial firmware operations retain the CLI's recovery and identity
checks; firmware files are supplied separately.

Use desktop Chrome or Edge where the required WebUSB, Web Serial, or Web
Bluetooth API is available. Hardware access depends on the browser and platform;
mobile support is not universal. BLE settings and opt-in BLE firmware remain
experimental; BLE recovery is unavailable. See [browser setup and local build](web-instrument/README.md),
[settings and batches](docs/settings-protocol.md), and [BLE limits](docs/bluetooth.md).

## License

[MIT](LICENSE). Product names identify compatible hardware only. This project is
independent of Shimano and is not endorsed by or affiliated with Shimano.
