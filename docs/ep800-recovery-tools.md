# EP800 recovery tool implementation

The [repair guide](ep800-hardware-recovery.md) contains the procedure.
The details below explain the dedicated helpers for developers maintaining them.

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
  unit **`00`**, slot **31**, and the exact stock files listed in the [repair guide](ep800-hardware-recovery.md#3-prepare-the-tools-and-original-firmware). It records progress
  in a durable journal and requires finish/reset acknowledgements.

The wrapper moves directly between these stages. Do not run another diagnostic
or power-cycle the adaptor between them.

Offline tests, without hardware or firmware files:

```sh
python3 -m unittest discover -s tools/ep800-recovery -p 'test_*.py'
cargo test --locked --manifest-path tools/ep800-recovery/Cargo.toml
```
