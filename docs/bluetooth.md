# Bluetooth settings

BLE support provides gateway discovery/authentication, connected-unit discovery, motor identification, and read/set/restore/batch operations for all schema-supported motor settings. BLE hardware validation is pending; support depends on the gateway, motor model, firmware, and setting permissions.

## Connect

Build with `cargo build --release --locked --features ble`, then use `target/release/cranklink` (or `target/release/cranklink.exe` on Windows) for the commands below. Prebuilt binaries must also include the BLE feature.

A powered bike needs a compatible E-TUBE BLE gateway, such as SC-EM800. A bare motor has no radio. Enable the gateway's connection mode and close other apps using the bike.

```sh
cranklink ble scan
cranklink --port 'ble:DEVICE_ID' ble authenticate
cranklink --port 'ble:DEVICE_ID' units
cranklink --port 'ble:DEVICE_ID' --component DU-EP800-CRG probe
```

Use the peripheral ID from the scan. Each command opens and authenticates its own connection. Enter the six-digit bicycle passkey at the masked prompt; automation can use `--ble-passkey-file /path/to/passkey` containing six ASCII digits.

macOS requires terminal Bluetooth permission; Windows requires a Bluetooth adapter supported by Windows; Linux requires BlueZ and a running D-Bus daemon. The BLE build enables vendored D-Bus support on Linux.

## Read and write

The example serial `010203040506` is illustrative; replace it with the actual 12-hex-digit motor serial returned by `probe`. The serial guard checks identity inside the settings connection.

```sh
# Discover names, units, ranges, and OEM flags offline.
cranklink settings schema --model ep800-crg

# Export every schema-supported setting; unavailable reads are reported separately.
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings read --output settings.json
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings read profile1.eco.character

# Write one setting and verify readback; each backup filename must be new.
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings set profile1.eco.character 4 --backup eco-before.json
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings restore eco-before.json --backup eco-current.json

# Destination and tire-circumference writes/restores automatically select BLE master routing and drive-unit authentication.
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings set destination 3 --backup dest-before.json

# Apply an ordered JSON plan through one authenticated connection.
cranklink --port 'ble:DEVICE_ID' -s 010203040506 settings batch plan.json --backup-prefix run-01
```

See [Settings](settings-protocol.md) for feature coverage, a batch plan, and backup semantics. `--component` is accepted for probe, settings read, and batch; omit it for standalone set/restore. Automatic selection requires one motor; use a freshly discovered `--slot` if selection is ambiguous. See [Component selection](unit-identification.md).

## Routing policy and drive-unit authentication

BLE routing is controlled by `--ble-master[=auto|normal|master]` (defaulting to `auto` when omitted; passing bare `--ble-master` selects `master`):

- **`auto` (default)**: Resolves routing after gateway authentication and before component discovery. Selects `master` routing (PC-link owner zero) when writing or restoring `destination`, `tire-circumference`, `lamp`, or `lighting-time`, or when a `settings batch` plan contains any of those writes or restores. Ordinary reads, `units`, `probe`, `--dry-run`, and other settings writes use `normal` (slave) routing.
- **`master` (`--ble-master` or `--ble-master=master`)**: Forces BLE master routing for the entire session, verifying gateway identity/revision, PnP discovery, motor owner-zero confirmation, and ordered restoration of normal/slave mode on exit.
- **`normal` (`--ble-master=normal`)**: Forces standard BLE slave routing. Not supported for `firmware flash ble-pair`, which requires master routing.

Drive-unit regulation authentication is planned separately from BLE routing:

- Writing or restoring any `oem`-protected setting (`destination`, `tire-circumference`, `walk-assist`, `light-output`, `torque-maximum`, `max-power.*`, etc.) automatically performs drive-unit authentication on supported models and firmware (`DU-EP800-CRG`, `DU-EP600`, `DU-EP801`) when `-s` / `--expect-serial` is provided.
- `--dry-run` validates input and reads current state over ordinary routing without performing privileged drive-unit authentication.
- `--unit-auth` (or `--ble-unit-auth`) explicitly requests drive-unit authentication; `--no-auth` disables automatic drive-unit authentication and stops before any `oem` write.
- Text output prints `BLE Route: <route> (<reason>)` and JSON output includes `ble_routing: { "route", "policy", "reason" }` without exposing authentication keys or proofs.

## Firmware boundary

Ordinary BLE sessions reject firmware, bootloader, recovery, and power-cycle commands. Experimental EP800 dual-MCU firmware relay uses `firmware flash ble-pair`; hardware validation is pending and BLE recovery/radio DFU are disabled. Use the wired [recovery guide](../README.md) for bricked motors. Settings backups and readback do not establish power-cycle persistence; check the result after reconnecting and power cycling the bike.
