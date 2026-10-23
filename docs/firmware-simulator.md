# Offline firmware simulator

The [E-TUBE guide](etube-protocol.md) explains discovery, routing and component-specific operations, with links to the implementation's exact encodings. Packet-matrix success and complete client probe success are separate checks. The simulator initializes explicit missing-M version and model caches for the profiled drive units; stock handlers consume those caches. Clearing the version cache reproduces the `01/2C` rejection that previously blocked some complete client probes. These fixtures do not replace query responses or establish full boot execution.

The headless simulator executes stock gateway and motor D firmware in separate Unicorn machines and connects them through a deterministic virtual packet bus. Use it for offline protocol development, Cranklink read integration and targeted persistent-write/browser-session regressions. The existing browser GATT simulator remains available for UI tests.

This implementation covers the packet engine, native/Bluetooth transports, targeted persistent-write paths and a separate EP800 resident-bootloader regression. Drive-unit writes require the exact EP800 D4.2.0 profile and support lamp and wheel circumference. Separate component fixtures execute cycle-computer settings and MU-UR500 gear-count changes. Complete board emulation remains outside scope. Firmware is supplied locally and identified by decoded SHA-256; an unfamiliar version never inherits another version's addresses.

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

# Inspect executable profiles and check local cache status
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py pull --list

# Auto-pull and cache firmware for given hardware (e.g. EP800)
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py pull --hardware ep800

# Run 9 fault scenarios directly using the hardware preset (auto-pulls if needed)
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py run \
  --hardware ep800 \
  --output /tmp/firmware-run.json

# Or specify profile IDs or custom local file paths explicitly
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py run \
  --gateway sc-em800-4.6.0 \
  --motor ep800-4.2.0 \
  --output /tmp/firmware-run.json
```

`run` executes all nine scenarios at the recovered slot 13. Output contains deterministic packet traces, instruction counts, cleanup results and modeled peripheral details. Unknown hashes, unhandled execution, instruction/event limits and failed invariants produce a nonzero exit. Tests remain enabled with `python -O`.

### Firmware auto-pulling and local caching

Firmware containers can be auto-pulled directly by profile ID, model name, or Shimano filename from official Shimano endpoints (`https://api.shimano.com/etube/public/data/upload/published/`). Downloaded images are validated against hash-pinned SHA-256 profiles, automatically decrypted (if container-encrypted) into plaintext binaries, and cached locally in `tools/firmware-sim/.cache/firmware/` (gitignored, never committed). You can also override the cache path via `--cache-dir` or `CRANKLINK_FW_CACHE`.

- `sim.py pull --list`: Lists all 18 profiled targets, models, versions, and local cache status.
- `sim.py pull [NAMES...]`: Pulls and decodes specific profile IDs (e.g. `ep800-4.2.0`, `sc-em800-4.6.0`) or filenames.
- `sim.py pull --hardware <name>`: Pulls the default matching gateway and motor pair for hardware (e.g. `ep800`, `ep600`, `ep500`, `e6100`, `e5000`, `e5100`).
- `sim.py pull --all --fixtures-output /tmp/firmware-fixtures.json`: Pulls all 18 profiled firmware images and generates a fixture manifest ready for `sim.py matrix`.
- `sim.py run` and `sim.py serve` accept `--hardware <model>` or profile names directly in `--gateway` / `--motor`, automatically fetching and decoding missing images on demand. Existing local `.decoded.bin` or plaintext `.dat` file paths remain fully supported.

### Di2 research inputs

`acquire_di2.py` downloads six SHA-256-pinned research files from Shimano's
published firmware endpoint. It verifies DCAS2i main-image headers and Intel HEX
records without registering any executable profiles or connecting to hardware.
Downloaded binaries stay outside Git. The default directory is ignored:

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/acquire_di2.py

# Revalidate cached inputs without network access.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/acquire_di2.py --offline

