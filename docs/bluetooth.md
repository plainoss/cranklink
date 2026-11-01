# Bluetooth (CLI)

Requires a build with the default `ble` feature (not `--no-default-features`) and a bike with an E-TUBE Bluetooth gateway (e.g. SC-EM800); a bare motor has no radio. macOS needs Bluetooth permission for the terminal; Linux needs BlueZ and D-Bus.

Native connection failures distinguish peripheral connection from service discovery.
These setup failures happen before the protocol trace is opened; stdout/stderr
captures are needed alongside `--trace` when investigating them.

When connecting to a known peripheral ID, the native CLI checks discovery every
100 ms and stops scanning as soon as that device appears, instead of always
waiting five seconds. If it is absent, discovery still waits up to five seconds.
An explicit `ble scan` keeps its full requested duration. Connection and command
failures are not retried.

Native sessions also watch the discovery adapter's disconnect events for the
selected peripheral. A confirmed disconnection interrupts pending operations
and avoids querying a peripheral that the Bluetooth backend has already removed.
This shortens failure reporting; it does not restore gateway role or bike controls.
Normal discovery requires an explicit successful gateway service-start status
before querying the bus; a refused or truncated receipt ends that attempt.

### SC-E8000 4.0.6 native hardware observation

On 2026-10-31, a macOS native read-only test on SC-E8000 gateway firmware
4.0.6.0 with a DU-E8000 reproduced the master-setup failure previously reported
on Android Chrome. Link authentication and normal-routing stock inventory passed
on separate connections, with normal controls confirmed after the link test.
The master attempt read the gateway revision and stock identity, received
`23/00` for `PCE03/20`, then sent `PCE04` 1003 ms after that acknowledgement.
There was no mode reply; native GATT reported a device disconnection, and cleanup
could not confirm PC-link release. Bike controls remained unavailable until a
power cycle, after which the rider confirmed normal operation.

The native build used commit `222a4452042d` plus setup-error context and the
early peripheral-discovery change described above. No PnP, component ownership
verification or settings writes were reached. This establishes reproduction
beyond Android/Chrome, not a radio-level root cause. macOS Bluetooth logs record
a link timeout near the PCE04 write; a transmission trace records the attempted
write, not proof of delivery or proof that it caused the link failure.

An isolated diagnostic subsequently sent `PCE03/20` once and passively observed
the connection without sending PCE04, PnP, settings or protocol cleanup commands.
One attempt observed no disconnection for three seconds before deliberately
closing GATT. A later attempt, with a three-second initial slave-settling delay,
received the master acknowledgement but macOS logged a link timeout about
1.06 seconds afterwards, before any subsequent GATT write. The native observation
operation eventually timed out. Thus PCE04 is not necessary for this link failure;
the master acknowledgement alone still does not establish that the asynchronous
role transition completed. The inconsistent passive outcomes do not validate a
longer delay as a fix. The exact gateway/radio transition remains unmodeled.

A further native diagnostic connected promptly, held GATT for seven seconds
before protocol startup and confirmed in macOS logs that the supervision timeout
had changed from 360 ms to 6000 ms before requesting master mode. Initial slave
confirmation and gateway identity reads succeeded. Master `23/00` was received,
but the subsequent PCE04 still got no reply and the link timed out about
6.7 seconds after that acknowledgement. Waiting for the initial connection
parameter negotiation therefore did not resolve the reported failure.

Starting pairing explicitly through the SC-E8000's Bluetooth LE menu also
reproduced the same acknowledged master request followed by PCE04 disconnection.
Automatic power-on advertising alone therefore does not explain this failure.

A 2026-11-01 native read-only diagnostic established normal gateway service and
successfully inventoried all four components before sending one master request.
The gateway acknowledged it, but the first PCE04 write, 1000 ms later, failed;
macOS reported lost synchronization and disconnection about 1.03 seconds after
the acknowledgement. The rider confirmed that the bike stayed powered throughout
the attempt and that controls recovered after a subsequent power cycle. This
tested service-first ordering is insufficient on this bike. No master mode reply,
PnP or settings write was reached, and the full-master follow-up was not run.

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

Cranklink's current routing policy selects gateway [master routing](../README.md#terms) for market region, wheel circumference, lamp and lighting-time writes (`--ble-master=auto`, the default). This policy does not establish that every E8000 firmware requires master routing; normal-routing settings writes on the tested D4.6.0 bike have not been hardware validated. `--ble-master` forces master routing and `--ble-master=normal` forces normal routing; the value needs `=`, as `--ble-master normal` reads `normal` as a subcommand. Drive-unit authentication for protected fields, and `--unit-auth`, work as over USB; see [protected writes](settings.md#protected-writes).

