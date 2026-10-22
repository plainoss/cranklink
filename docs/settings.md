# Settings

`cranklink settings schema --model <model>` lists each family's field names, units, ranges, choices, OEM flags, and read-only fields. Models can be motor family aliases (`ep800-crg`, `ep600`, etc.) or catalogued component names such as `ST-6870-L`. Unknown model names are rejected. `cranklink settings <command> --help` covers options.

## Component settings

Use `-c <MODEL>` / `--component <MODEL>` with `probe` or `settings read`, `set`, `restore`, and `batch`. A slot from a fresh `units` inventory also works. When both selectors are supplied they must match the same connected component. Without an explicit selector the CLI retains its drive-unit selection behavior. As for drive units, `set` and batches containing a `set` require `-s <SERIAL>` (from `probe`); sibling components such as left and right levers share a schema, so a slot or model name alone does not identify the unit.

Component sessions use the stock identity and, where necessary, a part discriminator to select an exact catalog model. They read the component application version without issuing motor cargo/RX queries. Unknown or unresolved identities are rejected. A catalog entry does not automatically grant a settings layout: components without an implemented schema return no fields and `capabilities.settings_available: false` in full snapshots.

| Components | Fields |
| --- | --- |
| Classic wired ST/SW Di2 controls, including 6770, 6870, 9070, R785, R8050/R8070, R9150/R9170 and RX815 variants | `switch-a-function`, `switch-b-function`; extended C/S wire channels are read-only |
| Classic rear derailleurs, including 6770, 6870, 9070, M8050/M9050, R8050/R9150 and RX805/RX815/RX817 | `multi-shift-speed`; current and maximum derailleur adjustment are read-only |
| Classic front derailleurs, including 6770, 6870, 9070, M8070/M9050/M9070, R8050/R9150 and RX815 | `synchro-shift-interval` where supported; current and maximum derailleur adjustment are read-only |
| SM-BMR1, SM-BMR2, BM-DN100, BT-DN110 power units | `multi-shift-limit.up`, `multi-shift-limit.down` (2, 3, or 255 for unlimited) |
| MU-S705, MU-UR500, MU-UR510, MU-UR520 hub actuators | `hub-adjustment` (−4…4, live maximum must be 4); `hub-gear-count` on S705 (8/11) and UR500/UR510 (5/8/11) |
| SC-M9050/M9051, SC-E8000/E6100/E7000, SC-EM800, SC-EN500/EN600/EN610 cycle computers | Model-specific `display-beep`, `display-distance-unit`, `display-backlight`, `display-language`, and `display-brightness` |

The schema is authoritative for exact model variants. Modern wireless levers and other components without a verified layout remain without named settings. Firmware may reject an individual service; a full read reports that field under `unavailable`. These layouts are covered by protocol tests, not hardware validation.

Switch A/B values are wire channels: 0 front up, 1 front down, 2 rear up, 3 rear down; SW-S705 accepts only 2/3. Writes preserve the sibling nibble and meaningful auxiliary channels, while reserved SET bytes stay zero instead of copying GET padding. Modern event-function extensions remain outside these classic schemas. Component edits also reject current values outside their writable schema so every accepted change has a restorable backup. Rear multi-shift speed is 0 off, 1 very slow, 2 slow, 3 normal, 4 fast, 5 very fast, 6 super fast. Front synchro interval is 0 normal, 1 slow, 2 very slow. Display language is 0 English, 1 French, 2 German, 3 Dutch, 4 Italian, 5 Spanish; beep is 0 off/1 on; distance unit is 0 kilometres/1 miles; backlight is 0 off/1 on/2 manual; brightness is 20/40/60/80/100 percent.

```sh
cranklink settings schema --model ST-6870-L
cranklink --port usb -c ST-6870-L -s <SERIAL> settings read
cranklink --port usb -c ST-6870-L -s <SERIAL> settings set switch-a-function 2 --backup lever-before.json
cranklink --port usb -c ST-6870-L -s <SERIAL> settings restore lever-before.json --backup lever-current.json
```

The web app uses the same Rust schemas. After connecting, select the target in **Active component**; discovery lists the connected models and slots. Refresh the inventory after changing the bus. Component selection reads a fresh identity and settings snapshot before displaying controls. Save or discard staged edits before switching targets. Read-only and unavailable fields cannot be staged, and catalogued models without a settings layout display an explicit unavailable message. Motor tuning and firmware actions appear only for drive units.