# Include the acquired-input checks in the simulator test suite.
CRANKLINK_DI2_IMAGES="$PWD/tools/firmware-sim/.cache/di2" \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover \
  -s tools/firmware-sim -p 'test_di2_images.py'
```

Use `--output /path/to/cache` to choose a different directory. A missing offline
file or a mismatched hash fails without replacing the input. The generated
`di2-acquisition.json` and [recorded assessment](../tools/firmware-sim/di2-acquisition.json)
describe acquisition only: zero ARM execution profiles and no validated topology.
The separate experimental service profiles below are not inferred from acquisition.

| Research input | Purpose | Current execution gap |
| --- | --- | --- |
| `BTDN110.4.5.0.dat` | BT-DN110 board 1 master application | PC-link start executes; stop runs to its actual receipt, then pauses before housekeeping. |
| `2BTDN110.4.5.0.dat` | BT-DN110 board 2 master application | Separate hash/entry; the same partial service coverage as board 1. |
| `EWWU101.4.6.0.dat` | WU101/WU111 main application family | PC-link start executes. Stop returns with an opt-in exact interrupt-control write fixture; strict execution traps at 0x3FF4. Model mapping comes from the E-TUBE application declaration. |
| `UPDATENRF-DI2-ap.4.7.1.hex` | Separate Nordic radio application | Plausible mapped Thumb reset vector; no recovered gateway handler profile. |
| `UPDATENRF-DI2-bt.1.4.0.hex` | Nordic bootloader | Acquired and format-checked only. |
| `UPDATENRF-sd.3.0.0.hex` | Nordic SoftDevice package | Acquired and format-checked only; RF behavior is not emulated. |

### Di2 service execution and timeout regression

The bounded MN101 interpreter executes original instructions from the acquired
main images. It models initialized RAM and a synthetic function call frame;
unknown instructions, uncertain REP/MULU/DIVU/ASR semantics, handy memory and
unmodeled peripherals trap. An opt-in wireless fixture admits only the exact
ordered interrupt-control writes recovered from the pinned handler. It rejects unsupported PC bank bits rather than
truncating them. This is a service test harness, not a full-chip emulator.
Instruction encodings are corroborated against Panasonic's
[MN101C49 manual](https://bitsavers.org/components/panasonic/panaXseries/21449-041E_MN101Cx49x.pdf),
including independent worked examples in the decoder tests.

```sh
# Execute acquired main-image PC-link handlers without BLE or network access.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/di2_service.py \
  --output /tmp/di2-services.json
# Continue the wireless main stop through an explicit interrupt-register fixture.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/di2_service.py \
  --model-wireless-interrupts --output /tmp/di2-services-with-irq-fixture.json

# Actual CLI regression, with a modeled gateway and battery firmware execution.
cargo build --locked --features ble --bin cranklink
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_di2.py \
  --binary target/debug/cranklink \
  --image tools/firmware-sim/.cache/di2/BTDN110.4.5.0.dat \
  --expect receipt --output /tmp/di2-timeout.json
# Repeat with 2BTDN110.4.5.0.dat for board 2.
# Lose metadata after an explicitly modeled local-owner start notification.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_di2.py \
  --binary target/debug/cranklink \
  --image tools/firmware-sim/.cache/di2/BTDN110.4.5.0.dat \
  --case metadata-timeout --expect receipt --output /tmp/di2-metadata-timeout.json
# Reproduce the original component part-GET failure class.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_di2.py \
  --binary target/debug/cranklink \
  --image tools/firmware-sim/.cache/di2/BTDN110.4.5.0.dat \
  --case component-timeout --expect receipt --output /tmp/di2-component-timeout.json
# The same fixture succeeds without that optional query.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_di2.py \
  --binary target/debug/cranklink \
  --image tools/firmware-sim/.cache/di2/BTDN110.4.5.0.dat \
  --case stock-only --expect stock --output /tmp/di2-stock-only.json

CRANKLINK_DI2_IMAGES="$PWD/tools/firmware-sim/.cache/di2" \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover \
  -s tools/firmware-sim -p 'test_di2*.py'
