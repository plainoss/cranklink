# Past PRs and firmware simulator coverage

Reviewed the bodies and actual diffs of PRs 1–11 on 17 October 2026 using read-only GitHub requests. Source line references below refer to each PR's merged revision, not today's working tree. PR #11's initial-start sequence and repeated browser-worker saves were also tested against two pinned source revisions; other historical failures were assessed from their diffs rather than reproduced.

The firmware simulator executes hash-pinned gateway and motor D application functions with initialized RAM. Its native endpoint exercises Cranklink's framing, discovery and read path; a separate modeled GATT peer exercises the BLE client's characteristic/session path. An opt-in exact EP800 D4.2.0 fixture also executes lamp/protected wheel writes and actual motor authorization, with modeled EEPROM commits and boot cache hydration. USB adapter operations, GATT radio delivery/gateway PIN challenge and motor M version responses are modeled. Passing these tests does not validate USB hardware, full UI clicking, physical Bluetooth RF, reset/boot, arbitrary privileged writes or physical EEPROM behavior.

## Main findings

| PR and changed behavior | Evidence in the actual diff | What this simulator can catch | Gap and recommended regression |
| --- | --- | --- | --- |
| [#5: alternate BLE gateways](https://github.com/plainoss/cranklink/pull/5) | `src/ble.rs:466–470` replaces the fixed `[1, 0x1e, 0x23, 2]` identity check with length, opcode and nonempty series/unit checks. Added tests at `1443` and `1464` accept EN600/EN610/EX310/WU101 identities and reject malformed replies before changing role. [Merged code](https://github.com/plainoss/cranklink/blob/9a0ae2d7de6dfcdbf777efe5d352cf6a8bd31eaf/src/ble.rs#L466). | Stock SC-E6100/SC-E7000 gateway execution can expose accidental dependence on SC-EM800 behavior. Stock handshake and cleanup tests detect broken owner-zero setup or normal-mode restoration. | A native endpoint does not call `BleSession::discover_master_units`. The four identities added in this PR have no executable profiles here. Run the real BLE session over the simulated characteristic transport with each supported gateway, and inject malformed `0x33` stock replies; assert no `03/20` role change after rejection. Keep the PR's synthetic alternate-identity tests. |
| [#8: automatic routing and authentication](https://github.com/plainoss/cranklink/pull/8) | `src/cli.rs:1903–1957` plans master routing for selected mutations while reads/dry runs retain normal routing; `1982` suppresses automatic auth on dry run. `src/settings.rs:1167` defines the routing-sensitive setting list. `src/ble.rs` adds `post_auth_master_routing_authenticates_ep800_runs_mixed_destination_speed_batch_and_redacts_trace` and `rejected_destination_write_stops_without_route_retry_or_second_write`. [Routing code](https://github.com/plainoss/cranklink/blob/08f29b748b324152066a9263ea49e5033d2837c2/src/cli.rs#L1903). | Firmware packet tests detect link/key ordering failures and GET response reassembly problems. A BLE transport integration can exercise explicit master setup, discovery, normal reads and cleanup through the actual session implementation. | The general simulator rejects privileged auth/write commands. The exact EP800 D4.2.0 browser fixture now executes stock lamp/protected wheel saves and authorization, but this does not establish destination writes, generalized authorization, rollback, or all no-retry behavior. Retain CLI planner and mocked write tests. Add transport integration for explicit master reads and rejected writes; assert no second write and cleanup. The PR's `preload_restores` change (`src/settings/batch.rs`) also needs file/preflight tests: malformed later restore must fail before opening a transport. |
| [#9: Android USB revision and charging](https://github.com/plainoss/cranklink/pull/9) | `src/native_usb.rs:59` admits revision `0x0A00`; `web-instrument/usb.js` updates its divisor mapping. `src/device.rs:278–303` adds a 90-second charging loop, threshold `2703`, and repeated power-state checks. [Charging code](https://github.com/plainoss/cranklink/blob/660e0bf25ad01ebc27388ec0070480d4984641e5/src/device.rs#L278). | Native tests exercise initialization against an immediately ready modeled adapter. | The endpoint's ready-power response cannot detect either original failure. Add scripted states `04 → low voltage → threshold reached but 04 → 02`, external-supply transition, truncated voltage and timeout. USB descriptor/divisor tests and Android hardware verification remain necessary; a firmware pair cannot validate FTDI configuration or capacitor charging. |
| [#9: coordinated speed/destination UI](https://github.com/plainoss/cranklink/pull/9) | `web-instrument/src/core/sessionController.ts:478–486` selects/stages a compatible destination. Added tests at `instrument.test.ts:235–306` cover revert, model ranges, destination-before-speed ordering and retained staging after rejected save. [Controller code](https://github.com/plainoss/cranklink/blob/660e0bf25ad01ebc27388ec0070480d4984641e5/web-instrument/src/core/sessionController.ts#L478). | A stock read can check parameter opcode/response compatibility. | Neither native firmware execution nor a read-only BLE endpoint drives sliders, staging, batch writes or error-screen state. Keep the controller tests and browser smoke test; add browser assertions that preset and slider use identical model limits and a rejected save preserves both staged values. |
| [#10: simulator sleep bypass and parallel browser tests](https://github.com/plainoss/cranklink/pull/10) | `web-instrument/host.js:52` bypasses sleep when the shared simulator flag is set; `app.js:31–33` writes that flag, with resets on real-device selection/disconnect. `sessionController.ts:262–267` retains the BLE reconnect delay for real devices. `playwright.config.ts:10–11` enables parallel workers; the service-worker reload test becomes independent. [Host code](https://github.com/plainoss/cranklink/blob/0cfb180f576544d75a63a7fbe36f38670ae8679c/web-instrument/host.js#L52). | Deterministic logical bus events let protocol tests avoid wall-clock gateway sleeps. | This cannot detect leaked browser simulator flags, cross-context IndexedDB state, or radio re-advertisement delays. Keep browser tests for simulator → disconnect → real-adapter selection, separate context isolation, service-worker reload, and production assets. Test that the hardware sleep path remains active; logical time cannot establish a safe physical delay. |

## Would it have caught PR #11's Bluetooth issue?

**Yes: the browser-worker test now reproduces the second-save failure before PR #11 and passes after it under an explicit connection-mode lifecycle model.** It also catches the duplicate-start sequence independently. [PR #11](https://github.com/plainoss/cranklink/pull/11) fixes repeated browser saves and duplicate motor-start sequences, retains the authenticated Bluetooth connection, clears rejected PINs, and expands motor-family support. A successful fresh CLI read alone would have missed these regressions. The test does not claim complete UI click coverage or physical RF reproduction.

The concrete duplicate-start change is in [`src/ble.rs:678`](https://github.com/plainoss/cranklink/blob/8cf69780e297bf5106d1aef915f13bef41f709f7/src/ble.rs#L678): initial master setup consumes the receipt already produced by gateway `PCE 0C/01`, rather than issuing another explicit motor `32/10` plus keys. The new `gateway_started_slots` and `master_pc_start_receipts` state preserves those receipts across discovery and inventory.

`verify_pr11.py` built the actual pinned sources with only the local GATT transport injected, then ran the same master-routed CLI probe against stock SC-EM800 4.6.0 and DU-EP800 4.2.0 firmware. It counted outgoing characteristic requests in each trace:

| Source revision | Probe | Gateway `PCE 0C/01` requests | Extra `FREE 32/10 start` requests | Exactly one initial start |
| --- | --- | --- | --- | --- |
| Before: `0cfb180f576544d75a63a7fbe36f38670ae8679c` | Pass | 1 | 1 | Fail |
| Fixed: `158e8f5bc8d5e379c08ee3fa4ca4d59c9af10789` | Pass | 1 | 0 | Pass |

The initial simulator branch started at PR #9; its three representative CLI probes recorded the extra start despite successful reads. The PR branch has since been rebased onto main including PR #11. Historical before/after evidence remains pinned to the source revisions above. The GATT integration report checks reads and error handling; run the separate pinned-revision check for the single-start regression.

The assertion therefore fails before PR #11 and passes after it. Both probes succeed, so ordinary read success would not have detected this difference. This separate probe check establishes the sequence contract; the repeated browser-worker save comparison follows below. The compact [validation report](../tools/firmware-sim/pr11-validation.json) contains counts and source revisions without raw firmware or traces.

Reproduce with locally supplied decoded images:

```sh
python tools/firmware-sim/verify_pr11.py \
  --gateway /absolute/path/SCEM800.4.6.0.decoded.bin \
  --motor /absolute/path/DUEP800-D.4.2.0.decoded.bin \
  --output /tmp/cranklink-pr11-comparison.json
```

The script builds both source versions in a temporary directory and requires the pinned Git objects, Cargo dependencies and simulator Python environment. It retains the versions' protocol/session implementations; GATT radio delivery and authentication challenge boundaries remain modeled.

The broader browser fix adds [`pause_browser_command`/`resume_browser_command`](https://github.com/plainoss/cranklink/blob/8cf69780e297bf5106d1aef915f13bef41f709f7/src/ble.rs#L238), and the CLI retains the session after a successful browser command ([`src/cli.rs:1449`](https://github.com/plainoss/cranklink/blob/8cf69780e297bf5106d1aef915f13bef41f709f7/src/cli.rs#L1449)). The browser-worker reproduction below exercises those paths with real stock motor setting/authentication execution and modeled durable storage. The general native/socket services remain read-only.

PIN recovery is also a separate layer: [`src/ble/browser.rs:68`](https://github.com/plainoss/cranklink/blob/8cf69780e297bf5106d1aef915f13bef41f709f7/src/ble/browser.rs#L68) sends `ble.reset-passkey` after authentication failure. The modeled GATT PIN challenge can test authentication rejection; checking that the next browser connection prompts again requires the browser's actual memory/session-storage path. A fresh CLI process cannot establish it.

The motor-family changes can be checked partly through stock identity/version/settings reads against each executable profile. They do not establish generalized authorization or settings-write compatibility. The feature checks in [simulator-feature-findings.md](simulator-feature-findings.md) cover native read and rejection paths. Cached-PIN browser recovery and physical RF remain separate from the repeated-save reproduction.

### Measured persistent browser-worker reproduction

`verify_browser_pr11.py` builds unchanged Rust/WASM plus each revision's actual `worker.js`, `host.js`, `WebBleGatt` and `BrowserFiles`, then runs them in Chromium. The same explicit master-routed commands save lamp `1 → 2`, or wheel circumference `2150 → 2151`, read the setting and disconnect. The wheel case executes the stock motor's actual regulation challenge/proof/AES path; authorization success is not stubbed. EEPROM commit/ready and selected boot cache hydration are explicit models.

Every case starts with fresh synthetic storage: lamp `0`, wheel `2149`. The runner verifies both saves were actual changes, the stock commit boundary was reached, and each correct backup was already durable in IndexedDB before forwarding its SET. It verifies readback, retained connection/ownership/routing state, cleanup, reconstruction of new machines from the persisted file and a new firmware GET after that controlled power cycle. Earlier exploratory results with reused state were discarded; the final reports assert clean seeds and reject no-op saves.

The two policies separate client behavior from radio assumptions. Always-advertising permits reconnects. Connection-mode-ending makes gateway `PCE 0C/00` end connection mode, preventing reconnect until explicitly re-enabled. This rule comes from PR #11's own E2E change, which sets `radioShutdown` on that exact command and throws `Gateway connection mode ended` on a later connect. It is a declared behavior model, not a newly measured RF event.

Both lamp and protected wheel runs produced the same lifecycle result:

| Revision / radio policy | Save 1 | Save 2 | Connections before explicit disconnect | Premature gateway stops | Value read after power cycle: lamp / wheel |
| --- | --- | --- | --- | --- | --- |
| Before / always-advertising | Pass | Pass | 3 | 3 | `2` / `2151` |
| Before / connection-mode-ending | Pass | Fails reconnect: connection mode ended | 1 | 1 | `1` / `2150` |
| Fixed / always-advertising | Pass | Pass | 1 | 0 | `2` / `2151` |
| Fixed / connection-mode-ending | Pass | Pass | 1 | 0 | `2` / `2151` |

The old version's first save mutates and persists successfully; its second save fails before SET because its first command already stopped the gateway and closed GATT. No unsupported old API error, failed authentication, unsupported slave topology, dirty fixture or automatic retry counts as reproducing the bug. The always-advertising control shows that the old write path itself can succeed while its retained-connection contract fails.

The fixed version keeps one authenticated GATT connection and performs one initial gateway/PnP setup; between commands it releases and freshly reacquires motor ownership. Its final [`ble-disconnect` worker message](https://github.com/plainoss/cranklink/blob/158e8f5bc8d5e379c08ee3fa4ca4d59c9af10789/web-instrument/worker.js) returns `ble-disconnected` with code zero and sends the final gateway stop. The old worker lacks that new API and instead has its existing per-command teardown recorded. All cases confirm final cleanup.

See the sanitized [lamp evidence](../tools/firmware-sim/browser-pr11-lamp-validation.json), [protected wheel evidence](../tools/firmware-sim/browser-pr11-tire-validation.json), and [setup/reproduction commands](firmware-simulator.md#run-the-pinned-browser-comparison). This is a complete repeated-save protocol/browser-worker reproduction under the stated radio model. It does not drive the entire UI, reproduce physical RF, validate PIN-cache recovery, establish general firmware write support, or test a power interruption during commit. The original old UI's slave-routed initial probe is deliberately outside this comparison; a failure at that unsupported topology would be a simulator gap.

## Remaining PRs

| PR | Diff evidence and coverage decision |
| --- | --- |
| [#1: CLI and instrument redesign](https://github.com/plainoss/cranklink/pull/1) | Adds `src/cli/session.rs` request/response types and the line-oriented JSON-RPC reader (`:141`), plus exclusive backup creation in `src/cli.rs`. Simulator-backed CLI probe/read tests can detect command/session integration errors. Add a persistent JSON-RPC open → multiple reads → close test. Backup durability, mutation safety and browser layout require their existing dedicated tests. |
| [#2: documentation](https://github.com/plainoss/cranklink/pull/2) | README adds command examples, including `settings read` and `session --json`. Run harmless examples against the endpoint to detect CLI drift; review descriptive output and unsupported flashing examples separately. |
| [#3: multi-generation flashing](https://github.com/plainoss/cranklink/pull/3) | `src/firmware_io.rs:180` adds the DCAS2 64-KiB page boundary; `:188` chooses indexed versus unindexed packets. `src/rx.rs:240` adds retry-capability gating. Application GET execution cannot validate bootloader erase/page/write semantics or M/RX execution. Keep bootloader transcript tests, including both sides of the page boundary, checksum rejection and capability gating; add executable bootloader profiles only as a distinct future stage. |
| [#4: mobile/desktop UI](https://github.com/plainoss/cranklink/pull/4) | `web-instrument/src/App.tsx` adds responsive navigation and `min-h-[44px]` controls; `index.html:45–46` changes touch targets. Firmware execution cannot detect overflow, accessibility or touch-target regressions. Keep mobile browser checks at the smallest supported viewport. |
| [#6: example serial redaction](https://github.com/plainoss/cranklink/pull/6) | Four README replacements use the illustrative `010203040506` serial (`:75`, `:94`, `:102`, `:124`). No firmware test can enforce documentation redaction. Keep example data synthetic and audit generated traces before committing. |
| [#7: production simulator exclusion](https://github.com/plainoss/cranklink/pull/7) | `web-instrument/app.js:10–11` makes the simulator import conditional on `import.meta.env.DEV`; `:356` rejects simulator use in production. Native simulator availability is independent of browser bundling. Keep the production build/asset test and assertions that simulator controls are absent. |

## Regression priorities

1. Execute real `BleSession` master discovery/read/cleanup over the characteristic transport for every executable gateway family; include malformed stock identity, dropped notification and fragmented reply cases. Label all modeled authentication/radio boundaries.
2. Keep the existing planner/auth/write/restore and browser tests. A passing firmware matrix does not replace them.
3. Add adapter charging scenarios and persistent CLI JSON-RPC reads. Repeated reads must exceed host ring capacity: the simulator now has a 40-read regression that checks a fresh actual gateway response for each request and final link cleanup.

No PR comments, reviews, or changes were posted as part of this investigation.
