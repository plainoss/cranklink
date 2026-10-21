# Restore an EP800 through E-TUBE with a temporary resistor bridge

Use this procedure for a motor that answers the first firmware connection key
but stalls before the second, repeatedly sending empty `0x70` responses. The
bridge lets its bootloader—the motor's recovery program—accept the connection
and restore the original D firmware through E-TUBE. M firmware and settings are
left unchanged. Try the [normal recovery CLI](../README.md#recover-a-motor-stuck-in-bootloader-mode)
first.

This worked on one **DU-EP800-CRG, board 1, series `23`, unit `00`, bootloader
5.0.6.1**, using an **SM-PCE02 and macOS native USB**. The public tools below were
adapted from that successful recovery and tested offline; this public build has
not been rerun on hardware. Other board revisions and platforms remain untested.

## 1. Connect the marked points

Disconnect battery and USB before positioning the contacts. Use the connector
board shown below, which holds the E-TUBE sockets and both processors. No battery
was needed for the successful recovery.

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

![E-TUBE breakout connection used for recovery](images/ep800-recovery/etube-wiring-cleaned.jpg)

Background-cleaned illustration of the breakout. Use the contact mapping below
for wiring; fine details in the illustration may differ from the photograph.

For this breakout:

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
| Decoded D, checked internally | `f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551` |

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
Compare the settings' raw and decoded values with your original backup. Read the
settings to compare them; a `settings read --output` snapshot is not a restore
input.

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
power. Once the restore starts, hold the bridge steady until its final result.

## What the tools check

The dedicated pair below preserves the connection sequence used successfully on
hardware. The ordinary CLI's `--recovery` setup differs and was not the sequence
validated for this failure.

- [`sustained_key_epochs`](../tools/ep800-recovery/src/bin/sustained_key_epochs.rs)
  checks adaptor readiness, selects protocol **`0x60`**, target **31**, the target
  mask and legacy framing selector **5**, then enables power. It starts the first
  key about **60 ms** later. Each attempt allows **10 seconds** and at most **120**
  key transmissions. Keys 1/2 can retry at intervals of at least **100 ms**;
  after Key 2, each next key requires the preceding acknowledgement.
- [`restore_sustained_boot`](../tools/ep800-recovery/src/bin/restore_sustained_boot.rs)
  checks the raw exchanges, checksums and ordered five-key proof, which must be
  less than **20 seconds** old. Before erase, it reads the motor identity and
  requires your supplied serial, bootloader **5.0.6.1**, board **1**, series **`23`**,
  unit **`00`**, slot **31**, and the exact stock files above. It records progress
  in a durable journal and requires finish/reset acknowledgements.

The wrapper moves directly between these stages. Do not run another diagnostic
or power-cycle the adaptor between them.

Offline tests, without hardware or firmware files:

```sh
python3 -m unittest discover -s tools/ep800-recovery -p 'test_*.py'
cargo test --locked --manifest-path tools/ep800-recovery/Cargo.toml
```

The [image manifest](images/ep800-recovery/manifest.json) records the source
photos, edits and file hashes.
