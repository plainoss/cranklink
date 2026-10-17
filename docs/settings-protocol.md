# Settings

The same named settings interface is available over USB/serial and optional BLE. Read all schema-supported motor settings, change supported values, restore individual backups, or run ordered batches. Availability and write permissions depend on the motor, firmware, destination, and installed drivetrain.

## Feature coverage

| Model | Settings |
|---|---|
| DU-EP800 / EP800-CRG | Lighting and walk assist; wheel circumference and mounting angle; destination and speed; assist profile selection, names, labels, torque, character, and start; OEM torque bounds and mode power; drivetrain/gearing and automatic shifting; display speed adjustment; maintenance alert/date/distance. |
| DU-EP600 / EP801 | Basic assist profiles, names and labels; lighting and walk assist; wheel/angle; destination/speed and display adjustment; maintenance. Hardware validation is pending. Fine-tune profiles, free-shift/modern automatic shifting, OEM light-output/power tuning, and OEM torque-bound writes are unsupported. |

Use the schema for exact names, units, ranges, choices, nullable values, and OEM flags:

```sh
cranklink settings schema --model ep800-crg
cranklink settings schema --model ep600
cranklink settings schema --model ep801
```

Schema output describes implemented fields; it does not guarantee every field is readable or writable on a particular motor. Reads preserve successful values and report unavailable fields separately. There is no unrestricted raw settings writer.

## Read, set, and restore

Wired example; `010203040506` is illustrative. Replace it with the actual motor serial from `cranklink probe`:

```sh
cranklink -s 010203040506 settings read --output settings.json
cranklink -s 010203040506 settings read profile1.eco.character
cranklink -s 010203040506 settings set profile1.eco.character 4 --backup eco-before.json
cranklink -s 010203040506 settings restore eco-before.json --backup eco-current.json
```

For BLE, use `--port 'ble:DEVICE_ID' -s SERIAL`; see [Bluetooth settings](bluetooth.md). Writing or restoring `oem`-protected settings (direct `set`, standalone `restore`, or `batch`) automatically runs drive-unit authentication when `-s` / `--expect-serial` is provided (override with `--unit-auth` or `--no-auth`; suppressed during `--dry-run`). Over BLE, settings that require PC-link owner zero (`destination`, `tire-circumference`, `lamp`, `lighting-time`) also automatically select BLE master routing (`--ble-master[=auto|normal|master]`, default `auto`). Authentication support is model/version constrained and does not override unsupported settings or device permissions.

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

Speed and maximum power are checked against the motor's destination and model limits. EP800 does not expose an independent OEM speed-ceiling setting. An unset EP800 destination stops speed/power validation; select an explicit supported destination with the required authentication first. EP600/EP801 can use the factory destination when the OEM destination is unset; missing or malformed values stop the write. The device may impose further limits.

In the browser, speed sliders and presets share the full model-supported range. Selecting a speed automatically stages a compatible destination if needed; the save review shows both changes, and the batch writes destination before speed. Selecting a destination explicitly adjusts the staged speed to its allowed range. Failed saves retain staged changes and display the transaction error in the settings screen.

Unset maintenance date/distance reads as JSON `null`. `maintenance-date` accepts `YYYY-MM-DD` in 2000–2099 or `unset`; `maintenance-distance` also accepts `unset`. Initialize an unset calendar with `maintenance-date` before editing individual year/month/day fields.
