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
cranklink --port 'ble:DEVICE_ID' --read-only ble detect-system
cranklink --port 'ble:DEVICE_ID' units --stock-only
cranklink --port 'ble:DEVICE_ID' -s <SERIAL> settings read --output settings.json
```

`DEVICE_ID` is the peripheral ID from the scan; `<SERIAL>` is the target component serial from `probe`, not the Bluetooth ID. Add `--component <MODEL>` to target a catalogued Di2 component. The six-digit bike passkey is prompted for, or read from `--passkey-file` or `CRANKLINK_PASSKEY`.

`units --stock-only` skips optional part-number and model-discriminator GETs. Unresolved models remain candidates. Named component selection resolves only candidates for the requested model, so an unrelated unresponsive lever cannot block another target. Each new connection still discovers its own current slots.

Writes that need gateway [master routing](../README.md#terms) (market region, wheel circumference, lamp, lighting time) select it automatically (`--ble-master=auto`, the default). `--ble-master` forces master routing and `--ble-master=normal` forces normal routing; the value needs `=`, as `--ble-master normal` reads `normal` as a subcommand. Drive-unit authentication for protected fields, and `--unit-auth`, work as over USB; see [protected writes](settings.md#protected-writes).

## Disconnect and Di2 recovery

When you are done, press **Disconnect** (or let the CLI command finish) and wait until the display, buttons and shifting respond normally before riding. The rest of this section explains how the link is closed.

Ordinary normal/slave sessions end by closing GATT, leaving the gateway firmware to handle its radio-disconnect event. They do not send an explicit logical-master `32/10` stop, gateway `0C/00` stop or gateway reset. This matches the standard client normal-disconnect path. Explicit stop acknowledgements did not restore either tested bike; closing GATT alone preserved normal operation on both EW-WU111/BT-DN110 Di2 and SC-EM800/EP800 STEPS hardware.

The browser retains its normal/slave service session between commands without stopping the logical master. Final Disconnect closes GATT. Changing routing roles on an active retained session requires disconnect/reconnect. Explicit master-routing sessions keep their acknowledged component stop and routing-restoration sequence.

The web app keeps **Disconnect** visible in its top bar, including on phones. It shows progress while closing the link, then a completion screen explaining that the bike may need a few seconds to return to riding mode. The browser cannot observe that physical transition; the completion screen stays until you choose **Back to connection screen**. A failed cleanup remains an error rather than showing successful Bluetooth closure.

Read and write errors still fail the operation; closing the link never converts an error into a successful read or write. A failed command is not retried. Link closure and normal physical operation are distinct observations: confirm shifting, display and controls when validating a new gateway. See the [Di2 BLE investigation](di2-ble-investigation.md) for the successful physical checks, rejected experiments and simulator limits.

The web app detects supported master families before starting service, keeping the original GATT connection. It queries gateway master metadata (`00 0d 00`) after normal/slave authentication, before PC-link (`00 0c 01`). Supported STEPS families select the existing master-routing workflow; BM-DN100/BT-DN110/BT-DN300 master families select ordinary routing. Di2 components on a STEPS bike do not change that decision.

A complete reply with valid battery status but an unknown family opens a choice **before service**, retaining Bluetooth. Historical STEPS families and third-party adapters do not silently become shifting-only bikes. Truncated metadata, invalid battery status and query timeouts fail and close the connection; timeout poison is never cleared to permit a fallback. Under **Bluetooth options**, a manual override can bypass detection on a new connection if the gateway cannot answer. A detected family must agree with later inventory, and a selected motor's stock identity is checked before its settings session starts. Native `ble detect-system` reports the master family and `service_started: false`, then closes; ordinary native routing defaults are unchanged.

### Validate automatic detection on a phone

The pre-service metadata handler has offline firmware coverage and client packet sequencing establishes the ordering. Explicit pre-service queries still need physical validation. Use the PR preview for testing; do not use the production URL until that change is merged and deployed.

1. Close E-TUBE and any other app connected to the bike. Open the preview in Chrome on Android or Bluefy on iPhone. Leave **Bluetooth options → Bike system override** at **Detect automatically**.
2. On the shifting-only bike, enable Bluetooth and connect once. Expect **Di2 shifting only detected** above the settings. Read one component's settings without saving anything.
3. Press **Disconnect**, wait for normal shifting and junction-button response, and note whether another power cycle was needed.
4. On the STEPS bike, enable its power-on Bluetooth window and connect once. Expect **STEPS detected**, including when Di2 shifting components are installed. Inventory and a settings read should succeed within that same connection, without another chooser or power cycle.
5. Press **Disconnect** and confirm the display, controls and shifting return to normal. Record which bike was tested, the detected label, any error text, and whether recovery needed a power cycle.

If a choice appears rather than a detected label, automatic identification was inconclusive: the manual choice tests only fallback. If a query fails, report the error before retrying with a manual override on a newly enabled connection. These checks cover inventory and reads; they do not validate settings writes or firmware updates.

Firmware recovery is not available over Bluetooth; use the SM-PCE02. `firmware flash ble-pair` is experimental and has not been run on hardware. Without `--allow-unverified-model` it accepts only EP800 board 1 motors running D 4.1.10, 4.1.11 or 4.2.0, and D 4.1.11 or 4.2.0 targets. With `--write`, each plaintext image (such as the EP800 M file) also needs its hash in `--expect-dcas-sha256` or `--expect-rx-sha256`.

## Staged Di2 read validation

For a bike that previously remained in service mode, validate one connection at
a time: authentication-only `ble test-link`, then `units --stock-only`, then
one read from the freshly selected named component. Use `--read-only` and a new
`--trace` path for every connection. Confirm shifting and the junction button
with the rider between stages. Do not reuse an old component slot or combine
the stages into an automatic retry loop. A failed read closes the link and ends
the test; it is not retried automatically.

E6000/E6001-family writes require no motor authentication; E6002/E6012 use legacy authentication from D3.4.0. E6002 region selection is available from D3.3.0. These additions remain hardware-unvalidated; see [E6000-family settings](settings.md#e6000-family-drive-units).