```

`--images` overrides the service tool's image directory. The battery start
handler returns after 128 original instructions. Its stop path generates a
mode-zero receipt, where execution deliberately pauses before unsupported
housekeeping. The WU main start returns. Strict stop traps at peripheral
`0x3FF4`; with `--model-wireless-interrupts`, the exact eight interrupt-control
writes are consumed and the stop handler returns after 1,199 original
instructions. Repeated start/stop calls retain RAM and restore the stack.
The fixture models write acceptance only; interrupts, timers, RF and physical
register effects remain unexecuted. A paused or trapped machine cannot accept
another service call. No result here establishes physical shutdown or shifting.

The battery receive ABI, zero RAM fixture and JSRV4 send/queue boundary are
modeled. The stop receipt is generated by original packet-building code. It
echoes the requested owner, so it does not replace the client's prior ownership
check. Nordic radio execution, PCE gateway firmware, bus delivery and timing,
boot/NVM initialization and derailleur settings remain unvalidated. The CLI
regression models gateway acknowledgements, discovery data and metadata. It
deliberately loses a master identity, metadata or component part GET reply,
then verifies real start, retained mode or actual stop receipt, and explicit
stop ordering. The metadata case injects a captured local-owner start
notification; that asynchronous notification path is not firmware execution.
Use `--expect stranded` with the old CLI to reproduce
the pre-fix cleanup omission. The [recorded validation](../tools/firmware-sim/di2-service-validation.json) contains image/source hashes, before/after results and execution limits. See the [investigation](di2-ble-investigation.md#firmware-backed-timeout-reproduction).

The public catalogue at `https://api.shimano.com/etube/firmware/5.4.6` supplied
the filenames, sizes, MD5 values and download URLs. Initial acquisition checked
those sizes and MD5 values, then pinned the raw SHA-256 values used by the tool.
The endpoint's `5.4.6` is not an assertion about the app or installed bike version.

The separate mapping evidence is the inspected E-TUBE application constructor
`com.shimano.etubeproject.sharedcode.domain.model.unit.category.eww.EWWU111Unit`:
its main firmware declaration selects `EWWU101` and its wireless-chip declaration
selects the `UPDATENRF-DI2` family. The inspected APK SHA-256 is
`2b16e03a73d06997ac15962813bffd8a8e95e2a823bfc73a81d5d46a4fbbefe5`.
This does not identify which board, radio revision or firmware is installed on a
particular bike. No proprietary firmware or decompiled application is bundled.

