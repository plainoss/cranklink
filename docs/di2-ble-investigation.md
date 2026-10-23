# Di2 BLE service-session lock investigation

## Observed on 2026-10-22

The bike used an EW-WU111 wireless unit, EW-RS910 bar-end junction and
BT-DN110 battery master. Inventory also identified FD-R9150, RD-RX815 and
ST-R9170 components. No settings SET or firmware operation was sent.

Gateway authentication and inventory succeeded. Later component queries timed
out. Shifting and the junction's connection-mode button then stopped working.
Reconnecting the WU111 restored LED activity and advertising. A gateway stop
ended the flashing but did not restore shifting. Disconnecting and reconnecting
the battery restored normal operation.

## Evidence and confidence

The strongest explanation for the loss of shifting is a PC-link service session
remaining active on the battery master. This is an inference, not a direct
reading of the battery's internal state. The recovery after a battery reset
supports a volatile session-state problem; it does not independently identify
the failing protocol step.

Representative credential-free captured frames:

| Direction/channel | Bytes | Observation |
| --- | --- | --- |
| TX PCE | `00 0c 01` | Gateway PC-link start during inventory. |
| RX FREE | `00 32 12 01 13 ff ff ...` | Logical master acknowledges PC-link start with PC address `13` hex. |
| TX FREE | `00 01 1c 00` | Logical-master stock GET. |
| RX FREE | `00 01 1e 11 00 ff ff ...` | Master stock identity matches BT-DN110. |
| TX PCE | `00 0c 00` | Gateway stop. |
| RX PCE | `2c 00 ...` | Gateway acknowledgement; no corresponding master stop receipt was recorded. |

The old normal/slave cleanup skipped an explicit stop for logical slot zero.
Inventory never set `linked_slot`, so it also lacked a selected component to
stop. This let a gateway acknowledgement pass as cleanup success without
confirming release of the master that discovery had started.

The component timeouts remain unresolved. The PC address and reported component
identities changed across connections, so old slot numbers cannot be reused.
Observed failures included part GET `32/9c` and direct stock GET `01/1c`.
The captures do not prove whether routing, discovery timing, cache contents or
component service-state requirements explain those failures. Authentication
succeeded repeatedly, so an incorrect passkey does not explain the subsequent
protocol timeouts. Some separate reconnect attempts failed OS BLE encryption.

## Offline changes and validation limits

Normal/slave cleanup now explicitly stops a known logical master using
`32/10 00 <pc-address> 00 00`, verifies its `32/12` stop receipt and owner, and
only then sends gateway `0C/00`. It records a pending start before sending
`0C/01`, and observes full master-start receipts independently of discovery
identity validation. An observed start survives a malformed identity reply.
If neither a receipt nor a valid identity resolves the pending start, cleanup
reports an unknown state rather than claiming success. Foreign-owner and late
start receipts also cannot pass as confirmed cleanup. This includes inventory
sessions with no selected component. Missing, rejected or mismatched stop
receipts cannot report successful cleanup.
A subsequent offline reproduction found that unconditional timeout fencing
prevented this stop sequence from running after the observed identity-read
failures. Verified identity/schema GET timeouts now allow one cleanup-only attempt, provided
normal/slave discovery established a master owned by the local PC. Ordinary
commands remain poisoned. Write/auth/firmware failures, unknown/foreign owners,
transport errors and notification overflow do not receive this exception. A
later transport error, oversized notification or flood revokes it.

Rust regressions model the battery-master start remaining active after a gateway
acknowledgement. They check explicit stop ordering, partial discovery failures,
late start receipts, unknown or foreign ownership, and incomplete or mismatched
stop receipts. Browser pause releases the master while retaining the radio;
the next discovery reacquires the service link. Existing master-routing tests
remain part of the verification suite.

The firmware simulator previously issued its own stop commands when GATT closed.
It now preserves firmware RAM and PC-link ownership on radio disconnect. New
tests execute stock SC-EM800/EP800 firmware, retain the active link across a
reconnect, then clear it through an explicit protocol stop. This validates the
simulator's lifecycle correction, not BT-DN110 firmware behavior.

A separate fault-injection regression drops stop delivery at the modeled bus
boundary. The unmodified stock gateway still generates `2C/00`, while the stock
remote firmware remains in PC-link, including after GATT closes. This reproduces
the acknowledgement-without-release failure class; it does not establish that
bus packet loss caused the observed hardware failure.

Partial, hash-pinned Di2 service execution is now available as described below.
The new master-stop sequence still needs validation on the physical bike before
claiming that it restores shifting. No hardware was accessed during this offline
investigation.

