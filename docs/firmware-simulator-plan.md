# Offline gateway and motor firmware simulator

Build a headless, deterministic firmware co-simulator alongside the browser GATT simulator. Execute stock gateway and motor instructions in separate machines and connect their bus boundaries through a virtual E-TUBE network. Start with packet delivery and expand toward peripheral-driven execution as each boundary is understood.

The first proof of concept **runs successfully** with SC-EM800 4.6.0 and EP800 D4.1.10. It connects the gateway's real PC-link sequence and FREE-command forwarding to the motor's real handshake handlers, dispatcher, response scheduler and reply builder. Motor-produced replies pass through the gateway's real FIFO reassembly and host-response builder. This validates function-level packet co-simulation. Booting complete firmware and reproducing bus timing remain later work.

## Implementation status

The [implementation guide](firmware-simulator.md) documents the reusable packet engine, native socket endpoint, current multi-model coverage and remaining boundaries. The original POC below is retained as the initial feasibility check. A modeled GATT endpoint now exercises the native Bluetooth client and stock SC-EM800 control handlers. The browser-worker regression now executes real EP800 D4.2.0 D8/E0 authentication and lamp/wheel setters with a modeled EEPROM commit boundary and restart hydration. Its unchanged pre/fixed PR #11 browser workers fail/pass the retained-session repeated-save scenario under an explicit connection-mode model. Full boot, physical radio, general storage layouts and interrupted-commit fidelity remain incomplete; stage 3 is only partially covered.

## Validated implementation

The initial two-image POC is superseded by the modular [simulator](../tools/firmware-sim/sim.py). Reproducible commands, supported versions and boundaries are in the [implementation guide](firmware-simulator.md); the [compatibility summary](../tools/firmware-sim/compatibility.json) records the verified cross-product. Raw packet traces and firmware inputs remain local run artifacts.

## Target architecture

```text
Cranklink CLI / Rust engine / browser worker
              |
   simulated PCE or GATT transport endpoint
              |
    gateway firmware machine -- optional radio firmware machine
              |
    deterministic virtual E-TUBE bus
              |
      motor D firmware machine
              |
    modeled internal D/M interface
              |
    optional motor M/Renesas RX machine
```

The packet boundary is the first useful integration point. USB adapter framing in `src/protocol.rs` is a host-to-adapter protocol; it must not be assumed to describe the physical E-TUBE bus. Similarly, the POC's FIFO headers cover only the observed six-byte case, not a general bus codec.

Each machine owns CPU context, RAM, flash, peripherals and local queues. A coordinator owns logical time and a stable event queue ordered by `(deadline, sequence)`. A bus event carries sender, destination/broadcast, payload, delivery time and fault metadata. Callback observation enqueues events; it never recursively runs another machine. Endpoint validation rejects impossible lengths and unknown destinations. CPU instruction counts are execution bounds, not a clock calibration.

Firmware profiles must pin decoded image hash, load segments, entry points, relevant globals, calling conventions and expected instruction bytes. Keep original firmware immutable and store experimental modifications separately, with original and modified hashes. Unknown images must fail before applying recovered addresses. Synthetic state, emulated peripherals, observed firmware bytes and inferred behavior must be separately visible in reports.

## Implementation stages and acceptance gates

### 1. Reusable packet simulator

Extract the POC into machine, firmware-profile, bus, scenario and trace modules. Retain the exact working profiles before adding EP800 D4.2.0 or SC-EM800 4.4.0. Replace the special six-byte adapter with a recovered framing implementation covering real fragment limits, sequence rules and padding. Let firmware generate PnP announcements and populate gateway inventory rather than supplying a connected-port assumption. Add versioned synthetic NVM fixtures.

**Gate:** repeated runs produce identical traces; PnP, PC-link start/stop, identity and several settings GETs traverse both firmwares. Drop, duplication, delay, reordering, malformed fragments and old-slot reuse have explicit outcomes. Incomplete replies never become successful transactions. Recovery from faults must include bounded cleanup and observable unresolved state.

### 2. Real Cranklink client integration

