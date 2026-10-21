# D / DCAS BLE transfer speed investigation

**Implementation update, 2026-10-21:** BLE DCASX upload prefetch is now in the
production sender. Old transfer/completion/checksum receipts are purged before
each transfer, including in sequential mode. The sections below retain the
original investigation; the implementation and current simulation results are
described at the end. The latest generated validation JSON supersedes the
historical test-only matrix described below.

Investigated 2026-10-17 against PR #22, worktree
`optimize_ble_firmware_speed`, revision `f145334`. No hardware operations or
production transfer changes were made.

## Hardware evidence

The existing local trace
`evidence/ble-firmware-20261016/write-20261016T092402Z/wire.jsonl`
(relative to the surrounding `esteps-recover` workspace) contains a completed
D transfer. Its adjacent journal records 156,300 image bytes and acknowledged
finish/reset. The later pair operation failed at the application version query;
this trace establishes D completion, not a successful complete D+M update.

The stream contains 2,443 blocks, each with 64 bytes including final padding,
four PCE fragments, buffer acceptance, transfer-start acceptance, gateway
completion, and an indexed motor checksum receipt. The analyzer validates
fragment order, block indices and each padded block checksum. Streaming took
331.022 seconds: **472.17 actual image B/s**, or 472.33 padded B/s. The journal's
entire D destructive stage took 334.705 seconds including setup/finish/reset.

| Host-observed phase, per block | Mean | Median | Total |
| --- | ---: | ---: | ---: |
| First to last fragment dispatch | 1.018 ms | 1 ms | 2.486 s |
| Last fragment to buffer acceptance | 63.720 ms | 57 ms | 155.669 s |
| Buffer acceptance to transfer dispatch | 0.470 ms | 0 ms | 1.147 s |
| Transfer dispatch to all remaining receipts | 70.003 ms | 63 ms | 171.017 s |

These are client log timestamps. RX timestamps record consumption by the
client, not radio arrival; TX timestamps precede the write call. The trace
does not record ATT write mode or negotiated connection parameters. Therefore
the two large phases cannot be attributed precisely to radio scheduling,
gateway scheduling, motor programming, or host/backend delays from this trace.
The four fragments are nevertheless dispatched almost immediately in this run.

Reproduce without connecting to a bike:

```sh
python3 tools/firmware-sim/analyze_d_transfer.py /path/to/wire.jsonl
```

Aggregate output is saved in
[`d-transfer-analysis.json`](../tools/firmware-sim/d-transfer-analysis.json).
The analyzer targets one complete indexed DCAS stream beginning at block 1;
it is not a general decoder for legacy unindexed DCAS2 or retried sessions.

## Why M queueing does not speed up D

`src/firmware_io.rs::flash_dcas_with_user_info` uploads one 70-byte logical
buffer packet for each 64-byte block. `src/ble/firmware.rs::send` splits it into
four PCE writes (20, 20, 20, 18 bytes), using fragment headers
`80/A0/C0/60`. The client waits for `82`, sends the short transfer-start
packet, then requires `83`, `C0`, and the indexed `88/31` motor receipt before
the next block. Receipt waiting already accepts out-of-order arrival.

PR #22's `queue_width` applies only to `send_batch(0x48, ...)` on FREE_COMMAND
for M streaming. D calls `exchange`/`send` on PCE_COMMAND instead. It never
reaches that batch path. Fragmented D writes already choose write without
response when advertised; short transfer commands prefer acknowledged writes.
Neither the 2 ms fallback batch pacing nor a fixed per-block sleep applies
to the D path. Removing reset/entry sleeps would only save fixed seconds.

## Priorities

1. **Measure link and gateway latency first.** Record negotiated connection
   interval and peripheral latency, ATT write mode, and host write start/end
   plus notification callback arrival. Compare the two per-block phases across
   supported hosts/adapters. If latency can be reduced without changing the
   motor protocol, both dominant phases benefit. This trace alone cannot
   establish a supported way to change the connection interval or predict its
   effect. The read-only model-GET benchmark in PR #22 tests FREE traffic;
   it does not validate PCE fragment or bootloader throughput.

