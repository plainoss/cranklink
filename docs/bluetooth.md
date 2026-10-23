# Bluetooth (CLI)

Requires a build with `--features ble` and a bike with an E-TUBE Bluetooth gateway (e.g. SC-EM800); a bare motor has no radio. macOS needs Bluetooth permission for the terminal; Linux needs BlueZ and D-Bus.

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
cranklink --port 'ble:DEVICE_ID' units --stock-only
cranklink --port 'ble:DEVICE_ID' -s <SERIAL> settings read --output settings.json
```

`DEVICE_ID` is the peripheral ID from the scan; `<SERIAL>` is the target component serial from `probe`, not the Bluetooth ID. Add `--component <MODEL>` to target a catalogued Di2 component. The six-digit bike passkey is prompted for, or read from `--passkey-file` or `CRANKLINK_PASSKEY`.

`units --stock-only` skips optional part-number and model-discriminator GETs. Unresolved models remain candidates. Named component selection resolves only candidates for the requested model, so an unrelated unresponsive lever cannot block another target. Each new connection still discovers its own current slots.

Writes that need gateway master routing (destination, wheel circumference, lamp, lighting time) select it automatically; `--ble-master` overrides the policy.

## Disconnect and Di2 recovery

Discovery itself can open a PC-link session on the logical bus master, including a BT-DN110 battery. Normal/slave cleanup sends that master an explicit `32/10` stop and requires its matching `32/12` receipt before the gateway's `0C/00` stop. This also applies to inventory sessions that never selected a component. A missing, rejected or mismatched master receipt is a cleanup failure, even if Bluetooth closes successfully.

The pending start is tracked before gateway startup. A malformed discovery reply cannot erase an observed master start. If the start outcome or PC owner is unresolved, cleanup reports that uncertainty instead of claiming normal operation was restored. A verified identity or schema GET timeout permits one cleanup-only attempt when normal/slave discovery has established the logical master and its local PC owner. Ordinary reads and writes remain fenced. A missing gateway master-metadata reply also qualifies only if a local-owner start receipt was already observed; a missing start acknowledgement does not. Schema queries must match declared selectors, and an opcode used by any supported setter is excluded. That exception sends only target selection and explicit PC-link stops, requires matching stop receipts, and never retries the failed read. Write/authentication/firmware timeouts, foreign or unknown ownership, transport failures, oversized notifications and notification floods do not grant it; a later transport failure or flood revokes it.

Gateway acknowledgement and extinguished LEDs do not establish that the entire bike has returned to normal shifting. The observed EW-WU111/BT-DN110 failure and the limits of offline verification are recorded in the [Di2 BLE investigation](di2-ble-investigation.md). The explicit battery-master stop still needs validation on that hardware; simulator success does not establish it.

Firmware recovery is not available over Bluetooth; use the SM-PCE02. `firmware flash ble-pair` is experimental and has not been run on hardware.

## Staged Di2 read validation

For a bike that previously remained in service mode, validate one connection at
a time: authentication-only `ble test-link`, then `units --stock-only`, then
one read from the freshly selected named component. Use `--read-only` and a new
`--trace` path for every connection. Confirm shifting and the junction button
with the rider between stages. Do not reuse an old component slot or combine
the stages into an automatic retry loop. A failed read may receive one bounded
cleanup attempt; unconfirmed cleanup ends the test.