Expose the headless coordinator through a local request/reply protocol. Add a native `Port` implementation at the `src/platform.rs` boundary so real `Device` code can use it. The simulator endpoint must implement adapter commands and host framing, not just unit messages. For BLE, add a transport matching the existing notification/write contract, initially modeling the radio-facing endpoint. Retain the current browser simulator for fast UI development.

**Gate:** unmodified probe/settings flows run through Cranklink against firmware-generated replies. Existing serial/model/auth gates, backup generation, readback and cleanup are exercised. Missing responses reach real client timeout paths. New CLI options and browser controls get README and relevant protocol documentation updates.

### 3. Ownership, authentication and persistent settings

Extend scenarios through actual D8/E0 authentication, rejection replies, owner changes and session expiry. Initial POC authentication state is zero; PC-link completion does not imply privileged motor authentication. Use known synthetic serial/NVM fixtures and preserve firmware-generated challenges and receipts. Implement storage semantics before claiming successful writes: erase/program constraints, busy/error states, restart and interrupted commit. Treat sensor and battery inputs as modeled environmental inputs.

**Gate:** correct proof succeeds and mutated proof fails; source/owner mismatch and cycle-computer collision reject; successful settings changes survive simulated restart; dropped acknowledgements do not trigger blind write replay. Validate representative traces against existing hardware receipts, including SC-EM800 4.4.0 / EP800 D4.1.10, before claiming hardware fidelity for that combination.

### 4. Continuous execution and peripheral models

First identify exact MCU variants, flash layout, reset vectors, image load offsets, missing bootloader regions and clocks. Run bounded reset probes that record the first missing dependency. Model only documented/observed MMIO with strict register accesses, timers, interrupts, watchdog, DMA/FIFOs and bus state. Replace direct handler calls incrementally with peripheral-driven receive and transmit paths. Record all remaining function boundaries and synthetic initialization.

**Gate:** both gateway and motor D reach their communication loops from a reproducible boot state; firmware-driven discovery, handshake and reads work without direct handler injection. Unsupported peripheral accesses fail with PC/address diagnostics. Interrupt timing and watchdog expiry pass deterministic scenarios. Merely mapping large peripheral ranges is insufficient.

### 5. Additional processors and physical bus fidelity

The gateway application, Nordic radio application/stack, motor D and motor M/Renesas RX are separate targets. The present harness executes only gateway application and motor D instructions. Evaluate an RX-capable execution backend against the exact motor M image and internal interface before committing to full dual-MCU emulation. A D/M behavioral peer can unblock D testing, but results must identify that peer as modeled. Radio application slices may validate record/GATT mapping before a full BLE stack is attempted.

Recover physical E-TUBE framing, arbitration, collision/retry behavior and timing from evidence before introducing a wire-level network model. Packet loss alone does not reproduce electrical collisions. Start with the packet-level network for offline settings iteration; add electrical/timing detail only for scenarios that need it.

**Gate:** M firmware executes an independently verified exchange with D, then boot/update interruption scenarios work across both processors. Radio results distinguish notification construction from successful over-the-air delivery. Physical-bus claims require comparison with captured timing/arbitration traces, beyond successful application replies.

## First implementation decision

Proceed with stages 1–2 using the validated Unicorn packet boundary. They can provide useful offline regression coverage without waiting for complete board emulation. Scope full boot, actual M execution and BLE radio execution as separate feasibility spikes with evidence-based gates; the current POC does not establish their cost or completeness.

Recovered addresses and boundary behavior originate in local `esteps-recover` evidence: `evidence/ble-us-28-diagnosis-20261015`, `evidence/ble-privileged-auth-20261015`, and `esteps-cli/research/ble-destination-4110/verify_gateway_master.py`, `verify_rejection.py`, `verify_slave_session.py`. These local inputs are not required source imports: the POC is standalone once the two decoded images are supplied. Unicorn APIs were checked against its [Python binding](https://github.com/unicorn-engine/unicorn/blob/master/bindings/python/unicorn/unicorn_py3/unicorn.py) and [hook documentation](https://github.com/unicorn-engine/unicorn/blob/master/docs/Hooks.md).