Legacy main-image vectors corroborate low-first nibble decoding at byte origin
`0x4000`: the exact board-1 battery, board-2 battery and WU-family images have
19, 18 and 21 vectors respectively beginning with MN101-family RTI encoding.
This is static mapping evidence, not service-handler execution or identification
of a specific physical MCU. The instruction-family reference is Panasonic's
[MN101Cx49x manual](https://bitsavers.org/components/panasonic/panaXseries/21449-041E_MN101Cx49x.pdf).
The current public execution backend is ARM-only. A separate experimental
decoder/executor can return isolated interrupt stubs but has unresolved ISA
semantics and is not admitted as a verified protocol backend.

To obtain topology coverage, first validate a strict MN101 backend, recover the
exact PC-link/discovery handlers and their RAM/transport context in both main
images, then connect those stock handlers through explicit modeled bus/radio
boundaries. Tests must distinguish a gateway acknowledgement from a master
stop and retain master state over radio disconnects. Executing only the Nordic
radio cannot establish the battery master's ownership or shifting behavior.

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
  --hardware ep800 \
  --socket /tmp/cranklink-firmware.sock
```

Or pass explicit profiles/paths (`--gateway sc-em800-4.6.0 --motor ep800-4.2.0`). In another terminal:

```sh
cargo build --locked --bin cranklink
./target/debug/cranklink --json --port sim:/tmp/cranklink-firmware.sock probe
./target/debug/cranklink --json --port sim:/tmp/cranklink-firmware.sock settings read lamp
```

`sim:` requires an absolute Unix socket path. Automatic device selection never opens a simulator. Windows and the WASM worker do not currently connect to this endpoint. The service handles one native session at a time and starts fresh firmware machines for each connection. Stop it with Ctrl-C; it refuses to replace an existing socket.

The endpoint models USB adapter power/control and discovery events. It runs the gateway's inventory parser, routing and receive reassembly, and the motor's PC-link, stock-query and normal-read code. The absent M application has a version-query behavior peer using the D container's compatibility bytes. Serial and parameter RAM start with synthetic values; EP800 cargo RAM uses a `00-CRG` fixture to exercise the existing settings schema. EP600 D4.6.0 uses a synthetic standard `00` model cache, read by the stock GET handler, so the current client can resolve the model without relaxing identity guards. These values are not readings from a real motor. Existing Cranklink model/firmware/schema gates remain active: a successful probe does not make settings supported for that model.

The endpoint admits normal reads and PC-link setup/cleanup. Settings writes, authorization, reset, bootloader and firmware-update operations fail before dispatch. Only normal packets up to nine bytes and slot 13 are modeled. The wired endpoint exercises the native engine; the GATT endpoint below exercises the Bluetooth client.

## Run the Bluetooth client offline

Start the same server with `--gatt` and a separate socket:

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py serve --gatt \
  --hardware ep800 \
  --socket /tmp/cranklink-gatt.sock
./target/debug/cranklink --json --port ble-sim:/tmp/cranklink-gatt.sock --ble-master probe
./target/debug/cranklink --json --port ble-sim:/tmp/cranklink-gatt.sock --ble-master settings read lamp
```

This native Unix transport works without the `ble` Cargo feature. It runs the actual client authentication/session/routing/notification code against a modeled radio, public synthetic PIN `000000`, and stock SC-EM800 PCE handlers. All six profiled SC-EM800 versions support this endpoint. Other display profiles currently support only the wired packet endpoint. Use `--ble-master`: normal/slave logical-master-zero discovery is explicitly unsupported.

Radio pairing, gateway PIN challenge delivery, notifications, motor logical-address relocation during role handoff and the slave-radio stop acknowledgment are modeled topology boundaries. The actual motor stop handler must confirm disconnection. Passing cleanup does not validate physical role handoff or RF. This socket service remains read-only. Use `--drop-replies`, or `--gatt-fault stale-free|duplicate-free`, to exercise response loss and notification handling. The separate persistent browser peer below enables narrowly scoped writes.

Closing a GATT endpoint does not send firmware stop commands or reset the powered device. Retained-device tests preserve a stranded PC-link owner across disconnect/reconnect, then require an explicit protocol stop to clear it. The former coordinator cleanup on radio close masked this class of client failure. These stock tests cover SC-EM800/motor ownership. Separate Di2 service profiles execute battery/WU main handlers, with the partial scope below; the complete EW-WU111/BT-DN110 slave topology remains unvalidated. See the [Di2 BLE investigation](di2-ble-investigation.md).

The [GATT results](../tools/firmware-sim/gatt-validation.json) cover 17 real CLI cases, including wrong PIN, dropped responses, stale/duplicate notifications and unsupported topology. Stock lifecycle tests cover six gateways × ten motors, twenty reads per pair (1,200 total), followed by stop/cleanup. A successful read alone misses PR #11's duplicate startup; the separate pinned-revision regression checks that contract.

## Persistent writes and retained browser sessions

The opt-in `browser_peer.py` coordinator retains one powered virtual bike across GATT reconnects. Its exact EP800 D4.2.0 storage fixture admits lamp SET/GET, wheel-circumference SET/GET and the motor's regulation-authentication commands. Motor challenge generation, proof verification/AES, authorization checks, setting validation, RAM updates, EEPROM record staging/checksums and reply construction execute stock firmware instructions. There is no authorization-success bypass. The synthetic gateway PIN remains a separate modeled credential.

The unavailable physical EEPROM commit/ready boundary is modeled with an atomic local JSON write. Boot hydration of selected RAM caches is modeled when new firmware machines reload that file. Seeds are synthetic: lamp `0`, wheel circumference `2149` mm and serial `000000000000`. The fixture is hash-fenced to this exact motor image; it does not enable writes for the full compatibility matrix or arbitrary settings. Interrupted commits, EEPROM electrical behavior/endurance and full hardware boot are unvalidated.

The browser regression runner uses unchanged historical Rust/WASM, `worker.js`, `host.js`, `WebBleGatt` and `BrowserFiles` in Chromium. A test peripheral forwards GATT operations to stock gateway/motor execution. The same master-routed commands save lamp `1 → 2`, or protected wheel circumference `2150 → 2151`, before reading again and explicitly disconnecting. It verifies actual changed receipts, durable IndexedDB backups before each SET, commit counts, ownership release, retained routing, cleanup, a new motor instance and a firmware GET after disk reload. Both source versions run with fresh state under always-advertising and connection-mode-ending controls.

The [lamp report](../tools/firmware-sim/browser-pr11-lamp-validation.json) and [protected wheel report](../tools/firmware-sim/browser-pr11-tire-validation.json) reproduce PR #11's repeated-save failure under the explicit connection-mode model: the old version shuts down after save one and cannot reconnect for save two; the fixed version completes both saves on one connection. In the always-advertising control, both versions save successfully, but only the fixed version retains the connection. This is browser-worker/protocol/storage integration, not complete UI clicking or RF validation.

### Run the pinned browser comparison

In addition to the simulator Python runtime, install the Rust WASM target, the `wasm-bindgen` CLI matching the pinned lockfiles (`0.2.129`), Node, the project's `@playwright/test` package and Chromium:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm --prefix web-instrument ci
cd web-instrument
npx playwright install chromium
cd ..

/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_browser_pr11.py \
  --gateway /path/to/SCEM800.4.6.0.decoded.bin \
  --motor /path/to/DUEP800-D.4.2.0.decoded.bin \
  --setting lamp --work-dir /tmp/cranklink-browser-builds \
  --output /tmp/browser-lamp-validation.json

/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_browser_pr11.py \
  --gateway /path/to/SCEM800.4.6.0.decoded.bin \
  --motor /path/to/DUEP800-D.4.2.0.decoded.bin \
  --setting tire-circumference --work-dir /tmp/cranklink-browser-builds --skip-build \
  --output /tmp/browser-tire-validation.json
```

The script uses pinned Git objects `0cfb180…` and `158e8f5…`, builds unchanged source versions and serves local browser modules over loopback. `--work-dir` keeps local builds for reuse; `--skip-build` requires both builds there. Each invocation creates fresh scenario state despite build reuse. Omit both options for temporary builds. `--playwright-module` can identify a separately installed Playwright module; otherwise the project package is used. Reports contain sanitized counts, synthetic values and source/WASM hashes, without firmware, proofs, backups or raw transport data.

The coordinator's JSON-lines API is used by the harness: `connect` opens a fresh GATT session on the same powered bike; `request` forwards characteristic operations; `disconnect` closes GATT; `snapshot` returns sanitized lifecycle/storage state; `power_cycle` reconstructs machines from the storage file; `reenable` explicitly restores modeled connection mode. The internal `GattDevice(..., storage_path=..., allow_saves=True, radio_lifecycle=True)` path enables this fixture. `sim.py serve` and `serve --gatt` do not expose write-enabling flags.

### Verification

```sh
# Synthetic tests always run; stock tests skip when no manifest is supplied.
/tmp/cranklink-firmware-sim/bin/python -m unittest discover -s tools/firmware-sim -p 'test_*.py'
# Optional independent host AES proof calculation for stock auth/write tests.
/tmp/cranklink-firmware-sim/bin/pip install -r tools/firmware-sim/requirements-test.txt
CRANKLINK_SIM_FIXTURES=/tmp/firmware-fixtures.json \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover -s tools/firmware-sim -p 'test_*.py'

# Actual CLI probe, settings read/schema gates, and a lost-response timeout.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_native.py \
  --binary target/debug/cranklink \
  --fixtures /tmp/firmware-fixtures.json \
  --output /tmp/native-verification.json
```

Run `verify_gatt.py --binary target/debug/cranklink --fixtures /tmp/firmware-fixtures.json --output /tmp/gatt-verification.json` in the same Python environment for the real Bluetooth client and stock GATT lifecycle matrix. `verify_features.py` adds snapshot, batch and preflight checks:

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_features.py --binary target/debug/cranklink \
  --fixtures /absolute/path/fixtures.local.json --output /tmp/cranklink-feature-results.json
```

It runs one pair, SC-EM800 4.6.0 with EP800 4.2.0 by default (profiles `sc-em800-4.6.0` and `ep800-4.2.0`, which the fixtures file must contain); choose another with `--gateway <profile> --motor <profile>`. Reports contain raw CLI output and should stay local.

The native verifier selects four representative pairs when their profiles are present: SC-EM800/EP800, SC-E6100/EP600, SC-E7000/EP800 and SC-E7000/E6100. It also tests the actual client's timeout with dropped GET replies. Firmware metadata checks and the virtual endpoint never weaken CLI gates.

The [archived native results](../tools/firmware-sim/native-validation.json) predate the explicit version/model-cache fixtures: they record an expected model rejection. Earlier bare-D SC-E7000 4.1.0 / E6100 D4.8.0 probes reached `01/2C 00` and received `01 2F 39 00 FF FF`, causing `verify_native.py` to fail despite a passing packet matrix. Queries with selectors 0–3 reproduced that rejection while `01/84 00` returned `01 86 48 00 00 00` (4.8.0.0).

The current exact-hash profiles initialize the missing M/RX version cache consumed by the stock handler. Normal `01/2C` queries and the native verifier now pass, including SC-E7000/E6100; clearing that cache reproduces the earlier rejection. The latest five native verifier groups, including the expected dropped-reply timeout, pass against the recorded CLI binary. See [current validation](../tools/firmware-sim/component-validation.json) for the base commit, binary hashes and results. Cache initialization remains a modeled boundary rather than execution of M firmware or complete boot.

## EP800 resident bootloader and Key 2 recovery

The recovery investigation added a separate regression using the 64 KiB resident bootloader read from a working EP800. It reproduces how repeated empty SPI replies can keep Key 2 queued, and how a first SPI byte of `0xff` lets the queued key advance. The physical bridge procedure is documented in the [recovery guide](ep800-hardware-recovery.md).

Supply your own acquired resident dump; no donor dump is included or downloaded. The harness accepts only the exact `ep800-resident-5.0.6.1` image, SHA-256 `59cc3fc007ca0b3595d8ba8e8207cc5257b05be84da8fd80278303d165488d9f`. A matching version string alone is insufficient because the code and RAM addresses are specific to those bytes.

```sh
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/sim.py resident \
  --image /path/to/resident.bin --output /tmp/resident-regression.json

# Include the acquired-image tests alongside the synthetic tests.
CRANKLINK_RESIDENT_IMAGE=/path/to/resident.bin \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover \
  -s tools/firmware-sim -p 'test_*.py'
```

The five scenarios check:

| Modeled condition | Required result |
| --- | --- |
| SPI polling inhibited | Key 2 is acknowledged. |
| SPI input stays zero | Empty `0x70` replies repeat; Key 2 remains queued. |
| SPI input stays `0xff` | Key 2 is acknowledged. |
| SPI changes from zero to `0xff` after stalling | The already queued Key 2 is consumed, then all five keys are acknowledged in order. |
| UART transmission never completes | Only the first transmission starts; later replies do not bypass the busy flag. |

The real RAM initializer, FIFO parser, key validators, SPI polling/checksum and UART transmit/busy routines execute. Unlike the first investigation fixture, UART transmit is observed without replacing its return path. The harness supplies decoded frames at the RX FIFO, a constant SPI receive-register value and UART completion between service slices. It does not run M firmware, erase/write/reset operations, full boot, interrupts or physical E-TUBE timing. The `0xff` case establishes a software escape condition; it does not predict which board voltage or resistor produces that condition.

### CPU model and instruction compatibility

Execution now selects a named Unicorn CPU model instead of relying on an implicit default. Existing application profiles use `cortex-m3` unless their `cpu` field specifies another supported model; `resident --cpu` accepts `cortex-m0`, `cortex-m3` and `cortex-m4`. These are execution settings, not identification of the physical processor.

Selecting Cortex-M0 in Unicorn 2.1.4 still permits the Thumb-2 `MOVW` sequence `41 f2 70 70`. A successful emulation therefore does **not** prove ARMv6-M instruction compatibility or that a modified binary boots on hardware. Check instruction encodings separately when changing firmware, and keep hardware boot verification distinct from a simulator pass.

## Component settings execution

The component fixtures run a separate stock target CPU behind the existing stock gateway. Their native and modeled-GATT endpoints admit only each fixture's explicit identity, PC-link and setting command set. They do not install a motor RX-version peer or grant drive-unit authorization. Exact coverage and aggregate results are recorded in [component validation](../tools/firmware-sim/component-validation.json); the older archived native report above predates the explicit version/model-cache fixtures.

| Target | Executable firmware | Settings exercised |
| --- | --- | --- |
| MU-UR500 | 3.3.1 | Signed adjustment GET, gear-count GET/SET, invalid values, record checksums and modeled persistence |
| SC-EM800 | 4.0.0, 4.1.0, 4.3.0, 4.4.0, 4.5.0, 4.6.0 | Beep, distance unit, language, brightness |
| SC-E6100 | 4.1.0 | Beep, distance unit, language, backlight |
| SC-E7000 | 4.1.0 | Beep, distance unit, language |

Component parsing, identity/version/serial replies, all five PC-link keys, settings validation, logical EEPROM-record staging/checksums, cache changes, schedulers and reply construction execute unchanged firmware instructions. Physical storage service/ready, initial factory caches and reload hydration are explicit models; flash allocation/compaction, complete boot and physical EEPROM behavior are not executed. Display checks include every byte value for every exposed setting, mixed-field changes, sibling preservation, restore, rejected/normalized values and failed modeled commits. The CLI runs additionally check complete backups before each SET, batch ordering, new-CPU disk reloads and uncertain outcomes after a dropped SET acknowledgment.

MU adjustment is asynchronous: firmware can acknowledge acceptance while GET still reports the old position. The fixture deliberately leaves the actuator unfinished. The actual CLI must send one SET, poll for completion, then retain its backup and report an uncertain outcome if completion never arrives. Firmware execution also verifies that setting the already-reported current value replaces a different pending target; Cranklink therefore applies a hub-adjustment target once even when its `changed` receipt is false. No actuator success is fabricated.

The current web UI and WASM also run against unchanged SC-EM800 4.6.0 and MU-UR500 3.3.1 through `component_browser_peer.py`. The opt-in browser suite clicks schema controls and save/restore confirmations, checks fresh durable IndexedDB backups before every actual SET, observes stock commits, creates a new firmware machine after a controlled power cycle, and verifies values by firmware GET before restoring them. It also checks that unfinished MU adjustment sends one SET, retains the staged value and backup, and reports uncertainty. GATT radio delivery, initial caches and physical NVM services remain modeled. See [browser test setup](../web-instrument/README.md#test) and [web validation results](../tools/firmware-sim/web-component-validation.json), which identify exact firmware, source and browser WASM hashes separately from the earlier CLI validation.

Most locally inspected classic lever, derailleur and power-unit images, plus MU-S705, still lack a verified CPU/mapping/execution profile. Their catalog schemas retain source and scripted-protocol coverage, but these images are **not** counted as firmware-simulated. Other catalogued models and versions outside the explicit profile lists also remain unverified by instruction execution, including MU-UR510/UR520 and other cycle computers. Modern Thumb images cannot stand in for a different classic model.

```sh
# Existing gateway/motor manifest: profile ID -> absolute local image path.
# Component manifest additionally maps mu-ur500-3.3.1 to its decoded image.
CRANKLINK_SIM_FIXTURES=/tmp/firmware-fixtures.json \
CRANKLINK_COMPONENT_FIXTURES=/tmp/component-fixtures.json \
  /tmp/cranklink-firmware-sim/bin/python -m unittest discover \
    -s tools/firmware-sim -p 'test_*.py'

# Current CLI, genuine MU target; use --transport gatt for the BLE client.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_components.py \
  --fixtures /tmp/firmware-fixtures.json \
  --component-image /path/to/MUUR500.3.3.1.dat.decoded.bin \
  --binary target/debug/cranklink --transport native --write-tests \
  --gateway-id sc-em800-4.6.0 --output /tmp/mu-cli-validation.json

# All executable display targets, native and BLE, through this gateway.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_display_settings.py \
  --fixtures /tmp/firmware-fixtures.json --binary target/debug/cranklink \
  --gateway-id sc-em800-4.6.0 --output /tmp/display-cli-validation.json

# Drop a genuine ACK after the stock setter and modeled commit have run.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_display_settings.py \
  --fixtures /tmp/firmware-fixtures.json --binary target/debug/cranklink \
  --gateway-id sc-em800-4.6.0 --target-id sc-em800-4.6.0 --fault-tests \
  --output /tmp/display-uncertain-write-validation.json

# Current CLI motor writes, real regulation authentication, restore and reboot.
/tmp/cranklink-firmware-sim/bin/python tools/firmware-sim/verify_settings_writes.py \
  --fixtures /tmp/firmware-fixtures.json --binary target/debug/cranklink \
  --output /tmp/current-cli-persistent-writes.json
```

MU `--gateway-id` is repeatable; omit it for all admitted gateways. `--case` permits focused independent read/guard checks; sequence-dependent write/restore plans must run together. Display `--target-id` filters executable target profiles. Display configurations use separate processes: per-instruction Python hooks in threaded emulators can contend for one interpreter and exceed unchanged client response deadlines. Reports keep simulator scheduling failures separate from firmware rejection or client bugs.

## Execution boundaries and remaining work

Each machine retains RAM and CPU state between bounded function calls. Firmware bytes remain unchanged. The motor receive entry and transmit-acceptance routines are modeled boundaries: a virtual queue acceptance allows firmware to consume its pending-response flag, while subsequent loss may still prevent delivery. The modeled host/radio consumer drains the real gateway TX ring. Event ordering is deterministic and separate from instruction budgets.

SC-E7000 additionally needs UART initialization and completion. The simulator runs its stock UART initializer and service routine, saves/restores the interrupted CPU context, and resumes at the same busy-poll instruction. The model supplies ready-status memory semantics; it does not reproduce interrupt timing or the radio. Combined instruction limits include this service work.

Full reset-to-main boot, complete MMIO/interrupt timing, general NVM/settings support, authorization for other images/families, actual motor M execution, BLE radio/SoftDevice and physical E-TUBE arbitration remain later stages. Runtime persistence has no additional cryptography dependency: stock motor AES executes in Unicorn and the browser client uses its existing Rust code. `requirements-test.txt` additionally pins PyCryptodome `4.0.0` only for independent host proof calculations in regression tests. Plain mapped MMIO and synthetic RAM cannot establish hardware timing or firmware boot acceptance. Keep these boundaries visible when adding profiles or interpreting a passing report.