Browser saves create durable local backups before each SET and require verified receipts. The Backups page offers restore only for the selected model and serial, and only while no edits are staged; switching components retains other backups for download. Full configuration snapshots are downloadable records; restore uses the setting backups created before writes. Batches run in order (apart from the region-change ordering under [Batch plans](#batch-plans)) and can partially apply on failure, so keep their backups for recovery. Restoring a save restores all of its backups in one batch, so a region change is undone together with its speed or power limit. WebMCP exposes inventory and component selection through `cranklink_list_components` and `cranklink_select_component`, with the same target and staging guards.

Signed values can be supplied directly, for example `settings set hub-adjustment -3 --backup hub-before.json`. Hub adjustment can acknowledge acceptance before its actuator reaches the target. Cranklink sends one SET and polls GET for a three-second completion window, checking the live maximum on every reply. Individual transport calls retain their own timeouts. Setting the current value still sends one SET so it can cancel a different pending target; the receipt's `changed` field remains false. An unfinished adjustment reports an uncertain outcome and retains its backup; it is never automatically retried.

Stock-firmware simulator checks cover exact MU-UR500 and cycle-computer images, including CLI reads, writes, backups and restores. Browser tests additionally drive the current UI and WASM through SC-EM800 4.6.0 and MU-UR500 3.3.1 save, power-cycle, readback and restore flows, and an unfinished hub adjustment. See [simulator coverage](firmware-simulator.md) for executable versions and modeled storage/actuator boundaries. Catalog recognition alone does not imply firmware execution coverage.

## Protected writes

Fields flagged `oem` require drive-unit authentication, run automatically when `-s <SERIAL>` is given (`--no-auth` disables it). The protocol is chosen from the live motor identity:

| Family | Authentication |
| --- | --- |
| EP800, EP800-CRG, E5100 | Legacy |
| EP600, EP600-CRG, EP801, EP801-CRG, EP500 | Modern |
| E8000, E8080 | Legacy from D4.7.0, none earlier |
| E7000, E6100, E6110, E6180 | Legacy from D4.4.0, none earlier |
| E6100-CRG, E6180-TRK | Legacy |
| E6000 family | None before D3.4.0; protected writes blocked on later firmware |

Only EP800-CRG settings are validated on hardware. Other families have protocol-test coverage; firmware execution is limited to the exact profiles and operations listed in [simulator coverage](firmware-simulator.md).

## Backups and restore

- `set` and batches containing a `set` require `-s <SERIAL>`, checked before connecting; `--dry-run`, reads and restores do not.
- `set --dry-run` reads the device and runs the same checks as the write (destination limits, OEM torque bounds, drivetrain, calendar), then reports the current value and, for numeric fields, the `planned_parameters` the SET would send and whether it would change the value (`changed`). It takes no backup, sends no SET and skips unit authentication, but refuses `--no-auth` for a protected setting as the write does.
- Every `set` writes a new backup file (component serial, original value, planned value) before sending, then verifies by readback. Component backups also record the settings model. Backup files are never overwritten.
- Each write reads the serial for its backup right before the SET and refuses, with no backup and no SET, if it differs from the serial verified when the session started. In a batch, earlier writes stay applied.
- If the PC-link stop or Bluetooth disconnect fails after a command completed, its receipt (including the backup path) is still printed before the error, and the command exits with an error.
- Destination, assist-profile, profile-display and maximum-power replies must echo the requested record selector; a reply for another record is refused before the backup, and the SET always names the requested record.
- `restore` takes such a backup, checks the serial, and writes a new backup of the current value first.
- Component restores also require the same catalog model in the backup; shared fields changed since the backup are preserved. Existing motor backups remain compatible.
- A `settings read --output` snapshot is not a restore input.
- Readback does not prove persistence. Power-cycle the bike and read again.

## Batch plans

```json
[
  {"op": "read"},
  {"op": "set", "name": "profile1.eco.character", "value": 4},
  {"op": "restore", "file": "eco-before.json"}
]
```

```sh
cranklink -s <SERIAL> settings batch plan.json --backup-prefix run-01
```

1–128 operations run in order on one connection; relative `file` paths resolve from the plan's directory. One exception keeps region changes valid: when a destination write (or restore) comes first, a later `speed-limit` or `max-power.*` write of the same change that fits both the current and the new destination runs before it. It never moves past a read, another destination write or an earlier write of the same setting; results and backup numbers keep plan order, and the journal's `order` lists the sequence that ran. In a plan with a destination write, a step that would still fall outside its destination is refused before any write. Each write gets its own backup. A batch is not atomic: on failure, earlier writes stay applied; restore them from their backups.

## Constraints

- Speed and maximum power are validated against the destination (region). An unset EP800 destination blocks speed changes; set a destination first. The web app stages a compatible destination automatically.
- A destination write or restore is refused while the current speed limit or maximum power is outside the new destination's range (for example 32 km/h before destination 0, 15 km/h before destination 1, or more than 400 W before destination 3). Change it first, or put the destination and the new limit in one batch, destination first. When neither order fits, as from 15 km/h in destination 0 to 30 km/h in destination 1, the batch is refused before any write; save a speed both destinations permit (19–25 km/h here) first.
- On EP600, EP801 and EP500, per-mode torque must be one of the vendor levels for the motor's OEM torque bounds. Only the 20–85 and 20–50 Nm tables are verified; torque writes are refused on motors with other bounds.
- An unset maintenance calendar must be initialized with `maintenance-date YYYY-MM-DD` before editing year, month, or day individually.
- Bus slots can change after a power cycle. Select a component with `-c <MODEL>` or a slot from a fresh `cranklink units`.
