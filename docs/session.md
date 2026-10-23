# JSON-RPC session

`cranklink session` reads JSON-RPC 2.0 requests from stdin and writes one response per request to stdout, so a script or agent can keep one wired adapter open across several calls. It is read-only: it connects, discovers and probes components, and inspects settings schemas and firmware files offline. It cannot read or write settings, flash firmware or talk to a bootloader; use the ordinary commands for those.

```sh
cranklink --port usb session
cranklink --port usb -s <SERIAL> --trace session.wire.jsonl session
```

- Wired ports only: `usb`, a serial port, `auto` or a `sim:` socket. `--port ble:…` is refused before the session starts, and `session.connect` to a Bluetooth port returns an error.
- `--port` (or `CRANKLINK_PORT`) sets the initial port, `auto` when omitted. `-s` (or `CRANKLINK_SERIAL`) sets the serial every `probe` must report. `--trace` appends the wire frames of each connection.
- Output is always JSON-RPC: `--json` and `--format` have no effect, and neither has `--slot` (`probe` takes its own). `--component` is rejected. `--read-only` is accepted.
- Progress notices, such as adapter charging, go to stderr.

## Framing

Before reading any input, the session prints an `event.ready` notification with the cranklink version and the initial port:

```json
{"jsonrpc":"2.0","method":"event.ready","params":{"default_port":"usb","version":"0.1.0"}}
```

Each request is one line holding a JSON object with a string `method`, optional `params` and optional `id`; `jsonrpc`, if present, must be a string, but its value is not checked. Blank lines are skipped. Every other line gets exactly one response line, in order, echoing its `id`. A request without `id` still gets a response, which then has no `id` field. A line that is not valid UTF-8, or not a JSON object with a string `method`, gets a `PARSE_ERROR` with `"id": null`, and the session reads the next line. Input bytes are never repaired, so a damaged port, path or name is never used.

`exit`, `quit` or the end of input ends the session. Any PC-link still open is stopped first, without reporting a failure.

## Methods

| Method (alias) | Params | Result |
| --- | --- | --- |
| `session.connect` (`connect`) | `port`, `expect_serial` (strings, optional) | `{"connected": true, "port", "power": {"adapter_supply", "raw_state"}}` |
| `session.disconnect` (`disconnect`) | none | `{"connected": false, "stopped"}` |
| `session.status` (`status`) | none | `{"connected", "port", "expect_serial", "identity"}` |
| `probe` | `slot` (integer 0–31), `component` (model name); both optional | `{"slot", "cargo", "dcas_version", "rx_version", "serial", "model_series", "model_unit"}` |
| `units` | none | `{"units": [...]}`: stock identity and resolved model per slot |
| `settings.schema` | `model` (default `ep800-crg`), `name` (optional) | All of the model's setting specs, or the one named (case-insensitive) |
| `firmware.inspect` | `file` (required), `companion` (optional) | The container's metadata: kind, versions, series/unit, board, sizes, SHA-256s, checksum and `integrity` (`md5-verified-container` or `none`), as `firmware inspect --json` prints it; with `companion`, the same for it under `companion` |
| `exit` (`quit`) | none | `{"exited": true}` |

- `session.connect` stops and closes any open connection, then opens and initializes the adapter, waiting up to 90 s if it needs charging. A `port` given here is kept for later calls, even if opening it fails. An invalid `expect_serial` is an `INVALID_PARAMS` error and changes nothing; a valid one replaces the serial from `-s`. Non-string values of either are ignored.
- `session.disconnect` closes the connection. `stopped` is false when nothing was open or its PC-link stop was not acknowledged.
- `session.status` reports the identity from the last successful `probe` since the last connect or disconnect, or null.
- `probe` connects first if needed and stays connected. It runs discovery, starts a PC-link at the selected unit, reads its identity and serial, and stops the PC-link again before answering; a stop that is not acknowledged fails the probe. Without a selector it probes the only motor; `slot` or `component` selects a catalogued component as `cranklink probe --slot`/`-c` do. `null` counts as absent; any other `slot` that is not an integer 0–31, or a `component` that is not a string, is an `INVALID_PARAMS` error, returned before the port is opened. When a serial is expected and the probed one differs, the result is a `SERIAL_MISMATCH` error whose `data` holds `expected` and `actual`. A probe with a `slot` or `component` can read identities before its PC-link starts, so after an earlier discovery on the connection it first initializes the adapter again, as `units` does; a probe without a selector does not.
- `units` runs discovery and reads each slot's identity without starting a PC-link. It needs an open connection, from `session.connect` or an earlier `probe`. If the connection has already run discovery (an earlier `units` or `probe`), `units` first resets and initializes the adapter again, as `session.connect` does, so it adds the same waits, about 5 s, or up to 90 s when the adapter needs charging. Without this, a tested EP800-CRG left its model read unanswered in a second inventory on one connection.
- `settings.schema` and `firmware.inspect` work offline, with or without a connection. A `companion` is checked against `file` first, and a mismatch is an error. Paths are relative to the session's working directory.

If a PC-link stop is not acknowledged, the connection refuses further discovery and probes until `session.connect` reopens the port.

## Errors

An error response carries `code`, `name`, `message` (the full error chain), and optionally `data` and `hint`. `name` and `hint` are cranklink additions to JSON-RPC 2.0.

| Code | Name | Returned for |
| --- | --- | --- |
| -32700 | `PARSE_ERROR` | A line that is not UTF-8 or not a JSON request; `id` is null |
| -32601 | `METHOD_NOT_FOUND` | An unknown method |
| -32602 | `INVALID_PARAMS` | A bad `probe` selector, an invalid `expect_serial`, an unknown schema model or a missing `file` |
| 4000 | `HARDWARE_ERROR` | Any other connect, probe, units or inspect failure |
| 4001 | `NOT_CONNECTED` | `units` without an open connection |
| 4002 | `SERIAL_MISMATCH` | A probed serial that differs from the expected one |
| 4003 | `AUTH_FAILED` | An error whose message mentions authentication or a challenge |
| 4004 | `VALUE_OUT_OF_RANGE` | An error whose message mentions a range or bounds |
| 4005 | `UNKNOWN_SETTING` | A `settings.schema` `name` the model does not have |
| 4006 | `TIMEOUT` | An error whose message mentions a timeout |

Only `PARSE_ERROR`, `METHOD_NOT_FOUND`, `INVALID_PARAMS`, `NOT_CONNECTED`, `UNKNOWN_SETTING` and the probe's own serial check are exact. Every other failure is classified by matching words in its message, checked in the order 4002, 4003, 4004, 4006 and falling back to 4000, so treat those codes as hints and read `message`.

## Example

Requests (`>`) and responses (`<`) for an EP800-CRG at slot 13:

```text
< {"jsonrpc":"2.0","method":"event.ready","params":{"default_port":"usb","version":"0.1.0"}}
> {"jsonrpc":"2.0","id":1,"method":"session.connect"}
< {"jsonrpc":"2.0","id":1,"result":{"connected":true,"port":"usb","power":{"adapter_supply":false,"raw_state":1}}}
> {"jsonrpc":"2.0","id":2,"method":"probe"}
< {"jsonrpc":"2.0","id":2,"result":{"cargo":"00-CRG","dcas_version":"4.2.0.0","model_series":35,"model_unit":0,"rx_version":"4.1.11.0","serial":"010203040506","slot":13}}
> {"jsonrpc":"2.0","id":3,"method":"exit"}
< {"jsonrpc":"2.0","id":3,"result":{"exited":true}}
```
