# Motor firmware speed-limit patcher

`tools/patch_motor_speed.py` changes the speed-setting ceiling in supported D
application firmware. It works offline with raw `.bin` images and encrypted
Shimano `.dat` containers. Supply your own firmware; none is included.

The default ceiling is **60 km/h**. Choose **15–99 km/h**, with up to two decimal
places. Raising the ceiling does not change the motor's saved speed setting;
that setting must be written separately, and `cranklink settings set` itself
only writes speeds its destination table allows (at most 32 km/h, 45 km/h for
modern destination 5). The tool does not flash a motor or modify M firmware.

## Supported images

| Motor | D application versions |
|---|---|
| DU-EP800 / DU-EP800-CRG | 4.1.10.0, 4.1.11.0, 4.2.0.0 |
| DU-E6100 | 4.8.0.0 |
| DU-E5100 | 4.1.0.0 |
| DU-E5000 | 4.5.0.0 |

The tool accepts only the exact unmodified stock images identified by its
built-in hashes. A matching version label is not enough. To choose a different
ceiling later, start again from the original stock file.

EP600, EP500, EP801, E8000 and E6000 are not supported. Only EP800 D4.2.0.0 has
been tested on a physical motor; for the other images the tool prints a warning
and reports `"physically_tested": false` in `--json` output. The 99 km/h limit bounds the tested arithmetic
range; it does not establish the motor's physical assistance behavior.

## Create an image

Use Python 3.10 or later. Install the optional dependencies in a virtual
environment: PyCryptodome for `.dat` files, and Unicorn for `--verify`.

```sh
python3 -m venv .venv-patcher
.venv-patcher/bin/pip install -r tools/firmware-sim/requirements-test.txt

.venv-patcher/bin/python tools/patch_motor_speed.py \
  --input DUEP800-D.4.2.0.dat \
  --output DUEP800-D.4.2.0.patched.dat \
  --speed-limit 65 \
  --verify --gateway SCEM800.4.6.0.decoded.bin
```

For EP800 D4.2.0.0, `--verify` also needs a stock SC-EM800 gateway binary matching
a [simulator profile](../tools/firmware-sim/profiles.json). Pass its path using
`--gateway`. Other supported versions use a smaller verification scope and do
not need a gateway.

Omit `--verify` to patch without running the emulator. Use `.bin` as the output
extension for a raw image, or `.dat` for an encrypted container. A raw image has
no container checksum, so `cranklink firmware flash` writes one only with
`--expect-sha256`. Add `--json` for a machine-readable report. Requested checks
must pass before output is written.
An existing output file is never replaced unless `--force` is given, and the
output is written to a temporary file and renamed into place.
`tools/patch_ep800_speed.py` accepts the same options as a compatibility entry
point.

Inspect an output container with the regular CLI before considering a flash:

```sh
cranklink firmware inspect DUEP800-D.4.2.0.patched.dat \
  --companion DUEP800-M.4.1.11.dat
```

## What changes

The ceiling function becomes an eight-byte, position-independent sequence of
16-bit Thumb-1 instructions: `movs`, `lsls`, `adds`, and `bx lr`. It uses no
`movw` or `movt` instructions and needs no literal pool.

The firmware's SET bound check and startup clamp remain conditional. Requests
above the ceiling are rejected; a saved setting above it is clamped on startup.
Only the eight-byte ceiling function changes. Image length and compatibility
fields remain unchanged, and `.dat` integrity data is rebuilt.

## What has been checked

For **EP800 D4.2.0.0**, `--verify` executes the ceiling function, startup clamp,
authentication, SET/GET paths, and assist-threshold calculations. It checks settings
against a temporary EEPROM file and reloads them into fresh emulator state.
Above-ceiling writes must fail without changing the saved setting.

For **other supported versions**, it checks the ceiling function, startup clamp
and GET handler. SET authentication, persistence and assistance calculations are
not verified for those versions.

These checks do not execute a complete boot or bootloader flash, model physical
EEPROM, or prove instruction-set compatibility. In particular, Unicorn can
accept `movw` even with a Cortex-M0 profile; the replacement instruction bytes
are checked separately by the regression tests.

On one EP800-CRG running D4.2.0.0, corrected 30 km/h and 65 km/h images were
flashed and booted without the recovery bridge. A 65 km/h setting was accepted
and survived an adaptor power restart; the other 73 settings were unchanged.
M firmware was not flashed. Assistance under mechanical load was not tested.

## Run regression tests

```sh
.venv-patcher/bin/python -m unittest discover \
  -s tools/firmware-sim -p test_ep800_patcher.py -v
```

Synthetic-image tests run without firmware files. Additional emulator and
container tests run when the required local firmware fixtures are available.