The CLI reports that automatic routing choice as a policy. Explicit normal routing still applies the usual identity, session, authentication and read-only checks; selecting it does not establish that a particular bike supports the requested write.

## Disconnect and Di2 recovery

When you are done, press **Disconnect** (or let the CLI command finish) and wait until the display, buttons and shifting respond normally before riding. The rest of this section explains how the link is closed.

Ordinary normal/slave sessions end by closing GATT, leaving the gateway firmware to handle its radio-disconnect event. They do not send an explicit logical-master `32/10` stop, gateway `0C/00` stop or gateway reset. This matches the standard client normal-disconnect path. Explicit stop acknowledgements did not restore either tested bike; closing GATT alone preserved normal operation on both EW-WU111/BT-DN110 Di2 and SC-EM800/EP800 STEPS hardware.

The browser retains its normal/slave service session between commands without stopping the logical master. Final Disconnect closes GATT. Changing routing roles on an active retained session requires disconnect/reconnect. Explicit master-routing sessions keep their acknowledged component stop and routing-restoration sequence. Both initial authentication and restoration send normal/slave mode, wait one second and read PCE4. An immediate PCE3 acknowledgement is optional: stock SC-E8000 4.0.7, all six profiled SC-EM800 versions, SC-E6100 4.1.0 and SC-E7000 4.1.0 transition without it. The same readback logic covers initial normal-mode discovery, without gateway or motor allowlists. Explicit rejection still fails, and missing mode confirmation poisons the session before any further cleanup command. This fixes a reproducible teardown timeout and the same timeout when reconnecting to a gateway left in master mode; it does not establish that the initial master connection failure on the reported bike is resolved.

The web app keeps **Disconnect** visible in its top bar, including on phones. It shows progress while closing the link, then a completion screen explaining that the bike may need a few seconds to return to riding mode. The browser cannot observe that physical transition; the completion screen stays until you choose **Back to connection screen**. A failed cleanup remains an error rather than showing successful Bluetooth closure.

Read and write errors still fail the operation; closing the link never converts an error into a successful read or write. A failed command is not retried. Link closure and normal physical operation are distinct observations: confirm shifting, display and controls when validating a new gateway. See the [Di2 BLE investigation](di2-ble-investigation.md) for the successful physical checks, rejected experiments and simulator limits.

The web app detects supported master families before starting service, keeping the original GATT connection. It queries gateway master metadata (`00 0d 00`) after normal/slave authentication, before PC-link (`00 0c 01`). Supported STEPS families select the existing master-routing workflow; BM-DN100/BT-DN110/BT-DN300 master families select ordinary routing. Di2 components on a STEPS bike do not change that decision.

E8000/E8080 (master series `12` hex) is an exception: automatic detection selects normal routing for discovery, component selection, and subsequent settings commands, while retaining the STEPS label and identity checks. This follows successful E8000 D4.6.0 inventory and motor reads on normal routing; the automatic selection still needs a physical preview check. The explicit STEPS override retains the master workflow unless the experimental normal-routing option below is selected. A failed master connection is never retried automatically on another route.

The browser blocks destination, wheel circumference, lamp and lighting-time saves/restores on normal routing by default. To test E8000/E8080 region and wheel writes, disconnect and select **Bluetooth options → Experimental E8000 region & wheel writes → Allow experimental writes in normal routing**, then reconnect and read the settings again. This explicitly selects normal routing, including when the STEPS identification override is selected, and cannot be changed on an active connection. Only the `e8000` profile's region and wheel fields gain this exception; other write guards remain. Authentication, serial-bound backups, verified readback and restore still apply. Hardware writes and persistence remain unvalidated. See [E8000 settings](settings.md#e8000--e8080-drive-units) and the [Android option](../android-app/README.md#experimental-e8000-normal-routing).

An unconfirmed component PC owner ends usable access to that session and requires disconnect/reconnect. Diagnostics retain the target slot, expected owner and received startup bytes; the app does not accept a foreign owner or retry the start. If connection cleanup also fails, the connection error remains visible alongside cleanup guidance. A normal-routed motor read succeeding at logical slot zero does not establish that shifter or derailleur sessions will succeed.

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

## Failed-connection diagnostics

On the connection screen, select **View details & logs**, then **Copy diagnostic
report**. This remains available after setup or cleanup fails, without another
connection attempt. Clipboard denial offers selectable report text. Copy before
reloading: logs stay in this tab's memory. The report includes the deployed
revision so production and preview builds can be distinguished.

GATT write errors identify the PCE command (including the role for PCE3), or the
unit slot/group/command. Parameters and authentication material are omitted.
Once the gateway revision and stock identity have been read, master-setup errors
also include those identifiers. A failed write still poisons the session and is
never retried; this diagnostic context does not establish that an unmodeled
physical master-mode failure has been repaired.
