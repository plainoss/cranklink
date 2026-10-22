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
Timeouts still fence further protocol traffic; this change does not attempt
automatic recovery after a poisoned session.

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

There is no hash-pinned EW-WU111/BT-DN110 executable profile in the current
simulator. The new master-stop sequence needs validation on that exact hardware
before claiming that it restores shifting. No hardware was accessed during
this offline investigation.
