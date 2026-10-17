# Offline firmware simulator

The headless simulator executes stock gateway and motor D firmware in separate Unicorn machines and connects them through a deterministic virtual packet bus. Use it for offline protocol development and read-only Cranklink integration checks. The browser GATT simulator remains available for UI tests.

This implementation covers the packet engine and native transport portion of the [simulator plan](firmware-simulator-plan.md). It does not yet provide complete board emulation or persistent settings writes. Firmware is supplied locally and identified by decoded SHA-256; an unfamiliar version never inherits another version's addresses.

## Verified firmware coverage

The compatibility run crosses every gateway profile with every motor profile: **80 combinations, nine scenarios each, 720 passing scenarios**. See the [recorded results](../tools/firmware-sim/compatibility.json). These are software packet checks with synthetic initialization, not physical compatibility certification.

| Display or gateway | Firmware file versions |
| --- | --- |
| SC-EM800 | 4.0.0, 4.1.0, 4.3.0, 4.4.0, 4.5.0, 4.6.0 |
| SC-E6100 | 4.1.0 |
| SC-E7000 | 4.1.0 |

| Motor | D firmware file versions |
| --- | --- |
| DU-EP800 | 4.1.10, 4.1.11, 4.2.0 |
| DU-EP600 | 4.4.1, 4.6.0 |
| DU-EP500 | 4.1.0, 4.1.1 |
| DU-E5000 | 4.5.0 |
| DU-E5100 | 4.1.0 |
| DU-E6100 | 4.8.0 |

File labels omit some internal revisions: EP600 4.6.0 reports D4.6.0.1; SC-EM800 4.5.0 reports 4.5.0.3. Profiles select bytes by hash, not filenames or labels. EP801 is not claimed from the EP600 image.

The nine scenarios exercise ordinary handshake/reads/cleanup; dropped, duplicated, reordered and delayed handshake keys; dropped replies; and truncated, reordered and incorrect-sequence reply fragments. Reads share machine state and cover a short lamp reply, destination reply and longer cargo reply. Reordering a single-fragment reply correctly leaves it valid. Logical delay is not a watchdog/timing test. Malformed-fragment scenarios replace the FIFO fixture per reply; they do not establish recovery of a continuous physical receive stream.

## Setup and run

Use Python 3.10+ and the optional pinned runtime:

```sh
python3 -m venv /tmp/cranklink-firmware-sim
/tmp/cranklink-firmware-sim/bin/pip install -r tools/firmware-sim/requirements.txt

/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py profiles
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py run \
  --gateway /path/to/SCEM800.4.6.0.decoded.bin \
  --motor /path/to/DUEP800-D.4.2.0.decoded.bin \
  --output /tmp/firmware-run.json
```

`run` executes all nine scenarios at the recovered slot 13. Output contains deterministic packet traces, instruction counts, cleanup results and modeled peripheral details. Unknown hashes, unhandled execution, instruction/event limits and failed invariants produce a nonzero exit. Tests remain enabled with `python -O`.

The files must already be decoded. A `.dat` that contains plaintext is accepted when its hash matches. Encrypted containers must first be decoded; firmware and decoding downloads are not performed by this tool.

### Inventory and test a local collection

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py inventory \
  --root /path/to/decoded-firmware \
  --output /tmp/firmware-inventory.json \
  --fixtures-output /tmp/firmware-fixtures.json

/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py matrix \
  --fixtures /tmp/firmware-fixtures.json \
  --output /tmp/firmware-matrix.json
```

Repeat `--root` to inventory more directories. Inventory distinguishes profiled, unprofiled and encrypted images; only exact profiled hashes enter the fixture manifest. A matrix executes every supplied profiled gateway/motor pairing and fails if any pairing fails. Relative image paths in a manually written manifest resolve against that manifest. A fixture manifest is an object mapping IDs such as `sc-em800-4.6.0` and `ep800-4.2.0` to local image paths.

The archived [motor inventory](../tools/firmware-sim/inventory-motors.json) and [display inventory](../tools/firmware-sim/inventory-displays.json) record the remaining local coverage gaps. They do not represent every published device/version. Broad support is the target; add a hash-pinned CPU/memory/ABI profile and execute the matrix before extending the support table. New processor families also need a suitable execution backend. SC-EN500/600/610, SC-E8000, SC-M9050, SC-MT800 and unprofiled variants are currently rejected.

## Run the real native CLI offline

On macOS or Linux, start a local endpoint:

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py serve \
  --gateway /path/to/SCEM800.4.6.0.decoded.bin \
  --motor /path/to/DUEP800-D.4.2.0.decoded.bin \
  --socket /tmp/cranklink-firmware.sock
```

In another terminal:

```sh
cargo build --locked --bin cranklink
./target/debug/cranklink --json --port sim:/tmp/cranklink-firmware.sock probe
./target/debug/cranklink --json --port sim:/tmp/cranklink-firmware.sock settings read lamp
```

`sim:` requires an absolute Unix socket path. Automatic device selection never opens a simulator. Windows and the WASM worker do not currently connect to this endpoint. The service handles one native session at a time and starts fresh firmware machines for each connection. Stop it with Ctrl-C; it refuses to replace an existing socket.