## Firmware acquisition follow-up

Official images for both BT-DN110 board variants, the WU111 main firmware family
(`EWWU101`) and the separate Nordic application/bootloader/SoftDevice have now
been acquired and hash-pinned. The main images are legacy DCAS2i with corroborated
MN101-family vector mapping. A separate bounded MN101 interpreter now executes
selected service handlers; the ARM backend remains unchanged.
The radio application is a separate Thumb image, not a replacement for the main
MCUs' service-state logic. No new executable topology coverage is claimed.

The [acquisition guide](firmware-simulator.md#di2-research-inputs) records the
source/model-mapping evidence, exact hashes, repeatable checks and the remaining
CPU/peripheral work.

## Firmware-backed timeout reproduction

`di2_service.py` executes the original PC-link handler in both battery images
(board 1 entry `0x6749`, board 2 `0x678c`) and WU-family main firmware
(`0x5854`). The fixture models the receive ABI and zero-initialized RAM. The
battery's JSRV4 send service is an explicit modeled queue boundary; packet
construction and service mode changes execute original instructions.

Both battery start handlers return after 128 original instructions and retain
mode 1 / the supplied owner. Both stop paths clear the mode and produce the
actual directed `32/12 00 <requester> FF FF` packet. Execution pauses at that
receipt, BEFORE the remaining battery housekeeping; it is not a completed stop
handler or proof of restored shifting. The receipt echoes the requester's byte,
even if it differs from the stored owner, so the client's observed-owner
precondition is essential. The WU-family start handler returns. Its strict stop
traps at peripheral `0x3FF4`. With an opt-in exact eight-write interrupt-control
fixture, the original stop handler returns after 1,199 instructions; repeated
start/stop cycles retain RAM, emit receipts and balance the stack. No interrupt
timing or physical register effects are simulated. RF, Nordic firmware, bus timing,
complete boot, derailleurs and shifting remain outside this execution scope.

`verify_di2.py` runs the actual CLI against this battery service and a narrowly
modeled normal/slave gateway. It deliberately drops the first master stock GET
reply. PR base `6f5683e` leaves the actual battery service mode active after the
CLI closes. The fix issues the explicit stop, consumes the firmware-produced
receipt and then stops the modeled gateway; both battery variants pass. The
original read still reports an error and is not retried. This establishes an
offline reproduction and regression check for the cleanup omission. It does
not establish why the bike's component reads stopped responding or validate
the complete WU111/BT-DN110 topology.


## Follow-up before hardware validation

Named BLE selection previously queried every ambiguous component before finding
the requested model. It now follows the wired selection approach: inspect the
current stock cache, resolve only matching model candidates, and revalidate the
selected identity. A regression selects FD-R9150 and RD-RX815 while an unrelated
lever's part GET would time out. This is a separate, reproducible client bug;
it does not explain the original rear derailleur's own missing reply.

`units --stock-only` omits optional component part/model queries while retaining
stock identities and unresolved candidates. The offline CLI fixture now models
a responding stock cache plus a lost RD part GET. Ordinary inventory reproduces
the timeout; stock-only inventory succeeds and both paths execute explicit
battery cleanup. A schema GET timeout also permits one cleanup-only sequence,
using exact declared query selectors and excluding every opcode used by a SET.
A missing gateway master-metadata reply also qualifies only after an observed local-owner master start; start-acknowledgement timeouts stay excluded. Transport/ownership checks and all failure revocations remain in force.

The physical checks should proceed one connection at a time: link test, stock-only
inventory, then one named-component identity/settings read. After every service
session, require both protocol cleanup receipts and manual confirmation that
shifting and the junction button still work. Do not carry a slot address across
connections, retry a failed query automatically, or infer normal operation from
LEDs alone. No settings SET or firmware operation is part of this validation.

## Main-branch integration before hardware testing

Integration with `origin/main` at `8ec3a0a` required retaining the logical-master
cleanup guard: upstream best-effort gateway shutdown after a rejected unit stop
is allowed only when the master is already inactive. Pending ownership, foreign
ownership and malformed master stop receipts must not trigger gateway shutdown.
Existing regression tests cover these cases.

The native transport now accepts already queued final acknowledgements after
the peripheral disconnects and treats an already closed link as closed. An
additional overflow check after awaited delivery prevents a queue that became
incomplete during the wait from supplying a successful cleanup receipt. A
hardware-free regression covers that race. Trace write failures now disable
diagnostic tracing without discarding protocol replies or aborting cleanup.
