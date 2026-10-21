# E-TUBE protocol reference

Revision: 2026-10-21. Scope: component integration, service traffic, identity,
settings and update transport. Hexadecimal byte strings omit `0x`; quantities
and times are decimal unless explicitly marked. `||` means concatenation.
`LE16/LE32` mean unsigned little-endian integers. Preserve unknown bytes.
Indices and slices are zero-based. In the settings table, "original bytes"
refer to GET reply parameters after removing route, group and opcode headers.

This is an implementation reference, not a complete specification of every
released Shimano product. The strongest coverage is the normal service path
between gateways and drive units. Product recognition, command support, firmware
execution, electrical interoperability and riding behavior are separate claims.

Jump to [USB framing](#2-sm-pce02-host-stream), [identity](#3-addressing-discovery-and-identity),
[PC-link](#4-normal-motor-pc-link-lifecycle), [Bluetooth](#5-bluetooth-service-and-routing),
[authorization](#6-protected-drive-unit-settings-authentication),
[settings](#7-settings-wire-dictionary), [updates](#8-firmwareupdate-traffic-recognition),
[bus fragments](#9-external-packet-fragments-and-missing-physical-specification),
[coverage](#10-product-and-simulator-coverage) and [decoding](#11-traffic-decoding-and-implementation-checklist).

## 1. Layers and integration boundaries

| Boundary | Encoding | Established scope |
| --- | --- | --- |
| Host ↔ SM-PCE02 | Serial stream, delimiter/escape/additive FCS | Normal service and selected motor update paths |
| Host ↔ Bluetooth gateway | Shimano GATT service and characteristic records | Authentication, normal routing, selected update relay |
| Gateway ↔ external E-TUBE nodes | Addressed packets; firmware FIFO fragments | Bounded packet processing in supported firmware |
| Motor D ↔ motor M | Separate internal interprocessor protocol | Some EP800 application exchanges; not an external E-TUBE port |
| Traction battery ↔ motor/charger | Product-specific battery interface | Not described by the USB or GATT framing rules |
| Wireless shifter / ANT / D-FLY / CAN accessory | Separate radio or accessory interface | Not established by a successful E-TUBE service query |

A host application can integrate through SM-PCE02 or a compatible BLE gateway
using the formats below. A new physical E-TUBE component additionally needs the
electrical transceiver, modulation, arbitration, discovery participation, power
behavior and normal-running messages. These are not fully specified here. In
particular, **2 Mbaud USB serial is not the external E-TUBE wire baud rate**.

EW-SD50 and EW-SD300 designate wiring/connector systems; connector conversion
does not establish functional compatibility. Follow the exact component and
firmware combinations in Shimano's [electronic compatibility charts](https://productinfo.shimano.com/en/compatibility/C-473)
and [e-bike charts](https://productinfo.shimano.com/en/compatibility/C-525).
Passive wiring, satellite switches and battery mounts need not appear as
independent service nodes. A Di2 BT-DN battery is not a STEPS traction BMS.

## 2. SM-PCE02 host stream

Serial configuration: **2,000,000 baud, 8N1, RTS asserted**, 500 ms write deadline.
RTS assertion is not evidence that CTS flow control is required. SM-PCE1 and
SM-BCR2 must not inherit this transport merely because they are PC interfaces.

For control `C` and payload `P`:

```text
D   = C || P
FCS = -sum(D) mod 256
wire = BB || escape(D || FCS) || BB
escape: BB → BD 9B; BD → BD 9D; every other byte unchanged
if len(wire) < 128: append FF until len(wire) == 128
```

There is no outer length field. Escape the FCS too. Padding is outside the
delimiters/checksum; writes above 128 bytes are not rounded to 128-byte multiples.
Receive padding length is not implied by this transmit rule. `FF` inside a frame
is data. Retain partial frames/escapes across reads and parse multiple frames per
read. Validate `sum(C || P || FCS) mod 256 == 0` before dispatch. Bound buffers,
reject invalid escapes/checksums and resynchronize at delimiters.

Cranklink retains `control` and the full `payload`. Its convenience normalization
removes the first payload byte **only for control `48`**. Do not normalize `A8`
that way: its first payload byte is still an address. Correlate before discarding
any route byte.

### Adapter commands

These payloads are inside the framing above. Reply status layouts vary by command.

| Request control | Payload | Reply control | Meaning / required interpretation |
| --- | --- | --- | --- |
| `10` | empty | `30` | Adapter initialization; first reply byte `00` |
| `1A` | `01` | `3A` | Unlock power limit; reply byte 1 is `00` |
| `1A` | `02` | `3A` | Power query: `02 state` |
| `1A` | `03` | `3A` | Enable adapter supply; reply byte 1 is `00` |
| `1C` | empty | `3C` | Capacitor level, first two bytes LE16 |
| `03` | mode | `23` | Set protocol/role; normal accepted status `00` |
| `04` | empty | `24` | Resulting protocol and local slot |
| `05` | empty | `25` | Adapter/gateway revision |
| `06` | slot | `26` | Select target, `00` success |
| `1D` | `00` | `3D` | Master PnP; accepted reply starts `00 00` |
| `1B` | empty | `3B` | Connected-slot bitmap, at least four bytes |
| `07` | subcommand, optional LE32 mask | `27` | Fast-mode adapter control; separate from ordinary discovery |

USB normal/master requests mode `20`, expecting readback `A0 | pc_slot`.
UPDATE/master requests `60`, expecting `E0 | pc_slot`. The mode byte includes
role/state bits; its low five bits are the local address, not a model identifier.
This is the implemented subset, not an exhaustive adapter opcode allocation.

Power state `01` denotes external supply and `02` permits adapter supply.
States `03/04` require charging: the client polls every 3 s, waits for capacitor
level ≥2703, then requires state `01/02`, with a 90 s limit. The level is a raw
device quantity, not a stated voltage unit. Initialization waits 3 s after reset;
protocol changes wait 1 s and PnP waits 5 s. These are host pacing policies, not
measured minimum bus timings. Adapter power does not prove traction power or
assist readiness.

## 3. Addressing, discovery and identity

Normal host requests are `48 [target, group, opcode, parameters…]`.
Normal USB replies are `48 [pc_slot, group, reply_opcode, parameters…]`.
The reply address is the **PC recipient**, not necessarily the requested node.
The serialized request/session identifies the responding target.

Slots are five-bit addresses, 0–31 in the host interface. Bitmap byte `i/8`, bit
`i%8` denotes slot `i`. Slots may change after PnP or power cycles even when the
occupied bitmap is unchanged. Resolve identities from the current map. Slot 13
is a simulator fixture and observed example, never a universal motor address.
The virtual packet bus also uses target 63 for broadcast; that does not make 63
a valid USB target-selection argument.

For the ordinary service families below, success opcode is request +2 and error
opcode request +3. Validate address, group, opcode, length and echoed selector;
do not apply this arithmetic to bootloader or adapter commands.

| Group / request | Parameters | Success | Reply parameters / use |
| --- | --- | --- | --- |
| `01/1C` | `00` | `01/1E` | Stock identity; first bytes series, unit |
| `04/0C` | `00` | `04/0E` | Switch/lever part discriminator where supported |
| `16/7C` | `00` | `16/7E` | Seven-byte product/model attributes on supported motors |
| `01/2C` | `00` on the supported newer path | `01/2E` | D application version; not universally accepted on older images |
| `01/84` | `01` | `01/86` | M/RX application version/cache |
| `01/3C` | `00` | `01/3E` | Six binary serial bytes; optional seventh reserved `FF` |
| `01/A4`, `01/AC` | `00` | `01/A6`, `01/AE` | Product serial pieces on supported modern motors |
| `32/9C` | `00` | `32/9E` | FD/RD part discriminator where supported; not a general ownership query |
| `26/4C` | `00` | `26/4E` | Motor's battery type/product/fix diagnostic cache |
| `26/44` | `00` | `26/46` | Battery-related diagnostic; full semantics not established here |

Version bytes `v0 v1 v2` mean `(v0>>4).(v0&15).v1.v2`; e.g. `42 00 00`
is 4.2.0.0 and `46 00 01` is 4.6.0.1. Filenames can omit the last revision.
Serial bytes are not a BCD number or ASCII string; retain wire order. Modern
product serial text is assembled from first reply bytes `[1:6]` and second reply
bytes `[0:6]`, then checked as printable ASCII. It differs from the binary serial.

Series/unit pairs can have multiple models. Match part and exact product
attributes, retaining ambiguity if an optional query is unavailable. EP800 uses
series/unit `23/00`, attributes `30 30 00 00 00 00 00` (standard) or
`30 30 2D 43 52 47 00` (cargo). EP600 uses `30/00`, EP801 `35/00`, EP500
`37/00`, E5100 `38/00`. EP600/EP801 share a firmware distribution family;
the filename does not establish the live motor model.

Older E6000-family identities need special care: sampled E6000 is `08/00`,
E6001-family includes `0C/00` and a distinct `0A/00` variant, and E6002/E6012
uses `15/00`. The current client catalog includes historical shared-`08` aliases;
do not treat those aliases as an authoritative live-wire identity map. Older
motor product attributes can be binary; decoding all such replies as text loses
information. An unknown identity is not permission to use another model's writes.

### Bounded EP800 memory-read service

Normal `32/8C → 32/8E` reads 1–3 bytes on the working EP800-CRG application
profiles admitted by [the resident-dump reader](../src/bootloader_dump.rs).
Request parameters are `[addr_hi,addr_lo,addr_mid,count,00]`, where the three
address bytes form a 24-bit address in the unusual high/low/middle order.
Reply parameters echo `[addr_hi,addr_lo,addr_mid,data…]`; validate that echo
and the six-byte record (or seven with reserved final `FF`). This read does
not enter boot mode. The reader restricts model, board, D/M versions, fixed
64 KiB resident range and reviewed application sample windows. It is not a
universal memory-read capability of every E-TUBE node.

## 4. Normal motor PC-link lifecycle

For a directly addressed motor with PC source `p`, after discovery and target
selection, send these logical group/opcode/parameter records in order:

```text
32 10 01 p 00 00             start
32 30 A2 2B 00 00           key 1
32 30 30 0E 00 00           key 2
32 30 7A 4D 00 00           key 3
32 30 62 2B 00 00           key 4
32 30 85 B4 00 00           key 5
← 32 12 01 ...              require matching start receipt
... read identity / settings ...
32 10 00 p 00 00             stop
← 32 12 00 ...              require stop receipt
```

The USB client waits 300 ms after start on adapter supply, otherwise 1 s, and
100 ms between keys. Fixed PC-link keys establish a service session; they do
**not** grant protected-setting authorization. Missing/reordered keys must not
be interpreted as a successful link. Preserve asynchronous receipts while
waiting for command replies. A second start on an already started motor is not
an innocuous keepalive.

## 5. Bluetooth service and routing

Service UUID: `000018ff-5348-494d-414e-4f5f424c4500`.
Characteristic UUIDs substitute the listed short value in the first eight hex
digits: e.g. `00002afa-5348-494d-414e-4f5f424c4500`.

| Short ID | Purpose | Record format / direction |
| --- | --- | --- |
| `2AF3` | Authentication control | Write user/app proofs; subscribe for auth events |
| `2AF4` | Authentication nonce | Read challenge |
| `2AF9` | PCE response | Notify/indicate: `[reply_control, payload…]` |
| `2AFA` | PCE command | Short write: `[00, control, payload…]` |
| `2AFD` | FREE response | Notify/indicate: `[component_slot, group, opcode, params…]` |
| `2AFE` | FREE command | Write: `[target_slot, group, opcode, params…]` |
| `2AFF` | Feature control | Access check `FF 00`; other values can change operation mode |

Discover actual read/write/notify properties. Use write-with-response where
required; `2AF3` requires it in the implemented authentication path. GATT payloads
have **no USB BB delimiters, BD escapes, additive FCS or 128-byte padding**.
FREE response byte 0 identifies the component, unlike the PC-recipient byte in
USB `48`. The normal client limits gateway params to 16 and unit params to 15;
these implementation limits do not establish every gateway's maximum record.

### Gateway authentication

If both auth characteristics exist, read a nonempty nonce of 1–16 bytes
(one 100 ms retry for an empty read), subscribe to `2AF3`, then write
`01 || user_proof` and `02 || app_proof`. Six ASCII passkey digits are required,
including leading zeroes. Gateway proof calculation uses AES-128 ECB: pad a
short nonce with PKCS#7 byte `16-n`, encrypt the first block and retain the first
`n` ciphertext bytes. A 16-byte nonce uses the encrypted first block without
appending a retained second block. The user key is a fixed ten-byte prefix plus
the six passkey bytes; the application key is separate. The implementation and
fixed test vectors are in [the BLE module](../src/ble.rs).

After proof writes, send Feature `FF 00`, subscribe to PCE/FREE replies, select
normal/slave with PCE `00 03 00` and require PCE `04` readback with high bits
`80`. A successful write or OS pairing alone is not gateway authentication.
Absence of both auth characteristics is a separate supported service shape;
presence of only one is invalid. Authentication failure clears the credential.

### Normal/slave route

1. Select mode `00`; query `04`, obtaining `80 | pc_slot`.
2. PCE `0C 01` enters gateway PC-link mode; require `2C` receipt.
3. PCE `0D 00` returns `[battery_status, master_series, compatibility_flag,…]`.
   Byte 2 is **not** the master unit number. A literal battery-present status
   from a gateway handler does not prove a real BMS handshake.
4. If a master is announced, read its stock at logical slot 0 (`01/1C`).
   Routed `01/14 00` returns at least six params; bitmap occupies bytes 2–5.
   The master can be absent from that bitmap; include separately verified slot 0.
5. Gateway cache `02/14 [slot] → 02/16 [slot,series,unit,…]` identifies slaves.
   Select the target with PCE `06` before direct FREE commands. Cache reads via
   slot 0 can change selection; restore target before subsequent direct traffic.

Do not substitute the USB master's PCE `1B` bitmap for this slave discovery path.

### Normal/master route

Master routing changes ownership and the address map. Query gateway revision
(`05`) and local stock (`13 [01 1C 00] → 33 [01 1E series unit…]`), request
mode `20`, and require `04 → 24 A0`. Then PnP `1D 00`, wait 5 s, obtain
PCE `1B` bitmap, and start gateway PC-link `0C 01`.

In this mode the gateway sends motor start plus all five keys. Consume its
motor start receipt rather than replaying start. FREE receipt begins
`[motor_slot,32,12,01,00,FF,FF]`, confirming owner zero. Direct FREE commands
carry the new target slot; do not reject a motor solely because it reused the
gateway's former slave slot. Read remote stock directly with `01/1C`.

For repeated operations, retain gateway mode and ownership state until explicit
disconnect; do not call gateway stop after each save. Cleanup order is motor
stop → select slave mode `00` → confirm `04` high bits `80` → gateway `0C 00`
→ close GATT. Slave restoration may acknowledge `23 10` asynchronously; only
readback confirms the resulting mode. Gateway stop can end Bluetooth connection
mode. A timeout fences subsequent dispatch; closing GATT does not prove that
the motor acknowledged stop. See [Bluetooth usage](bluetooth.md).

## 6. Protected drive-unit settings authentication

This is separate from the gateway PIN and fixed PC-link keys. Bind the exchange
to fresh stock identity, exact binary serial, model parameters and D version.
Only one attempt is made per session; malformed/reordered fragments or a failed
receipt stop the operation. Do not log keys, challenges or proofs.

| Motor family | Protocol selection |
| --- | --- |
| EP800 / cargo; E5100 | Legacy |
| EP600 / EP801 / EP500 | Modern |
| E8000 / E8080 | Legacy from D4.7.0, not required before |
| E7000; standard E6100 / E6110 / E6180 | Legacy from D4.4.0, not required before |
| E6100 cargo / E6180 trekking | Legacy |
| Older E6000 families | Variant-dependent; unresolved modern identities block client protected writes |

Legacy: `16/D8` carries a seven-byte serial-derived authentication value;
`16/DA` returns a 16-byte challenge in three fragments. Encrypt the challenge as
one AES-128 ECB block with the derived update key; send proof via `16/E0`.
Require `16/E2` success. Modern: send encrypted authentication block via `33/68`,
receive encrypted challenge via `33/6A`, decrypt with the encryption key and
encrypt the resulting nonce with the update key, then send via `33/70` and
require `33/72` success.

Both block exchanges use selectors `16`, `26`, `34` for respectively 6, 6, 4
bytes. The last fragment retains two trailing `FF` bytes in its six-byte data
area. Logical challenge record is `[group,reply,selector,data6]`; unit/USB route
adds another byte. Final success record is `[route,group,reply,FF,FF,FF,FF]`.
BLE accepts that seven-byte record or a ten-byte notification with exactly the
known extra three buffer bytes; do not mistake filler for a boolean status.

Exact derivation and synthetic vectors are centralized in
[unit_auth.rs](../src/ble/unit_auth.rs). Legacy uses the six binary serial bytes
as three LE16 words and a 32-bit xorshift generator. Modern also uses the last
six **ASCII** product-serial characters, without hex decoding or case conversion.
Keys cannot be transferred across these schemes. Authorization permits the
protected service; each command's range, ownership and model checks still apply.

## 7. Settings wire dictionary

This table specifies the implemented EP800 layouts. All queries use parameter
`00` unless another selector is given. Success is SET/GET +2; replies may retain
selectors and padding. Check the model schema before reusing a layout.

| Field | Group | SET / GET | Parameters / interpretation |
| --- | --- | --- | --- |
| Lamp | `10` | `10 / 14` | Byte: 0 off, 1 switch, 2 always on |
| Light output | `10` | `18 / 1C` | LE16 ×0.01 W; SET adds `FF FF` |
| Lighting reserve | `16` | `A0 / A4` | LE16 minutes; SET adds `FF FF` |
| Walk assist | `16` | `68 / 6C` | Byte: 0 disabled, 2 enabled |
| Wheel circumference | `35` | `00 / 04` | LE16 mm; SET adds `00 00` |
| Mounting angle | `16` | `58 / 5C` | LE16 degrees; EP800 SET adds `00 00` |
| Destination | `16` | `A8 / AC` | GET `01`; reply selector, value; SET selector,value,`FF FF` |
| Active profile | `16` | `80 / 84` | Byte 5 or 6; SET adds `00 00 00` |
| Assist speed | `16` | `B0 / B4` | GET `01`; SET LE16 km/h×100,`01 FF` |
| Assist tuple | `16` | `F8 / FC` | GET profile,mode; SET profile,mode,torque,character,start,`00 00` |
| Assist labels | `17` | `28 / 2C` | GET profile; SET profile,label,`00 00` |
| Profile name | `17` | `20 / 24` | GET selector; reply selector,ASCII5; legacy SET selector,ASCII5,`00`; selectors `50/51`, `60/61` |
| OEM torque bounds | `17` | `18 / 1C` | Minimum,maximum,`00 00` |
| Maximum power | `17` | `10 / 14` | GET mode `01/02/03`; reply mode,LE16 watts; SET mode,LE16 watts,`00` |
| Display speed adjustment | `16` | `D0 / D4` | Signed byte, 0.1%; valid steps 5 raw units |
| Shift system / gears | `0B` | `80 / 84` | System,gear count,`00 00` |
| Front teeth | `0A` | `48 / 4C` | Front gear count,teeth,`00 00` |
| Rear teeth | `0B` | `48 / 4C` | Gear selector,teeth,`00` |
| Rear pattern | `0B` | `B0 / B4` | LE16 index,`FF FF` |
| Start mode | `0B` | `88 / 8C` | SET start gear,original byte 1,`00 00` |
| Auto shift mode | `0B` | `00 / 04` | Byte |
| Auto cadence | `0B` | `90 / 94` | SET LE16 rpm×10,original bytes 2–5,`00` (7 bytes) |
| Shift advice | `0B` | `D8 / DC` | Byte |
| Shift limits | `0B` | `60 / 64` | SET up,down,original bytes 2–3; 255 unlimited |
| Maintenance alert | `48` | `40 / 44` | Boolean byte |
| Maintenance distance | `48` | `48 / 4C` | LE32 km; all `FF` means unset |
| Maintenance date | `48` | `50 / 54` | Year−2000,month,day,reserved; all `FF` unset |

Profile selectors 0/1 are OEM defaults, 2/3 editable user profiles. Modes 1/2/3
are ECO / TRAIL-or-NORMAL / BOOST-or-HIGH. EP800 tuples expose torque in Nm,
character 0–9 and start 0–4. Torque is not restricted to a five-Nm lattice;
factory values can be 27 Nm. OEM torque-bound controls have their own steps.
Labels 0/1 select ECO-NORMAL-HIGH / ECO-TRAIL-BOOST. Profile names allow up to
ten printable ASCII characters and need D4.1.4 or later in the legacy schema.
Each name chunk carries five text bytes, zero-filled for shorter names.

EP800 ranges include wheel 1300–3000 mm, mounting 0–60°, light output
135–2700 raw hundredths W and reserve 0–180 min. Destinations 0/3/4 permit
15–25 km/h and destination 1 permits 19–32 km/h in this model schema. Destination
3 caps maximum power at 400 W, other listed destinations at 500 W. These bounds
describe service settings, not an electrical power-stage specification.

Modern basic-mode assist tuples retain a sixth meaningful byte containing
carry-over/cutoff data; preserve it when editing another field. Modern names
use `[selector,ASCII5]` SET parameters, omitting the legacy trailing `00`;
both layouts still carry five text bytes per chunk. Modern label SET has only
profile and label. Modern character ranges differ by mode (ECO 0–9, TRAIL
0–14, BOOST 4–14). Fine-tune profiles and
free-shift are not implemented by this dictionary. E5100 and older E-series
expose narrower schemas; older lamp is 0/1 and mounting SET can be two bytes.
E6000 common wheel range is 2000–2400 mm, angle 0–45°.

On tested EP800 firmware, speed GET selector 0 aliases the effective speed;
it is not an independent OEM ceiling. Destination GET selector 0 can be rejected.
An OEM destination `FF` is unset, not EU. EP600/EP801 may support the documented
factory-selector fallback; do not copy it to EP800. Query current destination
before destination-dependent validation and stop on rejected or malformed data.

Read the full shared tuple, back up raw original plus identity, replace only the
selected field, reconstruct SET-specific reserved bytes, send once, and read
back. A GET echo verifies the live value; a fresh power-cycle GET verifies
persistence. There is no general extra EEPROM commit in the implemented setting
path. Multi-setting batches are ordered, not atomic. Profile names span two
writes and can partially complete. See [settings behavior](settings.md).

## 8. Firmware/update traffic recognition

D and M firmware are different images with companion-version requirements.
Container checksum, target series/unit, board, bootloader generation, complete
image length and companion compatibility all matter. A valid filename or outer
FCS does not establish a valid flash target. Normal version reads do not enter
boot mode; boot-capability probing can do so.

### D/DCAS family

Application address retention uses `01/C0 [slot] → 01/C2`. UPDATE/master then
uses outer `88` and five payloads:

```text
01 31 41 59    02 26 53 58    03 97 93 23
04 84 62 64    05 33 83 27
← 11 key_index ...           each key's distinct acknowledgment
```

These differ from normal PC-link keys. App-to-boot handoff can require key 5
again after a mode transition/delay. One key ACK is not a completed entry.
Legacy `88` replies do not carry an explicit component slot; serialize access.

| Boot payload prefix | Response prefix | Purpose |
| --- | --- | --- |
| `2E 00 00 00` | `3A` | First version bytes |
| `2F 00 00 00` | `3B` | Last version byte and board |
| `30 00 00 00` | `3C` | Series/unit |
| `29 00 00 00`, `2A 00 00 00` | `35`, `36` | Two three-byte serial pieces |
| `21 00 erase_code 00` | `31` | Erase/start; effectful |
| `0A page 00 00` | `31` | Set page |
| `24 00 40 00` | `31` | Initial logical write address `0x4000` |
| `27 image_sum 00 00` | `31` | Finish |
| `28 00 00 00` | `31` | Reset |

DCASX erase code is `(length−1)//2048`; older DCAS uses size-class codes.
Do not interchange them. Data relay uses outer `0B`:
`[packet,02,00,data64,padded_sum,index_lo,index_hi]`, followed by
`[next_packet,03,data_packet]`. Blocks are FF-padded to 64 bytes. Block checksum
includes padding; image finish checksum includes real bytes only. DCAS2i/X
block index is one-based LE16; legacy older layouts differ. Packet IDs for this
DCAS path wrap modulo 256; successful no-retry pairs begin data 0 / transfer 1.

Buffer acceptance (`82`), transfer acceptance (`83`), transfer completion (`C0`)
and motor checksum (`31`, with matching block index where supported) are distinct.
They can arrive asynchronously. A transport ACK cannot substitute for motor
checksum or finish. A stale unindexed `31 00 00` cannot prove block 2048.
Preserve pending completion events rather than discarding them when starting
the next wait. Unknown `12` or `70` responses are not successful key ACKs.

### Fast bridge and M/RX

The fast bridge uses addressed outer `A8` 16-byte payloads; do not strip its
slot. It is reached after boot keys, not as a workaround for incomplete keys.
Adapter `07` controls local masks and mode 5/6; D boot `4B` changes communication
mode and `4C` assigns/confirms a slot. USB `88` four-byte Pi-key payloads can be
converted by the adapter into three data bytes plus an inner checksum; host
and bus bytes are therefore not necessarily identical.

Fast RX request body starts `[slot,60,F0,05,00,cmd,p0,p1,p2,…]` and reply starts
`[slot,70,F2,…]`. Legacy RX instead uses outer `48`, request
`[slot,F0,00,cmd,p0,p1,p2]`, reply `[route,F2,capability,reply,…]`.
Do not parse either as an ordinary group/opcode setting.

RX boot query `41 → 51` reports capability and version. The M image base is
`0xFC0000`; legacy address command `24` uses three little-endian address bytes.
The ordinary application relay streams `48 [slot,F8,sequence,data4]`; sequence
increments modulo 8 starting at 1, then command `25` checks the window and `26` advances.
Windows are 1024 bytes from boot 2.0.2, otherwise 64; final data ends at the
last padded 64-byte packet, not a full artificial 1024-byte tail. The internal
`FirmwareIo` records use an address placeholder of `00`; the normal USB bridge
and BLE FREE relay replace it with the selected motor slot before dispatch.
RX retry-mode adapter packet IDs wrap modulo 255 (never 255), unlike DCAS IDs. Unsupported
retry capabilities must be rejected before erase. Finish/reset/return-to-app
and fresh D/M version reads are separate outcomes.

SM-PCE02 fast RX instead buffers **60 data bytes** with outer control `11` and
payload `[packet,11,sequence,F8,slot,data60,FF,FF,FF,FF]`. Within each checksum
block, sequence is `(fragment_index mod 43) × 6` (decimal). Load receipt `91`,
transfer-start acceptance `83` and completion `C0` are separate. Transfer start
uses `[next_packet,03,data_packet]`; `83` correlates the transfer packet ID,
whereas `C0` correlates the data packet ID. Replies retain slot and status;
require status `00`. Fast checksum windows are 1080 bytes, or 2100 for the
explicitly enabled retry-capable path. The checksum includes final 60-byte
FF padding and excludes the extra four adapter-padding bytes. Command `25`
verifies each window, `26` advances it, and retry-capable blocks reselect the
address. These sizes and envelopes differ from the ordinary 4-byte relay.

BLE D motor update relay uses PCE, not the gateway's own BOOT characteristic;
the legacy RX `F0/F8` envelopes use FREE with the selected motor slot.
Large PCE records repeat control after a sequence byte: offset `fragment_index * 0x20`,
bit `80` when more fragments follow, up to 18 payload bytes per fragment.
Here `20` is hexadecimal (32 decimal). Payload lengths 18/36/54 retain an
empty terminal record `[sequence,control]` after the full data fragments.
Prefer write-without-response for fragmented records when advertised; gateways
offering only acknowledged writes use those with the same relay receipts.
Exact terminal-record behavior is part of that codec;
see [firmware transport implementation](../src/ble/firmware.rs). Gateway radio
DFU is a different update family. The normal firmware simulator does not
execute resident flash erase/program protocols. A separate hash-pinned EP800
resident regression executes key-entry and SPI/UART routines: repeated zero
SPI input stalls Key 2 and produces empty `70` replies, while an `FF` first
SPI byte lets the queued key advance. That interpretation is bounded to the
exact resident image and modeled state; it is not a generic meaning for every
`70` frame. It does not execute erase/program/reset or full boot. See
[resident regression setup](firmware-simulator.md#ep800-resident-bootloader-and-key-2-recovery).

## 9. External packet fragments and missing physical specification

In the supported gateway receive fixture a normal reply is divided into
three-byte data chunks. Each four-byte FIFO fragment is
`[flags|sequence, data0,data1,data2]`; sequence starts at 1, first flag `80`,
last flag `40`, unused final data bytes `FF`. Example for seven-byte model data
plus group/opcode:

```text
16 7E 30 30 2D 43 52 47 00
→ 81 16 7E 30 | 02 30 2D 43 | 43 52 47 00
single fragment → C1 data0 data1 data2
```

This is a **firmware FIFO boundary**, not a complete physical capture frame.
The fixture supplies routing separately and supports 2–9-byte messages at slot
13. It does not establish preamble, address bit packing, line checksum, symbol
rate, BPSK waveform, collision priority, wire timing or every message length.
Do not build a raw-wire transmitter from these four bytes alone.

Still missing for a drop-in switch/display/derailleur: normal operating-mode
button events, acknowledgments and repeat/debounce timing; gear commands and
actuator status; live speed/cadence/torque/assist broadcasts; sleep/wake and
fault propagation; electrical loading and a complete network-management state
machine. ANT+/wireless shifter payloads also require their own specification.
The service dictionary above configures components; it is not a riding-traffic
message catalog.

## 10. Product and simulator coverage

| Category | Documented boundary | Verification / gap |
| --- | --- | --- |
| SM-PCE02 | Host framing, power, discovery, selected boot bridges | Host integration; actual adapter CPU not part of normal co-sim |
| SC-EM800 4.0.0/4.1.0/4.3.0/4.4.0/4.5.0/4.6.0 | Gateway packet routing and reassembly | Six exact firmware profiles; BLE model too |
| SC-E6100 4.1.0; SC-E7000 4.1.0 | Wired packet gateway | Exact profiles; not claimed BLE coverage |
| EP800 D4.1.10/4.1.11/4.2.0 | Normal D packet handlers | Exact profiles; D4.2.0 has narrow persistent lamp/wheel fixture |
| EP800 resident 5.0.6.1 | Five-key entry and SPI/UART service | Separate exact 64 KiB hash profile; five bounded scenarios, no erase/program/reset or full boot |
| EP600 D4.4.1/4.6.0; EP500 D4.1.0/4.1.1 | Normal D packet handlers | Exact profiles; no blanket persistent-write claim |
| E5000 D4.5.0; E5100 D4.1.0; E6100 D4.8.0 | Normal D packet handlers | Exact profiles; model/schema gates remain separate |
| EP801 | Identity and modern service implementation | Not separately proven by executing an EP600 image |
| E6000/E6001/E6002/E6010/E6012/E6050 | Identity/version distinctions, limited service knowledge | Outside main ARM simulator; separate bounded older-CPU work |
| E7000/E8000/E8080 and variants | Model/schema/auth policy | Not in current executable motor matrix |
| SC-EN500/EN600/EN610, SC-E8000, SC-M9050/MT800, EW-EN100/WU gateways | Known product families | No inference of executable support from their names |
| Di2 road/MTB/GRX, Alfine/Nexus MU units, switches, suspension, power meters | Broad identity catalog in source | No complete runtime command or radio specification |
| BT-E / BT-EN / partner traction batteries, CAN adapters | Separate subsystem | No traction-BMS firmware execution or universal acceptance proof |

The compatibility matrix crosses eight gateway images with ten motor images:
80 pairs × nine scenarios = 720 packet scenarios. It tests normal handshake,
reads and cleanup; dropped/duplicated/reordered/delayed keys; dropped replies;
and truncated/reordered/bad-sequence reply fragments. Single-fragment reordering
is correctly a no-op. Delay uses logical events, not a physical watchdog clock.
Synthetic RAM, transport acceptance, radio topology and EEPROM boundaries are
modeled. The motor M version can be a behavior peer rather than executing M.
Matrix success does not certify those combinations as real supported bikes.
A fresh native SC-E7000/E6100 D4.8.0 probe rejects `01/2C 00` with error
opcode `2F` even though its matrix handshake/read/cleanup scenarios pass.
Bounded checks on that image return `01 2F 39 00 FF FF` for `01/2C`
selectors 0–3, whereas `01/84 00` returns `01 86 48 00 00 00` (4.8.0.0).
This differs from the newer dual-MCU selector convention in the identity table.
Do not describe that pairing as a verified complete client probe.

See [simulator setup and exact boundaries](firmware-simulator.md). Adding a
product requires a firmware hash, CPU/ABI and memory profile, bounded execution,
positive and negative command tests, and explicit transport/storage limits.
Listing a product in [the identity catalog](../src/unit_models.rs) is not a
support promise. Shimano's current compatibility charts also include newer
BT-DN320 and FH-U6060 systems; the present service reference does not establish
their runtime protocol.

## 11. Traffic decoding and implementation checklist

1. Record observation point, direction, timestamp, firmware/model/board and
   discovery map. Preserve raw bytes alongside decoded fields.
2. USB: unframe/unescape/check FCS, then interpret control. BLE: identify
   characteristic first and retain record boundaries. Bus FIFO: reassemble only
   after validating sequence/flags and retaining the route context.
3. Correlate by transport, route, session, group/opcode and selector or block ID.
   A matching opcode alone is insufficient. The normal protocol has no general
   transaction ID to disambiguate concurrent identical requests.
4. Separate adapter acceptance, component reply, authorization, live readback,
   durable readback, firmware finish and normal application return.
5. Serialize one session owner per port/gateway. On timeout or disconnect,
   retain uncertain outcomes and invalidate routing/auth assumptions. Do not
   replay a SET/erase automatically just because its reply was lost.
6. Redact credentials and private identities from shareable traces. Keep unknown
   opcodes/statuses as raw data instead of inventing semantics.

Golden USB examples (TX before trailing padding):

```text
cargo GET slot 13: BB 48 0D 16 7C 00 19 BB
lamp GET slot 13:  BB 48 0D 10 14 00 87 BB
escaping fixture: BB 48 BD 9B BD 9D 40 BB
```

The first two require 120 trailing `FF` bytes on a short SM-PCE02 write.
The escaping fixture is codec data, not a valid component command. A cargo
reply example `BB 48 0D 16 7E 30 30 2D 43 52 47 00 AE BB` illustrates parsing;
its route byte must be interpreted in the capture's session context, not copied
as the expected recipient for every USB session.

### Reproduce the checks

With the [simulator environment and local firmware manifest](firmware-simulator.md)
prepared, run from the repository root:

```sh
python tools/firmware-sim/sim.py matrix \
  --fixtures /absolute/path/firmware-fixtures.json --output /tmp/etube-matrix.json
CRANKLINK_SIM_FIXTURES=/absolute/path/firmware-fixtures.json \
  python -m unittest discover -s tools/firmware-sim -p 'test_*.py'
```

Stock-image tests skip if their manifest is absent. Resident tests separately
require `CRANKLINK_RESIDENT_IMAGE`; no resident dump is bundled or auto-pulled.
The native and GATT client verifiers and their topology limits are documented
in the simulator guide. A nonzero verifier exit remains a failure even when
other matrix scenarios pass; the E6100 version-query limitation above is one
such case. The codec golden vectors and all serial split boundaries are also
covered by `cargo test --locked --lib protocol::`.
