# Settings (CLI)

`cranklink settings schema --model <model>` lists each family's field names, units, ranges, choices, and OEM flags. `cranklink settings <command> --help` covers options. This page covers what neither shows.

## Protected writes

Fields flagged `oem` require drive-unit authentication, run automatically when `-s <SERIAL>` is given (`--no-auth` disables it). The protocol is chosen from the live motor identity:

| Family | Authentication |
| --- | --- |
| EP800, EP800-CRG, E5100 | Legacy |
| EP600, EP801, EP500 (incl. CRG) | Modern |
| E8000, E8080 | Legacy from D4.7.0, none earlier |
| E7000, E6100, E6110, E6180 | Legacy from D4.4.0, none earlier |
| E6100-CRG, E6180-TRK | Legacy |
| E6000 family | None before D3.4.0; protected writes blocked on later firmware |

Only EP800-CRG settings are validated on hardware. The other families are covered by protocol and offline firmware tests.

## Backups and restore

- Every `set` writes a new backup file (motor serial, original value, planned value) before sending, then verifies by readback. Backup files are never overwritten.
- `restore` takes such a backup, checks the serial, and writes a new backup of the current value first.
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

1–128 operations run in order on one connection; relative `file` paths resolve from the plan's directory. Each write gets its own backup. A batch is not atomic: on failure, earlier writes stay applied; restore them from their backups.

## Constraints

- Speed and maximum power are validated against the destination (region). An unset EP800 destination blocks speed changes; set a destination first. The web app stages a compatible destination automatically.
- An unset maintenance calendar must be initialized with `maintenance-date YYYY-MM-DD` before editing year, month, or day individually.
- Bus slots can change after a power cycle. Select a motor with `-c <MODEL>` or a slot from a fresh `cranklink units`.
