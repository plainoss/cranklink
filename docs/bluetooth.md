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
cranklink --port 'ble:DEVICE_ID' --ble-expect-serial 010203040506 settings read --output settings.json
cranklink --port 'ble:DEVICE_ID' --ble-expect-serial 010203040506 settings read profile1.eco.character

# Write one setting and verify readback; each backup filename must be new.
cranklink --port 'ble:DEVICE_ID' --ble-expect-serial 010203040506 settings set profile1.eco.character 4 --backup eco-before.json
cranklink --port 'ble:DEVICE_ID' --ble-expect-serial 010203040506 settings restore eco-before.json --backup eco-current.json

# Apply an ordered JSON plan through one authenticated connection.
cranklink --port 'ble:DEVICE_ID' --ble-expect-serial 010203040506 settings batch plan.json --backup-prefix run-01
```

See [Settings](settings-protocol.md) for feature coverage, a batch plan, and backup semantics. `--component` is accepted for probe, settings read, and batch; omit it for standalone set/restore. Automatic selection requires one motor; use a freshly discovered `--slot` if selection is ambiguous. See [Component selection](unit-identification.md).

OEM settings may require experimental drive-unit authentication: add `--ble-unit-auth` with `--ble-expect-serial`. It is version checked, attempted once per connection, and does not guarantee the motor will accept a write. `--ble-master` enables experimental alternative gateway routing. Unsupported replies and failed authentication stop the operation.

## Firmware boundary

Ordinary BLE sessions reject firmware, bootloader, recovery, and power-cycle commands. Experimental EP800 firmware relay requires explicit `--ble-firmware`; hardware validation is pending and BLE recovery/radio DFU are disabled. Use the wired [recovery guide](../README.md) for bricked motors. Settings backups and readback do not establish power-cycle persistence; check the result after reconnecting and power cycling the bike.