2. **Investigate sending transfer-start without first waiting for `82`.**
   This is the largest protocol-level candidate: upload the four fragments,
   then dispatch the transfer command in order, and still require all four
   receipts before advancing. If gateway execution is ordered and buffer
   acceptance does not gate readiness, this could overlap part of the first
   latency phase. Eliminating its entire observed 155.669 seconds would leave
   about 175.353 seconds (~891 B/s, 1.89x). This is arithmetic headroom, not a
   prediction. Prove PCE reassembly/dispatch ordering, buffer lifetime and
   rejection behavior first. Failure must fence the session, with no replay
   or next block. An early transfer may already have side effects before a
   buffer error is consumed, so preserving receipt checks alone does not prove
   safety.

3. **Batch the four fragments only for a demonstrated transport bottleneck.**
   Batching could reduce browser worker RPC cost or sequential acknowledged
   writes on a response-only adapter. On this measured run, removing all
   first-to-last-fragment time saves at most 2.486 seconds, under 1%. Preserve
   byte order and fragment framing; concurrent futures or promises are not
   evidence of ordered peripheral dispatch on every backend. Existing browser
   write-without-response batching still awaits each write, although it saves
   worker RPC crossings. Keep this separate from skipping the buffer receipt.

4. **Defer multiple outstanding D blocks and larger blocks.** Packet numbers
   do not prove independent gateway buffer slots or safe concurrent programming.
   The client assumes one block at a time and packet numbers wrap every 128
   blocks (data/transfer each consume a number). Legacy DCAS2 additionally has
   unindexed motor acknowledgements and a page boundary. Increasing 64-byte
   blocks changes the boot protocol; increasing ATT MTU does not automatically
   change the stock 18-byte PCE fragment codec. These require separate firmware
   evidence rather than copying M's streaming queue.

## Original simulator limitation

`fw_sim/gatt.py::pce` rejects fragmented PCE input and permits only selected
normal-mode commands; update mode, logical `0B` and `88` are not admitted.
`bench_ble_speed.py` mocks machine setup and exercises the M behavior peer
directly rather than the Rust D sender. It supplies no D bootloader execution
or D programming timing. The gateway model also drains the stock TX ring after
each bounded call, so its normal-mode success does not prove burst capacity.

A useful next offline experiment needs stock PCE fragment reassembly and relay
dispatch plus an explicitly labeled D bootloader peer. Exercise early
transfer-start, buffer rejection, receipt loss/reordering/duplication, packet
wrap, partial final blocks and unchanged finish/reset checks. Use the actual
Rust sender. Such tests can establish modeled ordering and client behavior;
hardware timing remains a separate measurement.

The PR description and simulator speed section still describe queue width 1
as the default, while revision `f145334` sets M's default to 4. Neither setting
affects D.

## Offline follow-up: stock radio and gateway execution

The new, separate `fw_sim/d_relay.py` experiment executes the unmodified,
hash-pinned Nordic `UPDATENRF-STEPS3-ap.4.9.0` PCE fragment receiver and UART
enqueue code, connected to the stock SC-EM800 4.4.0 and 4.6.0 PCE dispatcher.
The normal GATT endpoint's admission policy is unchanged. The original radio
image came from the existing local vendor APK evidence; this does not establish
which radio image is installed on the physical bike.

The stock radio constructs the complete 70-byte upload before enqueueing it.
An immediately following transfer-start is a second FIFO record. The stock
gateway copies the upload into a buffer synchronously, then constructs `82`;
processing the next FIFO record successfully queues sixteen four-byte data
packets and the checksum command, returning `83`. Consuming `82` at the host
is not a gateway prerequisite. Supplying the accepted TX IDs to the stock
completion callback and flush function constructs `C0`.

Relevant inspected entry points:

| Image | Init | Completion flush | TX completion callback |
| --- | --- | --- | --- |
| SC-EM800 4.4.0 | `0x283D0` | `0x283E8` | `0x28560` |
| SC-EM800 4.6.0 | `0x279E0` | `0x279F8` | `0x27B70` |

