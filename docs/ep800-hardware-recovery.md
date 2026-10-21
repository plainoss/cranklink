# Restore an EP800 through E-TUBE with a temporary resistor bridge

Use this procedure for a motor that answers the first firmware connection key
but stalls before the second, repeatedly sending empty `0x70` responses. The
bridge lets its bootloader—the motor's recovery program—accept the connection
and restore the original firmware through E-TUBE. **D** is the Shimano
communication processor; **M** is the Renesas motor-control processor. This
procedure restores D only, leaving M firmware and settings unchanged. Try the
[normal recovery CLI](../README.md#recover-a-motor-stuck-in-bootloader-mode) first.

This worked on one **DU-EP800-CRG, board 1, series `23`, unit `00`, bootloader
5.0.6.1**, using an **SM-PCE02 and macOS native USB**. The public tools below were
adapted from that successful recovery and tested offline; this public build has
not been rerun on hardware. Other board revisions and platforms remain untested.

## Why the bridge helps

We read the bootloader from a working EP800-CRG with the same bootloader
version and ran that code in the firmware simulator. The tests
showed how repeated empty replies on SPI, the internal link between the two
processors, could keep the bootloader busy sending `0x70` responses instead of
processing the second E-TUBE connection key.

When the simulated SPI input returned `0xFF` (all bits high), the bootloader
stopped accepting those empty replies and could process the remaining keys.
This led us to try a resistor that pulls the corresponding board signal high.
Keeping it connected allowed a complete stock firmware restore on the failed
motor. That supports the simulator explanation; the failed motor's internal
SPI data was not directly measured.

You do not need a donor motor or a bootloader dump to follow this guide. The
tools restore the original application firmware; they do not replace the
bootloader or flash the donor's code.

## Find the connector board

Disconnect the battery and USB before opening or handling the motor.

![Opened motor showing gears and the power board](images/ep800-recovery/opened-motor-cleaned.png)

The board around the gears is the power board. The recovery contacts are on the
separate connector board shown below.

![Connector board: E-TUBE sockets and both processors](images/ep800-recovery/connector-board-processors-cleaned.png)

The connector board holds the E-TUBE sockets, the large Renesas processor (M),
and the smaller square SHIMANO chip (D). This is the side needed for the bridge.

![Reverse of the connector board, showing its board-to-board connector](images/ep800-recovery/connector-board-reverse-cleaned.png)

The large connector on the reverse side links the two boards.

## 1. Connect the marked points

With battery and USB disconnected, position the contacts on the connector
board. The adaptor supplies power for recovery; no battery is needed.

![Power and signal points for the temporary 20 ohm bridge](images/ep800-recovery/bridge-location-labelled.png)

With the **SHIMANO lettering upright**:

| Point | Where to connect |
|---|---|
| **Power point** | The marked exposed via just left of the Shimano chip's lower-left corner, above the left of the two large ceramic capacitors. |
| **Signal point** | The marked right-hand solder pad of the lower small horizontal component beneath the upper-right small chip. |

```text
Marked power point ── 20 ohm resistor ── Marked signal point
```

Use the resistor, not a direct wire. **20 ohms worked on this board; confirm your
board matches before using it.** The power point was used as a nominal 3.3 V
source; its voltage under load was not independently recorded. Arrange an
insulated, secure connection that can stay in place for the entire transfer.

## 2. Connect E-TUBE

![E-TUBE wiring overview with the full cable harness](images/ep800-recovery/etube-wiring-cleaned.png)

For the breakout used in this recovery:

| Viewing direction | E-TUBE inner conductor | E-TUBE outer conductor |
|---|---|---|
| Male plug viewed into its mating face, **flat part down** | Rightmost contact | Leftmost contact |
| Motor casing in the photographed orientation | Upper | Lower |
| Connector board with its identification lettering readable in the operator's orientation | Upper contact | Lower contact |

Looking from the cable side reverses left/right. Inner/outer describe the cable
conductors; this table does not assign positive/ground or wire colours. Preserve
the viewing orientation when comparing your wiring.

## 3. Prepare the tools and original firmware

You need Rust, Python 3, your motor's known **12-digit hexadecimal serial**, and
your own stock firmware files. Close other programs using the adaptor. Save any
existing settings backup and keep the recovery output private: it contains device
identifiers.

The tools accept only **D 4.2.0.0** with **M 4.1.11.0**. Confirm from your records
that M already has that version. The supplied M file is checked for compatibility
but is not flashed; its live version cannot be checked before restoring D.

| File | Required SHA256 |
|---|---|
| Stock D container | `35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543` |
| Stock M container | `94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44` |

From the repository root, build the tools:

```sh
cargo build --locked --manifest-path tools/ep800-recovery/Cargo.toml --bins
```

Replace the paths and serial below with your own. Run this preparation check
first; without `--execute`, it does not connect to hardware:

```sh
python3 tools/ep800-recovery/recover.py \
  --d /path/to/DUEP800-D.4.2.0.dat \
  --m /path/to/DUEP800-M.4.1.11.dat \
  --serial YOUR12HEXUID --output ./recovery-run-01
```

No firmware files are included in this repository.

## 4. Run the restore

With the wiring and resistor secured, connect USB. Run:

```sh
python3 tools/ep800-recovery/recover.py \
  --d /path/to/DUEP800-D.4.2.0.dat \
  --m /path/to/DUEP800-M.4.1.11.dat \
  --serial YOUR12HEXUID --output ./recovery-run-01 \
  --execute --countdown 10 --attempts 20
```

The countdown gives you ten seconds to establish contact. The script tries the
connection up to twenty times, then starts one stock restore when all five keys
are acknowledged and the motor's identity matches.

**Keep the resistor connected through erase, the entire transfer, finish and
reset.** Releasing it after the keys is too early. In the original recovery,
removing it during transfer caused the motor to stall again.

Wait for `Stock transfer finish/reset acknowledged` and a successful exit. The
journal must contain `finish-and-reset` with **157,996 bytes**, checksum **`0xCB`**,
`finish_acknowledged: true` and `reset_acknowledged: true`.

Only then remove the temporary resistor, leaving USB and E-TUBE connected.

## 5. Verify without the resistor

```sh
cargo build --locked --bin cranklink
./target/debug/cranklink --port usb probe
./target/debug/cranklink --port usb -s YOUR12HEXUID settings read \
  --output settings-after.json
```

Check the original serial and cargo identity, D **4.2.0.0**, and M **4.1.11.0**.
Compare the settings with your original backup.

The successful recovery verified normal communication and **74/74 original
settings** after removing the resistor. A full flash-byte readback and physical
motor operation were not tested. Check operation separately after reassembly.

## If the transfer is interrupted

Keep the output directory and journal. The script does **not** retry a restore,
resume a partial image or reset the motor after an uncertain write. Diagnose the
failure before deliberately starting another complete restore with a new output
directory. The successful case needed a fresh connection and full rewrite after
an interrupted first attempt.

Connection-only retries do not erase firmware, but may reset or cycle adaptor
power.

The dedicated recovery tools use the connection sequence that worked on this
motor. The ordinary CLI's `--recovery` setup differs. Developers can find the
protocol details and offline tests in [the tool notes](ep800-recovery-tools.md).
