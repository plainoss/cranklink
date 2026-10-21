# Bluetooth (CLI)

Requires a build with `--features ble` and a bike with an E-TUBE Bluetooth gateway (e.g. SC-EM800); a bare motor has no radio. macOS needs Bluetooth permission for the terminal; Linux needs BlueZ and D-Bus.

Power the bike on shortly before scanning. If it is not listed, power-cycle it or enable the gateway's connection mode, and close other apps connected to the bike.

```sh
cranklink ble scan
cranklink --port 'ble:DEVICE_ID' ble test-link
cranklink --port 'ble:DEVICE_ID' -s <SERIAL> settings read --output settings.json
```

`DEVICE_ID` is the peripheral ID from the scan; `<SERIAL>` is the motor serial from `probe`, not the Bluetooth ID. The six-digit bike passkey is prompted for, or read from `--passkey-file` or `CRANKLINK_PASSKEY`.

Writes that need gateway master routing (destination, wheel circumference, lamp, lighting time) select it automatically; `--ble-master` overrides the policy.

Firmware recovery is not available over Bluetooth; use the SM-PCE02. `firmware flash ble-pair` is experimental and has not been run on hardware.

For manual CLI speed testing, `firmware flash ble-pair` supports `--d-only`,
`--d-upload sequential|prefetch` (default: prefetch), `--d-fragment-batch`
(default: off), and `--m-queue-width 1|2|4|8` (default: 4). D prefetch keeps one
programming transfer outstanding and requires all receipts. Fragment batches
await each write in order; M streaming queues acknowledged writes when the
gateway requires them. Every M window and the complete image remain checked.
Cached stale D completion receipts are cleared before each transfer.

See the [manual testing guide](ble-firmware-speed-testing.md) for build commands,
baseline comparisons, trace timing, and the distinction between a dry run and
an actual transfer. Speed gains remain hardware-unverified.
