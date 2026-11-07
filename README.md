# cranklink

[![CLI](https://github.com/plainoss/cranklink/actions/workflows/build-cli.yml/badge.svg)](https://github.com/plainoss/cranklink/actions/workflows/build-cli.yml)
[![Web](https://github.com/plainoss/cranklink/actions/workflows/web.yml/badge.svg)](https://github.com/plainoss/cranklink/actions/workflows/web.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**cranklink** can change a Shimano STEPS e-bike motor's market region and flash its firmware, including on recent motor firmware. Connect wirelessly over Bluetooth through the bike's own display or wireless unit, or with a wired SM-PCE02 adapter. No dealer visit needed.

The project began as the recovery of a bricked EP800-CRG motor that official tools and dealer service could not restore. It is unofficial and not made or endorsed by Shimano.

## What you can change

- **Market region:** the sales market the motor is set for, which sets its maximum assist speed. Works over Bluetooth and wired.
- **Motor firmware:** flash the motor's firmware from your own firmware files, or recover a motor stuck in its bootloader.
- **Assist feel:** how strong and how quick each assist mode is, plus wheel size.
- **Brake/shift levers and handlebar switches:** what each button does, including swapping the assist and shift buttons on e-bike switches.
- **Derailleurs:** how fast they shift several gears at once, and synchronized shifting.
- **Wireless parts:** the Bluetooth passkey, and the radio mode on 12-speed wireless rear derailleurs.
- **Display:** its settings and clock.
- **Internal gear hub and Di2 battery:** hub adjustment and multi-shift limits.

Not sure your parts are covered? See [Supported parts](#supported-parts).

## Before you change anything

> [!WARNING]
> Writing settings or firmware can permanently damage motors, displays or batteries, and voids warranties. Raising the assist speed limit can make your bike illegal to ride on public roads where you live. Use at your own risk.

## Get started

### Web app

**[cranklink.plainoss.workers.dev](https://cranklink.plainoss.workers.dev/)**

[![Web app, assist profile settings](docs/images/web-instrument.png)](https://cranklink.plainoss.workers.dev/)

Open it in Chrome or Edge on a computer or Android phone (Safari and iOS can't talk to Bluetooth or USB devices from a web page). Everything runs in your browser. Connect in one of two ways:

- **Bluetooth:** put the bike's display or wireless unit (for example an SC-EM800 or EW-WU111) into connection mode, close other apps connected to the bike, and press **Connect**. The app shows how to do this for your parts; see also [connection mode](docs/bluetooth.md#connection-mode).
- **Wired:** plug in an SM-PCE02 adapter. On Android, use WebUSB.

After connecting, choose the part you want to change under **Active component**: motor, lever, derailleur, battery, hub or display. The app saves a backup before every write and checks the result afterwards.

When you are done, press **Disconnect** and wait until the display, buttons and shifting respond normally again.

If connecting fails, choose **View details & logs**, then **Copy diagnostic report** to share what went wrong.

### Android app

Install **cranklink** from Google Play (`app.plainoss.cranklink`), Android 8.0 or later. It connects to STEPS e-bikes over Bluetooth and covers motor settings, with the same backups and checks as the web app. Demo mode lets you try it without a bike. Di2 parts and firmware flashing are web app and command line only. See the [Android app](android-app/README.md) for other ways to install it.

On first launch, a short warranty and component-damage warning requires acknowledgment before continuing.

### Command line tool

For recovering a motor stuck in its bootloader, and for scripting, use the [command line tool](#command-line).

---

## Supported parts

The rest of this page is reference detail. You only need it to check a specific part, or to use the command line.

### Motors

| Motor | Motor firmware | What you can change | Status | Firmware recovery |
| :--- | :--- | :--- | :--- | :--- |
| **DU-EP800-CRG** | 4.1.10 – 4.2.0 | Assist tuning, region, wheel size | Tested on a bike, over Bluetooth (SC-EM800) and wired | Yes, tested |
| **DU-EP800** | 4.1.10 – 4.2.0 | Assist tuning, region, wheel size | Preview | Yes² |
| **DU-EP600, EP801, EP500** (and CRG versions) | 4.4.1 – 4.6.0 | Assist tuning, region, wheel size | Preview | Opt-in¹ |
| **DU-E5100** | All | Region, assist speed limit, maintenance settings | Preview | Opt-in¹ |
| **DU-E8000, E8080** | 4.1.0 – 4.10.0 | Assist presets, Boost/Trail levels, region, wheel size | Preview; reading tested on a bike | Opt-in¹ |
| **DU-E7000** | All | Assist presets, Boost/Trail levels, wheel size, mounting angle | Preview | Opt-in¹ |
| **DU-E6100, E6110, E6180** | All | Assist pattern (Comfort or Sportive), wheel size, mounting angle | Preview | Opt-in¹ |
| **DU-E6002, E6012** | 3.3.0 and newer | Region (Europe, US, Korea) | Preview | Opt-in¹ |
| **DU-E6000, E6001, E6010, E6050** | 3.2 – 3.9 | Wheel size, mounting angle | Preview | Opt-in¹ |
| **DU-E5000, E5080** | All | Nothing; identified only | — | — |

- **Tested on a bike:** confirmed on real hardware.
- **Preview:** built and checked against the protocol in software; confirmation on real bikes is in progress.
- ¹ Command line only, with `--allow-unverified-model`. Not run on hardware.
- ² Shares the EP800-CRG's identity (`23/00`) and firmware, so the command line accepts it without `--allow-unverified-model`. Not run on hardware.

E7000, E6100 and older E6000-family motors keep their factory region. Which fields appear depends on the motor's firmware; see [settings](docs/settings.md), [market regions](docs/settings.md#market-regions) and [legacy assist tuning](docs/settings.md#legacy-assist-tuning-e8000-e7000-e6100).

### Other parts

| Part | What you can change | Models |
| :--- | :--- | :--- |
| Road and gravel Di2 levers (10 and 11-speed) | Button functions, top buttons, bike computer buttons (D-Fly) | Classic levers; top buttons and D-Fly on ST-R9150, R8050, RX815 |
| E-bike handlebar switches | Button functions, swap assist and shift buttons | SW-E7000, SW-E6000, SW-E6010-L, SW-E8000-L |
| Front and rear derailleurs (10, 11 and 12-speed) | Multi-shift speed, synchronized shifting | Di2 derailleurs |
| 12-speed wireless rear derailleurs | Radio mode, pairing passkey | RD-R9250, R8150, R7150, RX825, RX827, RX717, RX828-SGS |
| Wireless units and wireless displays | Bluetooth passkey | EW-WU101, EW-WU111, EW-EN100, wireless SC-E displays |
| E-bike displays | Display settings, clock | SC-E8000, E6100, E7000, EM800, EN600, EN610 |
| Internal gear hub shifting units | Hub adjustment | MU-S705, MU-UR500, UR510, UR520 |
| Di2 batteries | Multi-shift limits | BT-DN110, BT-DN300 |

Not supported yet: 12-speed levers (wired and wireless) and SW-EN600 and SW-EM800 switches. These layouts have protocol tests but have not been checked on real bikes. To see every field for a part, run `cranklink settings schema --model <MODEL>`.

## Command line

### Install

Download the latest build for Linux, macOS (Apple Silicon and Intel) or Windows from the [rolling prerelease](https://github.com/plainoss/cranklink/releases/tag/rolling), which is rebuilt from `main` and may contain unreleased changes, or a versioned [release](https://github.com/plainoss/cranklink/releases). Each binary has a `.sha256` checksum. macOS binaries are not notarized: if macOS blocks the binary, run `xattr -d com.apple.quarantine ./cranklink`. How releases are built is in [CLI releases](docs/cli-releases.md).

To build from source (Rust 1.90+):

```sh
git clone https://github.com/plainoss/cranklink.git
cd cranklink
cargo build --release --locked --bin cranklink
./target/release/cranklink --help
```

Bluetooth support is included by default; add `--no-default-features` for a USB-only build. Linux Bluetooth needs a C compiler and BlueZ/D-Bus; macOS needs Bluetooth permission for the terminal. See [Bluetooth](docs/bluetooth.md).

**Connecting the adapter**

- **macOS**: `--port usb` for direct SM-PCE02 access.
- **Linux**: `--port usb` needs access to USB device `1e44:7151`, for example with a udev rule in `/etc/udev/rules.d/70-sm-pce02.rules`:

  ```text
  SUBSYSTEM=="usb", ATTR{idVendor}=="1e44", ATTR{idProduct}=="7151", MODE="0660", TAG+="uaccess"
  ```

  Then run `sudo udevadm control --reload && sudo udevadm trigger`, or replug the adapter. `uaccess` grants access only to the user at the local seat; on a headless or SSH-only host, use `GROUP="plugdev"` (or another group you are in) instead. A serial path such as `/dev/ttyUSB0` exists only if a USB-serial driver is bound to the adapter; cranklink does not detach that driver, so `--port usb` cannot claim the adapter while it is bound.
- **Windows**: prefer the FTDI COM port (`--port COM3`); direct USB (`--port usb`) requires WinUSB.
- Bridge recovery (`--bridge-recovery`) and `firmware dump-bootloader` work only over direct USB (`--port usb`), not a serial or COM port.
- Close other adapter applications first. If the adapter's supercapacitor needs charging, cranklink waits up to 90 s; keep USB connected.

### Connect and identify

```sh
cranklink scan                 # attached USB adapters (add --ble for Bluetooth gateways)
cranklink --port usb units     # components on the E-TUBE bus
cranklink --port usb units --stock-only  # stock identities/candidates; skip optional part queries
cranklink --port usb probe     # drive unit model, serial, firmware versions
cranklink --port 'ble:DEVICE_ID' --read-only ble detect-system # master family, before service mode
```

```text
Drive Unit Probe:
  Model:       DU-EP800-CRG (Slot 13)
  Serial:      010203040506
  Cargo ID:    00-CRG
  DCAS (D):    v4.2.0.0
  Renesas (M): v4.1.11.0
```

Settings writes (`set` and batches that set) and ordinary firmware flashing require `-s <SERIAL>` (the 12-digit hex serial from `probe`) and fail if the connected unit reports a different serial, checked again before each settings SET; restores are bound to the serial in their backup. Bridge recovery can read and bind the serial automatically after connecting. Record the serial before flashing: a motor stuck in its DCAS bootloader cannot run `probe`, but reports it as `Serial:` in recovery step 2, and a serial mismatch names the live serial.

### Read and change settings

```sh
# Snapshot all settings
cranklink --port usb -s <SERIAL> settings read --output settings.json

# Run the write's checks without writing
cranklink --port usb -s <SERIAL> settings set profile1.boost.torque 85 --dry-run

# Write it (saves a pre-write backup, then verifies by readback)
cranklink --port usb -s <SERIAL> settings set profile1.boost.torque 85 --backup before.json

# Roll back
cranklink --port usb -s <SERIAL> settings restore before.json

# Inspect and target a catalogued Di2 component
cranklink settings schema --model ST-6870-L
cranklink --port usb -c ST-6870-L -s <SERIAL> settings read
cranklink --port usb -c ST-6870-L -s <SERIAL> settings set switch-a-function 2 --backup lever-before.json
```

See [settings](docs/settings.md) for protected fields, backups, and batch plans, and [Bluetooth](docs/bluetooth.md) for wireless use, including how cranklink picks a Bluetooth route for each write.

### Flash firmware over Bluetooth

The web app's firmware view and `firmware flash ble-pair` update a working motor's D and M firmware together through the bike's Bluetooth gateway. Bluetooth flashing is experimental and has not been run on hardware yet. Without `--allow-unverified-model` it accepts only EP800 board 1 motors on D 4.1.10, 4.1.11 or 4.2.0. A motor stuck in its bootloader can't be reached over Bluetooth; recover it with a wired SM-PCE02 as below. See [Bluetooth](docs/bluetooth.md#cli) for its options.

### Recover a motor stuck in bootloader mode

Requires an intact bootloader, stable power, and your own firmware containers (`.dat`, not bundled). Only a DU-EP800-CRG (`23/00`, DCASX board 1) has been tested; a plain DU-EP800 reports the same identity and is admitted too. Other families need `--allow-unverified-model`, for dry runs too. Recovery cannot fix hardware damage or an erased bootloader. RX (M) recovery also needs a responding D application.

cranklink checks the image's declared series, unit and board, and the checksum of encrypted `.dat` containers, but cannot prove an image is right for your motor. A wrong image can leave the motor unrecoverable, including by dealer tools. Plaintext images, such as the EP800 M file, carry no checksum, so writing one requires `--expect-sha256` with the SHA-256 of the file you verified.

```sh
# 1. Inspect the containers offline (it prints each file's integrity and SHA-256,
#    the companion's last); compare the printed Source SHA-256 (<D_SHA256> below)
#    with your trusted record of the D file
cranklink firmware inspect DUEP800-D.4.2.0.dat --companion DUEP800-M.4.1.11.dat

# 2. Confirm the bootloader responds and note its Serial (use it as <SERIAL> below)
cranklink --port usb firmware probe --target dcas --recovery

# 3. Dry run (default): validates target and container integrity, writes nothing
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat --expect-sha256 <D_SHA256> --recovery

# 4. Write
cranklink --port usb -s <SERIAL> firmware flash dcas DUEP800-D.4.2.0.dat \
  --companion DUEP800-M.4.1.11.dat --expect-sha256 <D_SHA256> \
  --journal D.journal.jsonl --recovery --write
```

Ordinary recovery compares the file's SHA-256 only when `--expect-sha256` is given (writing a plaintext image requires it) and checks the companion's kind, series and version but not its hash; `--bridge-recovery` pins the stock D 4.2.0.0 and M 4.1.11.0 hashes itself.

For RX, probe with `--target rx`, then run `firmware flash rx` with the M file as input and the D file as companion. M files such as `DUEP800-M.4.1.11.dat` are plaintext, so `--write` also needs `--expect-sha256 <SHA-256>` of the M file you verified (`firmware inspect` prints it and the integrity status, as `Companion SHA-256` and `Companion Integrity` when M is the companion; `--json` has them as `source_sha256` and `integrity`, `md5-verified-container` or `none`, with the companion's under `companion`).

<details>
<summary><strong>If a recovery step fails</strong></summary>

Nothing is erased unless a write has started. Each case below says what cranklink prints and what to do.

- **A write fails after it started:** cranklink prints `CRITICAL: FLASH WRITE INTERRUPTED` with the journal path and the exact recovery command. Do not power-cycle the motor. Bluetooth cannot recover a motor, so after a failed `firmware flash ble-pair` write the printed command is one to run over a wired SM-PCE02.
- **The motor does not enter its bootloader, or does not acknowledge the reset after a refused image:** nothing was erased, but the motor may stay in its DCAS bootloader at the application's slot. A flash journals `boot-entry-failed` for a failed entry. Both cases print a read-only `cranklink probe` to check whether the application still answers and, only if it does not, the `firmware probe --target dcas --recovery --reset` command, with that `--slot`, that returns it to its application. A journal write that fails after entry is reported with the refusal and never skips the reset.
- **An RX (M) probe or dry run:** `firmware probe --target rx` and an RX dry run reset the boot bridge and restart the application after they read the RX bootloader, also when they refuse its reply; a journal write that fails first never skips the reset. If RX update entry fails before a usable reply, RX may stay in update mode with the bridge active, and no reset is sent; a dry run journals `rx-boot-failed`, and a failed bridge reset `rx-reset-failed`. Nothing was erased in either case. Check the application with the read-only `cranklink probe` before retrying, or with `firmware probe --target rx --recovery` after a `--recovery` run, since a recovery RX application cannot report its version.
- **A DCAS dry run or probe without `--recovery`:** these reset the bootloader and succeed only after the application comes back with the same cargo and D/M versions. If it does not come back, the dry run journals `application-return-failed`, and both print the same check-then-reset guidance.
- **The motor is slow to answer after a reset:** after any bootloader reset (a DCAS probe or dry run, the RX probe and dry run, the return after a completed DCAS or RX write, and `firmware probe --target dcas --recovery --reset`), the motor can take a few seconds to answer discovery again. Cranklink repeats discovery up to four times, about 24 s, until the motor answers, and journals the number of discoveries it took as `discoveries`. If no component answers, the error says so. Ordinary probes discover once. `--recovery --reset` succeeds only once a motor answers.
- **A write finished but the application does not answer:** if the transfer finished with its checksum and reset acknowledged but the application does not answer, the write journals `verification-failed` and prints the same read-only `cranklink probe` check, followed, for D, by the `--recovery --reset` command for the bootloader's slot.

</details>

#### Other recovery tools

For the EP800 failure that acknowledges Key 1 but floods empty `0x70` frames before Key 2, follow the [temporary bridge recovery guide](docs/ep800-hardware-recovery.md) using `firmware flash dcas --bridge-recovery`.

For bootloader research on a working, powered EP800-CRG, `firmware dump-bootloader --output NEW_DIR` reads the 64 KiB resident window without flashing. See the [extraction instructions](docs/ep800-hardware-recovery.md#read-a-working-motors-bootloader).

### More options

- `cranklink --help` and `cranklink <command> --help` list all commands and options.
- `--json` gives machine-readable output. With it, a wired DCAS or RX dry run and a `ble-pair` dry run print one JSON object (`"dry_run": true`, the verified target and serial, and `requires_*` flags, each true when the `--write` command must carry that option, such as `--expect-sha256` for a plaintext image) instead of the text report, and `firmware probe` prints its JSON only once its checks pass.
- Environment variables: `CRANKLINK_PORT`, `CRANKLINK_SERIAL` and `CRANKLINK_FORMAT` (`json` or `text`) stand in for `--port`, `-s` and `--format` when those are omitted; `CRANKLINK_PASSKEY` supplies the passkey for `ble:` ports.
- `cranklink --port usb session` serves a read-only JSON-RPC 2.0 driver on stdin and stdout for scripts and agents (wired ports only); see [JSON-RPC session](docs/session.md).

## Terms

- **Drive unit**: the e-bike motor.
- **Modern and legacy motors**: modern motors are the EP600, EP801 and EP500 (including their CRG variants). They use a newer authentication scheme and add the Japan and US Class 3 [market regions](docs/settings.md#market-regions). Legacy motors are the EP800 family, E5100 and older drive units.
- **Market region** (`destination` in the CLI and backups): the sales market the motor is set for, which limits its assist speed.
- **D and M firmware**: a STEPS motor has two processors. D runs on the Shimano communication chip (DCAS) and talks to the E-TUBE bus; M runs on the Renesas motor-control chip (also called RX). Firmware files come in matching D and M pairs.
- **Identity** (for example `23/00`): the series and unit numbers a component reports, used to pick the right settings and firmware checks. `23/00` is the EP800 and EP800-CRG.
- **Gateway**: the part of the bike with Bluetooth (a display such as the SC-EM800, or a wireless unit such as the EW-WU111) that apps connect through.
- **Slot**: a component's address on the E-TUBE wire. It can change after a power cycle.
- **OEM or protected field**: a factory limit that needs motor authentication to change.
- **Master routing**: a Bluetooth mode some motor writes need. Afterwards the bike may need to be switched off and on to return to riding mode.

## Documentation

- [Settings](docs/settings.md): market regions, protected writes, backups, batch plans.
- [Bluetooth](docs/bluetooth.md): connection mode, CLI over a Bluetooth gateway, routing and disconnect behaviour, hardware observations.
- [JSON-RPC session](docs/session.md): `cranklink session` methods, errors and framing for scripts.
- [Understanding E-TUBE](docs/etube-protocol.md): how cranklink discovers parts, routing and the wire reference.
- [EP800 hardware recovery](docs/ep800-hardware-recovery.md): temporary bridge recovery and bootloader extraction.
- [Firmware patcher](docs/ep800-speed-limit-patcher.md): create and check supported motor images offline with `tools/patch_motor_speed.py`.
- [Firmware simulator](docs/firmware-simulator.md): offline firmware tests, `sim.py resident` bootloader regressions, and hash-verified [Di2 firmware acquisition](docs/firmware-simulator.md#di2-research-inputs).
- [EP800 D4.2.0 firmware architecture](docs/firmware-d420-architecture.md): memory layout, flash journal, packet queues and protocol state machines.
- [CLI releases](docs/cli-releases.md): how the rolling and versioned CLI builds are published.
- [Web app development](web-instrument/README.md): build, test, deploy.
- [Android app development](android-app/README.md): build, test, and [Play releases](docs/android-releases.md).

## License

[MIT](LICENSE). Product names identify compatible hardware only. This project is independent of Shimano and is not endorsed by or affiliated with Shimano.
