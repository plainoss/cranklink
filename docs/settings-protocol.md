# Settings

The same named settings interface is available over USB/serial and optional BLE. Read all schema-supported motor settings, change supported values, restore individual backups, or run ordered batches. Availability and write permissions depend on the motor, firmware, destination, and installed drivetrain.

## Feature coverage

| Model | Settings |
|---|---|
| DU-EP800 / EP800-CRG | Lighting and walk assist; wheel circumference and mounting angle; destination and speed; assist profile selection, names, labels, torque, character, and start; OEM torque bounds and mode power; drivetrain/gearing and automatic shifting; display speed adjustment; maintenance alert/date/distance. |
| DU-EP600 / EP600-CRG / EP801 / EP801-CRG / EP500 | Basic assist profiles, names and labels; lighting and walk assist; wheel/angle; destination/speed and display adjustment; maintenance. Hardware validation is pending. Fine-tune profiles, free-shift/modern automatic shifting, OEM light-output/power tuning, and OEM torque-bound writes are unsupported. |
| DU-E5100 | Lighting and walk assist; wheel circumference and mounting angle; destination/speed and display adjustment; maintenance. No profiles or drivetrain tuning. |
| DU-E8000 / E8080 / E7000 / E6100 family | Wheel circumference (1300–3000 mm), mounting angle (0–60°), lamp off/on. |
| DU-E6000 family | Wheel circumference (2000–2400 mm), mounting angle (0–45°), lamp off/on. Conservative ranges shared across variants. |

Use the schema for exact names, units, ranges, choices, nullable values, and OEM flags:

```sh
cranklink settings schema --model ep800-crg
cranklink settings schema --model ep600
cranklink settings schema --model ep801
cranklink settings schema --model ep500
cranklink settings schema --model e5100
cranklink settings schema --model e8000
cranklink settings schema --model e7000
cranklink settings schema --model e6100
cranklink settings schema --model e6000
```

Schema output describes implemented fields; it does not guarantee every field is readable or writable on a particular motor. Reads preserve successful values and report unavailable fields separately. There is no unrestricted raw settings writer.

## Compatibility and validation

| Motor family | Settings schema | Protected-write authorization (USB/serial and BLE) |
|---|---|---|
| EP800, EP800-CRG | Legacy profiles and settings | Legacy |
| EP600, EP600-CRG | Modern basic profiles and settings | Modern |
| EP801, EP801-CRG | Modern basic profiles and settings | Modern |
| EP500 | Modern basic profiles and settings | Modern |
| E5100 | Common settings, no profiles | Legacy |
| E8000, E8080 | Wheel, angle, lamp | Legacy from D4.7.0; none earlier |
| E7000 | Wheel, angle, lamp | Legacy from D4.4.0; none earlier |
| E6100, E6110, E6180 | Wheel, angle, lamp | Legacy from D4.4.0; none earlier |
| E6100-CRG, E6180-TRK | Wheel, angle, lamp | Legacy |
| E6000, E6001, E6002, E6010, E6012, E6050 | Wheel, angle, lamp | None before D3.4.0; protected writes blocked on later firmware because stock identity does not distinguish authorization requirements |

Selection uses the live stock identity and known product variant, not an exact firmware-patch list. Older families retain the firmware thresholds where authorization was introduced; no keys are sent on versions that do not require it. Standard and cargo variants share their family's field encodings. This permits other firmware versions to use established protocols; it does not claim that every version or setting has been tested. Older firmware may lack individual fields. Unsupported reads are reported separately, and writes still require valid values, device permission, an original-value backup, a positive reply, and readback. Failed authorization is never retried with another protocol.

Physical settings validation covers EP800-CRG, including repeated BLE wheel-size changes on D4.1.11.0. Modern authorization has offline coverage on EP600-family D4.6.0.1 and EP500 D4.1.1.0, with additional EP600 D4.4.1.0 key-derivation coverage. EP801 and other firmware combinations use shared family behavior and remain hardware-unvalidated. Modern wired exchanges have protocol tests, not physical-device validation. E5100 legacy key generation has offline coverage on D4.1.0. Older E-series subsets use shared command layouts and conservative model ranges; no physical validation has been performed for these additions. Firmware flashing has a separate compatibility policy.

