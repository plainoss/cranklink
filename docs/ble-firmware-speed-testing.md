# BLE firmware speed testing from the CLI

PR #22 prepares the following stock-protocol optimizations for manual testing.
No physical flash or hardware speed benchmark was performed for this update.

| Option on `firmware flash ble-pair` | Behavior |
| --- | --- |
| `--d-upload sequential` | D baseline: accept upload, start programming, verify all receipts, then upload the next block. |
| `--d-upload prefetch` (default) | Upload the next D block while the current block completes. Each transfer still waits for its own buffer acceptance and all preceding programming receipts. Only one programming transfer is outstanding. BLE DCASX only; wired and older formats stay sequential. |
| `--d-fragment-batch` (default: off) | Submit each D upload's four ordered PCE fragments through one transport call. Native BLE awaits each fragment in order inside one runtime entry; this is not concurrent fragment transmission. Browser implementations retain ordered individual writes. |
| `--m-queue-width 1\|2\|4\|8` (default: 4) | Queue M streaming writes in groups of this size. Width 1 restores sequential writes. Does not change D transfers or request a new connection interval. |
| `--d-only` | Check the installed M version against the supplied companion, then flash only D. Skips M bootloader entry and all M erase/program commands. Companion compatibility, settings backup, D checksums, finish/reset, application identity/version and settings readback remain required. |

All runs retain the serial/model/board/version gates. Settings, image and journal
paths must refer to the intended unit. An uncertain operation stops without
automatic erase replay or reconnect/resume. Recovery over BLE is not established;
the [documented EP800 recovery](ep800-hardware-recovery.md) is a separate wired
procedure. These options do not modify firmware binary contents.

## Build and prepare

Build from this PR's branch. Keep other Bluetooth clients disconnected.

```sh
cargo build --locked --features ble --bin cranklink
./target/debug/cranklink ble scan
./target/debug/cranklink --port 'ble:DEVICE_ID' probe
./target/debug/cranklink firmware flash ble-pair --help
```

Use the actual 12-hex-digit motor serial from `probe`, the gateway's BLE ID,
your six-digit passkey file, and compatible stock firmware files. The target D
version must be at least the installed version. D-only requires the installed
M version to equal the companion version; an incompatible pair fails before
bootloader entry or erase. Reflashing the same version is permitted.

The first command below is a **dry run**. It saves settings, probes D's bootloader
and resets back to the application; it sends no firmware blocks. It does not
benchmark speed or test prefetch. With `--d-only`, it does not enter M's bootloader.

```sh
./target/debug/cranklink --port 'ble:DEVICE_ID' -s YOUR12HEXUID \
  --passkey-file /path/to/passkey.txt --trace dry-run-wire.jsonl \
  firmware flash ble-pair \
  --dcas /path/to/DUEP800-D.4.2.0.dat --rx /path/to/DUEP800-M.4.1.11.dat \
  --d-only --d-upload sequential --m-queue-width 1 \
  --backup dry-run-settings.json --journal dry-run.journal.jsonl
```

## D transfer comparisons

Adding `--write` below performs a real D erase and flash. Each comparison is
another flash, not a harmless benchmark. Use stable power and unique output
paths for every run; preserve the journal if anything fails.

```sh
./target/debug/cranklink --port 'ble:DEVICE_ID' -s YOUR12HEXUID \
  --passkey-file /path/to/passkey.txt --trace d-prefetch-wire.jsonl \
  firmware flash ble-pair \
  --dcas /path/to/DUEP800-D.4.2.0.dat --rx /path/to/DUEP800-M.4.1.11.dat \
  --d-only --d-upload prefetch --m-queue-width 1 \
  --backup d-prefetch-settings.json --journal d-prefetch.journal.jsonl --write
```

For a sequential baseline change `--d-upload prefetch` to `--d-upload sequential`
and use fresh output paths. To isolate fragment batching, repeat the chosen
schedule with `--d-fragment-batch`. M queue width has no effect on D-only runs.

The first physical run can be compared with the existing historical baseline,
avoiding an additional baseline flash. A matched comparison on the same host,
gateway and image gives stronger evidence. Do not automatically repeat a failed
operation.

For a 156,300-byte D image, the trace-calibrated model predicts 331.0 seconds
sequential versus 172.9 / 233.3 / 306.6 seconds with prefetch at assumed 0 / 30 /
60 ms acknowledged write-return delay. These are assumptions, not a physical
speed guarantee. Ordered batching saves runtime crossings; the historical
fragment-dispatch phase accounts for less than 1% of the baseline.

## M queue comparisons

Omit `--d-only` to perform the full D+M transaction. Select `--m-queue-width 1`,
then 2 or 4 for comparisons, with fresh paths. Width 8 is available for explicit
testing; no hardware queue width is certified by the simulator. Per-window and
whole-image validation remain enabled. The D scheduling flags can be held
constant to isolate M behavior, but every full pair run also flashes D.

Before M flashing, the non-erasing native benchmark can compare acknowledged
ordinary GET queueing. It does not validate motor programming or recovery:

```sh
cargo run --locked --features ble --example ble_ack_queue_bench -- \
  --device 'ble:DEVICE_ID' --passkey-file /path/to/passkey.txt \
  --expect-serial YOUR12HEXUID --output-dir ack-bench
```

## Inspect results

The journal records the selected options and destructive stage boundaries.
Confirm `dcas-finish-and-reset`, final application identity/version, unchanged
settings and `cleanup-confirmed`. A full pair run must also confirm
`rx-finish-and-reset`; D-only must have no `rx-destructive-start` record.

The wire trace adds `tx-complete` after individual D writes and start/complete
markers around ordered D batches and M batches. Compare these timestamps with
their dispatch records to measure host write-return latency. They describe host
calls, not radio airtime; receive timestamps describe client consumption.
In ordered-batch mode the four `tx` records are logged before the batch call;
use the batch start/complete markers for its duration, not the spacing between
those four records. Whole-stream time remains comparable across both modes.

```sh
python3 tools/firmware-sim/analyze_d_transfer.py d-prefetch-wire.jsonl
```

The analyzer associates overlapping D uploads with indexed motor receipts,
including packet-number wrap. Stream timings exclude connection, backup, erase,
final reset and postflight checks; compare journal stages for complete operation
time. See [D investigation and offline evidence](ble-d-transfer-speed.md) and
[simulator boundaries](firmware-simulator.md).

## Candidates deliberately kept offline

Early transfer-start before buffer acceptance has a demonstrated dirty-buffer
counterexample. It is not exposed as a hardware CLI mode; prefetch provides
overlap while retaining acceptance-before-programming. Multiple concurrent D
programming transfers share checksum state; a third upload can overwrite a
buffer. Bigger D blocks are rejected by stock code. Skipping receipts/checksums
and speculative erase retries are not enabled. Connection-interval changes are
only simulator assumptions, not a supported host tuning control.
