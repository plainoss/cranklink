# Bluetooth (CLI)

Requires a build with the default `ble` feature (not `--no-default-features`) and a bike with an E-TUBE Bluetooth gateway (e.g. SC-EM800); a bare motor has no radio. macOS needs Bluetooth permission for the terminal; Linux needs BlueZ and D-Bus.

Activate the gateway's connection mode shortly before scanning, and close other apps connected to the bike. The web app includes the same connection help.

## Connection mode

Follow the instructions for the wireless gateway, which can be separate from the component you want to configure:

- **Di2 wireless rear derailleur / MU-UR520:** hold the function button, then release as soon as the blue LED starts blinking.
- **EW-WU111 / EW-WU101:** hold the Junction A button, then release when its red and green LEDs alternate.
- **SC-M9051 / SC-MT800:** hold the mode switch until C appears, then release.
- **STEPS cycle computers except SC-E8000:** press a non-power button or turn the system on to open its 15-second connection window.
- **SC-E8000:** use its Bluetooth LE menu as described in the connection guide.

Release at the indicated signal; holding longer can select another mode. Classic wired Di2 requires a compatible Bluetooth wireless unit or a USB adapter; lever buttons alone do not make it discoverable. These instructions follow [Shimano's E-TUBE PROJECT Cyclist connection guide](https://si.shimano.com/en/pdfs/um/7J4MW/UM-7J4MW-005-ENG.pdf).

## CLI

```sh
cranklink ble scan
cranklink --port 'ble:DEVICE_ID' ble test-link
cranklink --port 'ble:DEVICE_ID' -s <SERIAL> settings read --output settings.json
```

`DEVICE_ID` is the peripheral ID from the scan; `<SERIAL>` is the target component serial from `probe`, not the Bluetooth ID. Add `--component <MODEL>` to target a catalogued Di2 component. The six-digit bike passkey is prompted for, or read from `--passkey-file` or `CRANKLINK_PASSKEY`.

Writes that need gateway master routing (destination, wheel circumference, lamp, lighting time) select it automatically (`--ble-master=auto`, the default). `--ble-master` forces master routing and `--ble-master=normal` forces normal routing; the value needs `=`, as `--ble-master normal` reads `normal` as a subcommand. Drive-unit authentication for protected fields, and `--unit-auth`, work as over USB; see [protected writes](settings.md#protected-writes).

Firmware recovery is not available over Bluetooth; use the SM-PCE02. `firmware flash ble-pair` is experimental and has not been run on hardware. Without `--allow-unverified-model` it accepts only EP800 board 1 motors running D 4.1.10, 4.1.11 or 4.2.0, and D 4.1.11 or 4.2.0 targets. With `--write`, each plaintext image (such as the EP800 M file) also needs its hash in `--expect-dcas-sha256` or `--expect-rx-sha256`.

E6000/E6001-family writes require no motor authentication; E6002/E6012 use legacy authentication from D3.4.0. E6002 region selection is available from D3.3.0. These additions remain hardware-unvalidated; see [E6000-family settings](settings.md#e6000-family-drive-units).