The endpoint models USB adapter power/control and discovery events. It runs the gateway's inventory parser, routing and receive reassembly, and the motor's PC-link, stock-query and normal-read code. The absent M application has a version-query behavior peer using the D container's compatibility bytes. Serial and parameter RAM start with synthetic values; EP800 cargo RAM uses a `00-CRG` fixture to exercise the existing settings schema. These values are not readings from a real motor. Existing Cranklink model/firmware/schema gates remain active: a successful probe does not make settings supported for that model.

The endpoint admits normal reads and PC-link setup/cleanup. Settings writes, authorization, reset, bootloader and firmware-update operations fail before dispatch. Only normal packets up to nine bytes and slot 13 are modeled. The wired endpoint exercises the native engine; the GATT endpoint below exercises the Bluetooth client.

## Run the Bluetooth client offline

Start the same server with `--gatt` and a separate socket:

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py serve --gatt \
  --gateway /path/to/SCEM800.4.6.0.decoded.bin \
  --motor /path/to/DUEP800-D.4.2.0.decoded.bin \
  --socket /tmp/cranklink-gatt.sock
./target/debug/cranklink --json --port ble-sim:/tmp/cranklink-gatt.sock --ble-master probe
./target/debug/cranklink --json --port ble-sim:/tmp/cranklink-gatt.sock --ble-master settings read lamp
```

This native Unix transport works without the `ble` Cargo feature. It runs the actual client authentication/session/routing/notification code against a modeled radio, public synthetic PIN `000000`, and stock SC-EM800 PCE handlers. All six profiled SC-EM800 versions support this endpoint. Other display profiles currently support only the wired packet endpoint. Use `--ble-master`: normal/slave logical-master-zero discovery is explicitly unsupported.

Radio pairing, challenge delivery, notifications, motor logical-address relocation during role handoff and the slave-radio stop acknowledgment are modeled topology boundaries. The actual motor stop handler must confirm disconnection. Passing cleanup does not validate physical role handoff or RF. Motor authentication, settings writes and persistent browser sessions remain unsupported. Use `--drop-replies`, or `--gatt-fault stale-free|duplicate-free`, to exercise response loss and notification handling.

The [GATT results](../tools/firmware-sim/gatt-validation.json) cover 17 real CLI cases, including wrong PIN, dropped responses, stale/duplicate notifications and unsupported topology. Stock lifecycle tests cover six gateways × ten motors, twenty reads per pair (1,200 total), followed by stop/cleanup. See [feature findings](simulator-feature-findings.md) and the [past PR assessment](simulator-pr-review.md). A successful read alone misses PR #11's duplicate startup; the separate pinned-revision regression checks that contract.

### Verification

```sh
# Synthetic tests always run; stock tests skip when no manifest is supplied.
/tmp/cranklink-firmware-sim/bin/python -m unittest discover -s tools/firmware-sim -p 'test_*.py'
CRANKLINK_SIM_FIXTURES=/tmp/firmware-fixtures.json \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover -s tools/firmware-sim -p 'test_*.py'

# Actual CLI probe, settings read/schema gates, and a lost-response timeout.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_native.py \
  --binary target/debug/cranklink \
  --fixtures /tmp/firmware-fixtures.json \
  --output /tmp/native-verification.json
```

Run `verify_gatt.py --binary target/debug/cranklink --fixtures /tmp/firmware-fixtures.json --output /tmp/gatt-verification.json` in the same Python environment for the real Bluetooth client and stock GATT lifecycle matrix. `verify_features.py` adds snapshot, batch and preflight checks; its commands are documented in [feature findings](simulator-feature-findings.md).

The native verifier selects four representative pairs when their profiles are present: SC-EM800/EP800, SC-E6100/EP600 and SC-E7000/EP800 and SC-E7000/E6100. It also tests the actual client's timeout with dropped GET replies. The [native results](../tools/firmware-sim/native-validation.json) record successful probes, settings reads and the expected E6100 model rejection. Firmware metadata checks and the virtual endpoint never weaken those CLI gates.

## Execution boundaries and remaining work

Each machine retains RAM and CPU state between bounded function calls. Firmware bytes remain unchanged. The motor receive entry and transmit-acceptance routines are modeled boundaries: a virtual queue acceptance allows firmware to consume its pending-response flag, while subsequent loss may still prevent delivery. The modeled host/radio consumer drains the real gateway TX ring. Event ordering is deterministic and separate from instruction budgets.

SC-E7000 additionally needs UART initialization and completion. The simulator runs its stock UART initializer and service routine, saves/restores the interrupted CPU context, and resumes at the same busy-poll instruction. The model supplies ready-status memory semantics; it does not reproduce interrupt timing or the radio. Combined instruction limits include this service work.

Full reset-to-main boot, complete MMIO/interrupt timing, persistent NVM writes, full unit authentication, actual motor M execution, BLE radio/SoftDevice and physical E-TUBE arbitration remain later stages. Plain mapped MMIO and synthetic RAM cannot establish hardware timing or firmware boot acceptance. Keep these boundaries visible when adding profiles or interpreting a passing report.
