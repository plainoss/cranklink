# DCASX motor speed-limit patcher and bounded emulator checks

`tools/patch_motor_speed.py` is an experimental offline firmware analysis tool for
legacy DCASX motors. It handles raw `.bin` images and encrypted Shimano `.dat`
containers. `tools/patch_ep800_speed.py` remains a compatibility entry point.

## Supported images and limits

| Family | Application versions | Series |
|---|---|---|
| DU-EP800 / DU-EP800-CRG | D4.1.10, D4.1.11, D4.2.0 | `0x23` |
| DU-E6100 | D4.8.0 | `0x20` |
| DU-E5100 | D4.1.0 | `0x38` |
| DU-E5000 | D4.5.0 | `0x22` |

The tested range is **15.00–99.00 km/h**, with a default ceiling of **60.00 km/h**.
`--speed-limit` / `-s` always takes km/h, with at most two decimal places. There
is no implicit raw-integer mode. Out-of-range, non-finite, and excess-precision
inputs fail before an output file is written. The Python `patch_binary` API takes
integer hundredths of km/h (`1500..9900`).

The earlier prototype's unsigned maximum of `65535` is unsupported. Although
SET/GET acknowledged it, the stock EP800 assist routine uses signed 16-bit
intermediates and clamps negative results. At `60000`, the sampled thresholds
became `[0, 0, 0, 0]`; at `65535`, they became `[124, 0, 0, 0]`. A successful
protocol acknowledgment does not establish correct assistance calculations.
The 99 km/h cap is a conservative tested envelope, not the theoretical maximum
of every firmware arithmetic path or proof of physical motor behavior.

Modern EP600/EP500/EP801 images do not use the same legacy speed-setting path.
E8000 and E6000 use different architectures. These families are rejected by this
patcher. Emulator verification also rejects versions without an exact supported
version profile instead of guessing another version's function addresses.

## Patch mechanism

The tool locates three code sites by their Thumb signatures:

| Site | Signature | EP800 D4.2.0 address |
|---|---|---|
| Ceiling lookup | `02 28 07 d0 04 d3 04 28` | function `0x16b8e` |
| SET bound comparison | `38 00 21 00 80 b2 89 b2 88 42` | branch `0x16a46` |
| Startup cache comparison | `02 00 0b 88 92 b2 9a 42` | branch `0x15256` |

1. Replace the ceiling function with position-independent `movw r0, #limit;
   bx lr; nop`. No PC-relative literal alignment is required.
2. Retain or restore SET's `bhs` (`01 d2`): the firmware compares the ceiling
   against the requested value and accepts only values at or below the ceiling.
3. Retain or restore startup's `bhs` (`00 d2`): the firmware leaves valid cached
   values unchanged and clamps stale values above the configured ceiling.

The earlier prototype changed both branches to unconditional jumps. Repatching
such an image restores the bounds. Patching the ceiling raises the accepted
maximum; it does not itself persist a selected runtime speed setting.

Image length and compatibility fields remain unchanged. Encrypted output gets a
new plaintext MD5 and AES-CBC envelope. The offline analysis computes the image's
additive checksum; it does not rewrite an assumed checksum byte in the header.

## Usage

```sh
# Default 60.00 km/h ceiling; use a profiled, decrypted SC-EM800 gateway image.
python3 tools/patch_motor_speed.py \
  -i DUEP800-D.4.2.0.dat \
  -o DUEP800-D.4.2.0.patched.dat \
  --gateway SCEM800.4.6.0.decoded.bin \
  --verify

# Explicit ceiling for a raw image.
python3 tools/patch_motor_speed.py \
  -i DUEP800-D.4.2.0.decoded.bin \
  -o DUEP800-D.4.2.0.patched.bin \
  -s 50.00 \
  --gateway SCEM800.4.6.0.decoded.bin \
  --verify --json

# Other supported motors receive the smaller direct-function verification scope.
python3 tools/patch_motor_speed.py \
  -i DUE6100-D.4.8.0.dat -o DUE6100-D.4.8.0.patched.dat --verify

# Compatibility entry point, with identical options and units.
python3 tools/patch_ep800_speed.py \
  -i DUEP800-D.4.2.0.dat -o DUEP800-D.4.2.0.patched.dat -s 45.00
```

`.dat` handling needs PyCryptodome; emulator checks need Unicorn. Supply
`--gateway` for EP800 D4.2.0; historical local fixture paths are also searched.
Output is written only after requested checks complete successfully.

## What verification establishes

**Offline flash layout analysis** validates the DCASX header, declared size,
trailer, architecture and supported series, then calculates the 64-byte block
count, erase dimensions and additive image checksum. JSON reports this under
`flash_analysis`, with `status: "analyzed"` and `bootloader_executed: false`.
The old Python name `verify_flash_simulation` is an alias for this analysis.
It does not execute a bootloader, erase flash, validate transport acknowledgments,
or establish executable integrity or physical flashability.

**EP800 D4.2.0 application verification** reports
`scope: "modeled_settings_persistence_and_assist"`. It:

- Executes the patched ceiling function and actual startup clamp, checking that
  low/at-ceiling values survive and a stale `65535` cache is clamped.
- Connects motor and stock gateway application routines over the virtual bus and
  authenticates using the motor's real AES path.
- Sweeps distinct SET values from 15 km/h through the selected ceiling and
  checks acknowledgment, GET readback and a modeled EEPROM commit for each.
- Reads EEPROM record `0x29` from a temporary disk file independently of the RAM
  cache, then recreates CPU/RAM, reloads the file, executes the actual startup
  clamp and checks GET again. Authentication must reset on each reload.
- Rejects writes above the ceiling and confirms they change neither RAM nor the
  persisted file.
- Checks the SET-generated synthetic EU fixture's assist thresholds at
  `0x200024b8` against `[speed+125, speed-300, speed-200, speed]` at every step.

The initial connection uses the simulator's synthetic 25 km/h cache so the first
15 km/h SET must execute a real modeled commit even with a 15 km/h ceiling.
Subsequent connections exercise the persisted record and startup clamp.

**Other supported versions** report `status: "partial"` and
`scope: "ceiling_function_boot_clamp_and_get_handler"`. They check the ceiling
return value, execute the clamp against low/at-ceiling/above-ceiling cache values,
and exercise GET with synthetic connection state. SET authentication, persistence
and assist curves remain unverified on those versions.

Every result lists the unverified scope. Full boot, physical EEPROM and hardware,
and bootloader flashing remain outside these checks; no output claims “100%
verified” or safe flashing. Generated containers can be inspected separately:

```sh
cranklink firmware inspect DUEP800-D.4.2.0.patched.dat \
  --companion DUEP800-M.4.1.11.dat
```

## Regression tests

```sh
python3 -m unittest discover -s tools/firmware-sim -p test_ep800_patcher.py -v
```

Input/CLI/layout tests use synthetic images and run without private firmware.
Emulator and real-container cases run when their local private fixtures exist.
Regression cases cover unsigned-overflow inputs, unit confusion, destroyed
headers, zeroed assist tables, missing disk commits, stale reloads, corrupted
startup instructions and bypassed SET/startup ceiling checks.