On Nordic 4.9.0, `0x1D2AC` is the PCE fragment receiver and `0x1D64E` is the
UART enqueue function. The experiment runs both. UART state is initialized
busy, so the stock scheduler retains queued records instead of starting a
physical DMA transaction. The explicit consumer preserves and checks the stock
UART record XOR. The actual bus queue's producer/consumer state is preserved
between calls, then consumed explicitly on modeled physical TX completion;
stall experiments leave it unconsumed. Gateway host TX records are consumed
explicitly, as in the existing simulator. No instruction bytes are patched.

Motor boot identity, erase/programming and checksum/finish/reset replies remain
a **behavior peer**, including direct delivery into the client's notification
queue. The model verifies the exact synthetic image, final padding, each block
checksum, whole-image checksum, finish, reset, and single erase. Motor bootloader
execution, gateway motor-reply decoding, reset-to-main startup, SoftDevice,
the host Bluetooth backend and physical timing are outside this experiment.

## Results and decisions

The Rust experiment calls the real `firmware_io::flash_dcas` block loop and
`MotorFirmware` BLE sender. Only the alternate dispatch schedule and ordered
batch transport live in a test-only wrapper. It exercises sequential, early,
batch-only, early+batch and next-upload prefetch schedules against both gateway
images. Nominal early and prefetch transfers use 156,300-byte synthetic images
(2,443 blocks), covering packet
wrap, block index 2048 and partial final padding. Other wrap runs use 131 blocks.
The resulting matrix has 140 cases, including assertions that expose the
existing sequential sender's stale-completion weakness.

The Python suite adds eleven stock-code tests, including negative cases that
should demonstrate rejection or unsafe behavior. Passing a negative test
does not make that optimization safe. The generated
[`d-speed-validation.json`](../tools/firmware-sim/d-speed-validation.json)
records the matrix, image hashes and proof boundaries.

| Candidate | Offline evidence | Decision |
| --- | --- | --- |
| Early transfer-start, one block outstanding | Same stock bus data and receipts as sequential; full synthetic image, reordered/duplicated receipts, missing82/83/C0/motor receipt, injected NAK, corrupted fragment, missing terminal fragment and disconnect cases | Viable experiment under ordered delivery and a clean session. Keep every receipt, stop/fence on uncertainty, and clear stale gateway receipts **before** sending. Hardware speed remains unmeasured. |
| Prefetch the next upload, one programming transfer outstanding | Uploading block 2 while block 1 completes preserves block 1's checksum index and bus data. The Rust loop waits for block 2's 82 and all block 1 receipts before transfer 2. Full-image and fault/wrap runs exercise this schedule | A promising alternative to early-start: overlaps the next buffer upload while retaining acceptance-before-programming. At most one extra, untransferred upload is allowed; any uncertainty fences both. Requires the same clean-session, queue and receipt safeguards. Physical overlap/speed remains unmeasured. |
| Ordered four-fragment batching | Same codec and gateway result; batch-only and early+batch run through the Rust loop and faults | Saves three host write calls per block (7,329 for the measured image). This tests an ordered pipe batch, not concurrent native/browser Bluetooth operations. Measured fragment-time headroom remains under 1%. |
| Multiple D blocks outstanding | Two starts enqueue successfully and can produce two C0 receipts, but share a global checksum index. Three accepted uploads overwrite the oldest of two buffers. Stalled bus output eventually partially enqueues a block and returns transfer error | Do not copy M's queue width into D. Two buffers are not a proved concurrent programming protocol; no general credit or overwrite protection was found. |
| Bigger blocks / larger MTU | Stock dispatcher rejects a 68-byte data block even when the ATT size restriction is bypassed | A larger MTU does not expand the 64-byte boot block or stock PCE codec. No supported speedup established. |
| Skip checksum/receipt waits | Stock 82 accepts deliberately wrong data checksums; the motor peer rejects them. Missing receipts and NAKs fence the candidate schedule | Retain all block receipts and final validation. |
| Short transfer command without ATT response | Logical command behavior is independent of host ATT completion in this function-level experiment | Does not prove host/backend ordering, supported write mode, or a speed gain. Existing fragmented writes already use without-response when advertised; the short command still needs 83/C0/motor receipts. Leave policy unchanged. |
| Lower connection interval / peripheral latency | No RF, SoftDevice or negotiated connection parameters execute here | Remains a measurement candidate. Changing the M simulator's assumed interval is not evidence that a D flash or a particular host can obtain it. |
| Remove sleeps, logging or progress overhead | D has no per-block sleep; stream accounts for 331.022 of 334.705 seconds in the original journal | No meaningful stream gain established. Reset/entry settling cannot be validated by this function-level model. |

