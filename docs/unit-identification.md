# Component selection

Connected components are discovered in each session. Bus slots can change after discovery or a power cycle; do not reuse a previously recorded slot without checking the current inventory.

```sh
cranklink units
cranklink --component DU-EP800-CRG probe
```

For BLE, add `--port 'ble:DEVICE_ID'`. `units` lists connected component identities; `probe` reports the selected motor's identity, serial, and firmware versions.

Model names match case-insensitively with optional hyphens. `--component` must resolve to exactly one active component and is accepted by `probe`, `settings read`, and `settings batch`. Only fields supported by the selected model's settings schema are available. Standalone `settings set` and `settings restore` use automatic motor selection or an explicit `--slot` from the current inventory.

If more than one motor is present, use a fresh inventory to select its slot. Bind wired settings operations with `--expect-motor-serial SERIAL`; BLE operations use `--ble-expect-serial SERIAL`. Use the motor's 12-hex-digit serial, not the Bluetooth peripheral ID. A serial mismatch stops the operation.

Drive-unit authentication is restricted to supported motor identities; selecting a display, switch, or shifter does not grant motor settings access. Firmware recovery uses separate bootloader selection and serial checks; follow the [recovery guide](../README.md).