## Read, set, and restore

Full `settings read` JSON snapshots include an `identity` object with motor slot, cargo, firmware versions, model identifiers, and the serial read in the same connection. If that serial read fails, `identity.serial` is null; the browser refuses to establish a session without a verified serial. Single-setting reads retain their existing result shape.

Wired example; `010203040506` is illustrative. Replace it with the actual motor serial from `cranklink probe`:

```sh
cranklink -s 010203040506 settings read --output settings.json
cranklink -s 010203040506 settings read profile1.eco.character
cranklink -s 010203040506 settings set profile1.eco.character 4 --backup eco-before.json
cranklink -s 010203040506 settings restore eco-before.json --backup eco-current.json
```

For BLE, use `--port 'ble:DEVICE_ID' -s SERIAL`; see [Bluetooth settings](bluetooth.md). Writing or restoring `oem`-protected settings (direct `set`, standalone `restore`, or `batch`) automatically runs drive-unit authentication when `-s` / `--expect-serial` is provided (override with `--unit-auth` or `--no-auth`; suppressed during `--dry-run`). Over BLE, settings that require PC-link owner zero (`destination`, `tire-circumference`, `lamp`, `lighting-time`) also automatically select BLE master routing (`--ble-master[=auto|normal|master]`, default `auto`). Authentication is selected by the supported motor family and does not override unsupported settings or device permissions.

Each write saves the motor serial, original value, and planned value to a new durable backup before sending changes, preserves sibling fields, and verifies readback. Backup files cannot be overwritten. `restore` checks the backup format and serial, restores its named setting, and requires a new backup for the current value. Unrelated settings remain unchanged; maintenance calendar restores retain the complete captured date.

A full `settings read --output` export is a snapshot, not a `settings restore` input. Restore accepts per-setting write backups. A successful readback does not prove power-cycle persistence; verify after power cycling the bike. A partially completed multi-command write is reported and its original value remains in the backup.

## Batch operations

Save this as `plan.json`:

```json
[
  {"op": "read"},
  {"op": "set", "name": "profile1.eco.character", "value": 4},
  {"op": "read", "name": "profile1.eco.character"}
]
```

```sh
cranklink --expect-motor-serial 010203040506 settings batch plan.json --backup-prefix run-01
```

Plans contain 1–128 ordered read/set/restore operations. A restore operation has the form `{"op":"restore","file":"eco-before.json"}`; relative restore paths resolve from the plan's directory. Each write gets its own backup and verified readback; the new prefix creates a journal and numbered backups. Later operations see earlier changes. A batch is not atomic: failure can leave earlier successful writes applied. Use their backups to restore them.

## Values and limits

Use the schema's input units: speed in km/h, cadence in rpm, torque in Nm, light output in hundredths of a watt, and display adjustment in tenths of a percent. Assist character/start are numeric indices; ranges differ between model and mode. Profile names accept at most ten printable ASCII characters, subject to firmware support.

Speed and maximum power are checked against the motor's destination and model limits. EP800 does not expose an independent OEM speed-ceiling setting. An unset EP800 destination stops speed/power validation; select an explicit supported destination with the required authentication first. EP600/EP801/EP500 can use the factory destination when the OEM destination is unset; missing or malformed values stop the write. The device may impose further limits.

In the browser, speed sliders and presets share the full model-supported range. Selecting a speed automatically stages a compatible destination if needed; the save review shows both changes, and the batch writes destination before speed. Selecting a destination explicitly adjusts the staged speed to its allowed range. Failed saves retain staged changes and display the transaction error in the settings screen.

Unset maintenance date/distance reads as JSON `null`. `maintenance-date` accepts `YYYY-MM-DD` in 2000–2099 or `unset`; `maintenance-distance` also accepts `unset`. Initialize an unset calendar with `maintenance-date` before editing individual year/month/day fields.