### Safety limits found by the negative tests

**Early-start cannot be considered safe for an arbitrary dirty session.** An
accepted but untransferred buffer retains its packet number. A new malformed
upload using that number is rejected with 82/error, but speculative transfer-start
still selects and transmits the older buffer. This is an actual stock gateway
counterexample, not a behavior-peer assumption. Normal experiments explicitly
initialize the gateway relay and allow only one outstanding block; an uncertain
session must never be reused or automatically retried. A dropped 82 can also
leave one block programmed before the client discovers the missing receipt.

**The existing sequential path has a stale C0 receipt weakness.** D packet
numbers repeat every 128 blocks. Injecting one duplicated C0 for block 1, then
dropping the real C0 for block 129, lets the unchanged sequential path consume the
old receipt and finish the synthetic image. The test-only early schedule clears
matching 83/C0 receipts before speculative dispatch and correctly aborts this
case. Clearing after dispatch would risk discarding fresh replies. This purge
does not distinguish an arbitrarily late old reply arriving after the purge;
the packet protocol itself lacks a block index on C0. Persistent duplicate
traffic is also bounded by the client's unsolicited queue, not an infinite
recovery mechanism. The matrix deliberately records the sequential weakness as
an observed limitation rather than claiming every baseline fault was rejected.

**Radio backpressure is silent.** Its eight-record ring has seven usable slots;
an eighth queued command is dropped. Early-start needs two records instead of
one before the host waits. That fits an empty queue, but ATT success does not
establish available UART capacity. The model uses FIFO writes and explicit
consumption; it cannot guarantee ordering under concurrent OS Bluetooth calls.

Early-start's ~1.89x figure above remains an upper-bound calculation from the
old trace. Offline success establishes command/receipt behavior under the
declared model, not that this speedup will occur or that live flashing is ready.
At the end of that initial investigation, production D dispatch remained sequential.

The prefetch experiment is deliberately different from two concurrent motor
programming transfers. It may leave a second upload in gateway RAM on failure,
but never starts programming it until the current block has completed and its
own buffer acceptance has been consumed. The matrix checks that failures cannot
advance to another transfer, finish, reset or erase replay. This makes prefetch
the preferred candidate for a later hardware experiment, while preserving the
same unresolved transport/timing boundaries as early-start.

## Reproduction

Use the Python environment containing `tools/firmware-sim/requirements.txt`
and local decoded stock images. Firmware images stay outside Git. Create a
local config such as:

```json
{
  "radio": "/absolute/path/UPDATENRF-STEPS3-ap.4.9.0.hex.bin",
  "gateways": [
    "/absolute/path/SCEM800.4.4.0.decoded.bin",
    "/absolute/path/SCEM800.4.6.0.decoded.bin"
  ]
}
```

Then run the complete offline experiment:

```sh
/path/to/python tools/firmware-sim/verify_d_speed.py \
  --config /absolute/path/d-relay.local.json \
  --output tools/firmware-sim/d-speed-validation.json
```

The runner invokes both the Python stock-code tests and the ignored Rust matrix.
Ordinary Rust tests do not silently substitute mocks for missing stock images;
this experiment is explicitly ignored until requested with its fixtures.

## Production upload prefetch and receipt fix (2026-10-18)

`src/firmware_io.rs` now owns the prefetch schedule. BLE's `MotorFirmware`
advertises support for it; the shared sender enables it only for DCASX
format 4. Wired transports and DCAS2/2i remain sequential, including the legacy
page switch. The CLI now exposes `--d-upload sequential|prefetch`, ordered
`--d-fragment-batch`, `--m-queue-width 1|2|4|8`, and a guarded `--d-only` path.
See the [manual CLI testing guide](ble-firmware-speed-testing.md).

