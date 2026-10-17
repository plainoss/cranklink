# Bluetooth settings

BLE support provides gateway discovery/authentication, connected-unit discovery, motor identification, and read/set/restore/batch operations for all schema-supported motor settings. Hardware validation currently covers EP800-CRG settings; other combinations use the compatibility and validation limits below. Support depends on the gateway, motor model, firmware, and setting permissions.

## Connect

Build with `cargo build --release --locked --features ble`, then use `target/release/cranklink` (or `target/release/cranklink.exe` on Windows) for the commands below. Prebuilt binaries must also include the BLE feature.

A powered bike needs a compatible E-TUBE BLE gateway, such as SC-EM800. A bare motor has no radio. Power the bike on just before searching, or power it off and on again if it is not discoverable. Enable the gateway's Bluetooth connection mode where required, and close other apps using the bike. Discovery may only be available briefly after power-on.

```sh
cranklink ble scan
cranklink --port 'ble:DEVICE_ID' ble authenticate
cranklink --port 'ble:DEVICE_ID' units
cranklink --port 'ble:DEVICE_ID' --component DU-EP800-CRG probe
```

Use the peripheral ID from the scan. Each command opens and authenticates its own connection. Enter the six-digit bicycle passkey at the masked prompt; automation can use `--ble-passkey-file /path/to/passkey` containing six ASCII digits.

The browser selects master routing before its initial motor session and obtains motor identity and the full settings snapshot from the same session. The authenticated connection stays open between commands. The browser retains that route until explicit Disconnect, including subsequent reads selected with automatic routing. Between commands it releases the selected motor's PC-link ownership; the next command confirms gateway master mode and its component inventory, then freshly checks motor identity and establishes motor PC ownership and any required authorization. It does not repeat role changes or PnP discovery while the retained master route remains valid. Explicit Disconnect stops the gateway session and closes GATT; failed operations also close the connection and are never replayed automatically. A browser session lock prevents another tab on the same origin from using its own BLE session concurrently. Enable the bike's connection mode again after disconnecting.

A browser firmware-pair command validates its images and options before taking over the retained connection; it keeps the existing firmware identity, serial, backup, and cleanup checks. Each browser command starts a new trace when `--trace PATH` is supplied, requiring a new file. An omitted trace option stops recording to the previous command's trace; a trace-file creation failure leaves the retained connection unchanged.

Failed gateway authentication clears the cached PIN in memory and browser session storage; the next connection prompts again. Known connection errors show a concise cause and recovery step, while Diagnostics retains the complete error.

macOS requires terminal Bluetooth permission; Windows requires a Bluetooth adapter supported by Windows; Linux requires BlueZ and a running D-Bus daemon. The BLE build enables vendored D-Bus support on Linux.

For offline development on macOS/Linux, `--port ble-sim:/absolute/socket --ble-master` connects this same client to the stock-firmware simulator with a modeled radio. It needs no Bluetooth adapter or `ble` build feature. See the [setup and coverage guide](firmware-simulator.md#run-the-bluetooth-client-offline); the ordinary socket endpoint stays read-only. A separate [pinned browser-worker regression](firmware-simulator.md#persistent-writes-and-retained-browser-sessions) covers persistent lamp/wheel writes, real motor authentication and retained sessions on the exact EP800 D4.2.0 profile. Physical radio behavior remains modeled.

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

- Writing or restoring any `oem`-protected setting (`destination`, `tire-circumference`, `walk-assist`, `light-output`, `torque-maximum`, `max-power.*`, etc.) automatically performs drive-unit authentication on supported motor families (`DU-EP800`/`DU-EP800-CRG`, `DU-EP600`/`DU-EP600-CRG`, `DU-EP801`/`DU-EP801-CRG`, `DU-EP500`, `DU-E5100`, and older E-series subsets) when `-s` / `--expect-serial` is provided.
- Authorization uses the live motor family: legacy for EP800/E5100 and applicable older E-series versions, modern for EP600/EP801/EP500. Older firmware without authorization support sends no authentication keys. Firmware patch numbers are not an allowlist. Unknown model variants stop before sending authorization material. USB/serial uses the same family policy. See [compatibility and validation](settings-protocol.md#compatibility-and-validation).
- `--dry-run` validates input and reads current state over ordinary routing without performing privileged drive-unit authentication.
- `--unit-auth` (or `--ble-unit-auth`) explicitly requests drive-unit authentication; `--no-auth` disables automatic drive-unit authentication and stops before any `oem` write.
- Text output prints `BLE Route: <route> (<reason>)` and JSON output includes `ble_routing: { "route", "policy", "reason" }` without exposing authentication keys or proofs.

## Firmware boundary

Ordinary BLE sessions reject firmware, bootloader, recovery, and power-cycle commands. Experimental EP800 dual-MCU firmware relay uses `firmware flash ble-pair`; hardware validation is pending and BLE recovery/radio DFU are disabled. Use the wired [recovery guide](../README.md) for bricked motors. Settings backups and readback do not establish power-cycle persistence; check the result after reconnecting and power cycling the bike.

## Queued M firmware writes

Both the native and browser (WASM) builds keep up to 4 acknowledged `FREE2AFE` writes in flight while streaming the Renesas M image (`BleSession::set_queue_width`; 1 restores sequential writes). Every write is still acknowledged, sequence order and each 1 KiB window checksum are enforced, and no write-without-response is used. Offline-validated only; not yet physically tested.
