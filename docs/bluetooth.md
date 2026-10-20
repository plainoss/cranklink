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