The first block is uploaded and its 82 acceptance consumed. The client then
starts that block's transfer, uploads at most one successor, and waits for the
current 83, C0 and indexed motor checksum. Only after those receipts are verified
does it consume the successor's 82 and start its transfer. Progress still counts
verified blocks. A prefetched upload is not sent twice, and its fresh 82 is never
purged by a later exchange. A missing/rejected prefetched upload or an uncertain
dispatch fences the BLE session without starting another programming transfer,
replaying erase, or sending finish/reset.

Before a new transfer, all obsolete 83/C0 and motor checksum receipts are removed.
Before a new upload, old 82 receipts are removed. The BLE hooks first drain
already-arrived notifications with a zero timeout. Crucially, the transfer purge
retains the successor upload's fresh 82. This fixes cached C0 reuse after packet
wrap and prevents persistent duplicate receipts from filling the queue across
otherwise successful blocks. The sequential path receives the same fix.
Arbitrarily late old C0 arriving after cleanup remains indistinguishable on the
wire because C0 contains no block index; this change does not add a protocol epoch.

The ignored stock-firmware Rust matrix configures the production session's
prefetch and fragment-batch options. It implements no alternate schedule in test
code: all four combinations execute the production block loop and BLE sender.
Tests include lost/rejected prefetched 82, a disconnect during successor
upload, successor data corruption, sustained duplicates across wrap, and the
old duplicated-C0/missing-C0 case. Both modes must now reject that last case.

### Virtual latency model

The new peer uses deterministic events rather than emulator runtime or sleeps.
The mean first-to-last fragment cost (1.018 ms), upload acceptance latency
(63.720 ms), transfer dispatch overhead (0.470 ms), transfer receipt latency
(70.003 ms), and inter-block overhead (0.288 ms) come from the original trace.
The sequential model reproduces its 331.022-second stream for 156,300 bytes;
this calibration is checked in the Rust matrix.

The model separately varies acknowledged-write return duration through 0, 30
and 60 ms. That duration was not logged in the hardware trace. In sequential
mode it is hidden within the 70 ms transfer receipt phase. Prefetch cannot
start the successor upload until the transfer write returns, so a long return
duration reduces overlap. The model assumes that the old upload and transfer
phase latencies remain independent when overlapped and that UART/bus consumers
make progress. RF/SoftDevice scheduling, bus contention, browser RPC latency
and real motor flash timing can violate those assumptions. All figures below
are **modeled stream times**, not measured bike update times.

For a full 156,300-byte image on SC-EM800 4.4.0:

| Assumed acknowledged-write return time | Sequential stream | Prefetch stream | Speedup | Time saved |
| --- | ---: | ---: | ---: | ---: |
| 0 ms | 331.022 s | 172.932 s | 1.91x | 47.8% |
| 30 ms | 331.022 s | 233.335 s | 1.42x | 29.5% |
| 60 ms | 331.022 s | 306.595 s | 1.08x | 7.4% |

SC-EM800 4.6.0 also passes the full-image sequential/prefetch comparison at
0 ms and produces the same times because both use the same declared latency
parameters. No version-specific physical timing difference was modeled. The
earlier ~1.89x headroom estimate has been replaced by these executed-sender
simulations, which retain the initial upload and distinguish host write-return
latency. They still cannot choose the correct write-return duration from the old
trace; hardware write start/end instrumentation is needed for that.

The updated report contains **152 production-sender scenarios**, including sixteen
complete 156,300-byte image runs. Both schedules now stop at block 129 in the
duplicated-old-C0/missing-new-C0 regression, with no finish/reset. Prefetch also
rejects missing/rejected successor acceptance, disconnects and corrupt successor
data without another programming transfer. Ordered fragment batching is checked
with each schedule and the same faults; it uses sequential physical writes,
not concurrent fragment dispatch. Broader checks include ordinary Rust and web
tests, the Python suite, and the WASM library check. Optional firmware-dependent
tests require their local fixtures. The ignored Rust matrix is run explicitly
by the reproduction command.
