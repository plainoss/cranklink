# Shimano STEPS DU-EP800 (D4.2.0) Firmware Architecture Reference

## 1. Executive Summary

This document specifies the technical architecture and operational mechanisms of the **Shimano STEPS DU-EP800 D4.2.0** application firmware. 

The motor drive unit runs two coordinated microcontrollers:
- **D Application Processor**: An ARM Cortex-M microcontroller executing Thumb-2 machine code. It acts as the master communication gateway, handling the physical wire protocol (E-TUBE bus over CAN/UART), node registration, setting persistence, packet fragmentation, and user assist configuration.
- **M Motor Inverter Processor**: A motor-control microcontroller (Renesas RX24T family) dedicated to three-phase BLDC PWM generation, field-oriented control (FOC), and low-level torque sensing.

---

## 2. Memory Architecture & Address Mapping

The D processor executes from an internal flash address space mapped starting at `0x00010000`:

```
+------------------------------------------------------------------------+
| 0x00010000 - 0x0004F000 : Flash ROM (252 KiB Image)                    |
|   - 0x00010040 : Exception Vector Table & Reset Entry                 |
|   - 0x00010DC0 : Command Translation / 3-Byte Lookup Table            |
|   - 0x00011BFC : Group 1 Dispatch Jump Table                          |
|   - 0x00019380 : Central Ingress Dispatcher Entry                     |
|   - 0x0001B7C0 : Setting & Configuration Dispatcher                    |
|   - 0x0001E998 : Main Event Scheduler                                |
|   - 0x00022D48 : Wire Transport & Packet Ingress Frame Handler        |
+------------------------------------------------------------------------+
| 0x20000000 - 0x20010000 : Internal SRAM (64 KiB)                       |
|   - 0x2000049A : Series ID (0x25 for EP800)                           |
|   - 0x2000049E : Bus Role ID (0x01 = Drive Unit)                      |
|   - 0x200020E4 : Group 1 Query Cache                                  |
|   - 0x200023CC : Active Role Slots / Connected Peer Table             |
|   - 0x200023EA : Current Bus Master / Owner Node                      |
|   - 0x200026B4 : Outgoing Wire Reply Buffer                           |
|   - 0x2000273C : Authentication Challenge Nonce Output Buffer          |
|   - 0x20002C40 : Authentication Challenge 16-bit Sequence Counter     |
|   - 0x20002C7C : Staged Max Power Ceiling (Watts)                     |
|   - 0x20002D42 : Active Assist Mode (0=Off, 1=Eco, 2=Trail, 3=Boost)  |
|   - 0x20002D5A : Authentication State Flag                            |
|   - 0x20002D7B : Ingress Primary Lane Occupancy Counter (Max 15)      |
|   - 0x20002D7F : Ingress Secondary Lane Occupancy Counter (Max 5)     |
|   - 0x20002DB8 : Pending Flash Commit Request Flag                    |
|   - 0x20002DCC : Staged Lamp Output State                             |
|   - 0x20002E05 : Staged Profile Selector (0=Profile 1, 1=Profile 2)   |
|   - 0x20002E0B : Staged Assist Setting Selector                       |
|   - 0x20002E0C : Staged Assist Operation (0=Stage, 1=Reset Defaults)  |
|   - 0x20002E0D : Staged Assist Multiplier                             |
|   - 0x20002E0E : Staged Max Torque (Nm)                               |
|   - 0x20002E0F : Staged Start Response Characteristic                 |
|   - 0x20002E10 : Staged High-Cadence Cutoff Behavior                  |
|   - 0x20002E2E : Flash Wear-Leveling Active Page Index                |
|   - 0x20002E49 : M-Side Motor Version Cache (4.2.0)                   |
|   - 0x20002EAD : Connection State Flag (1=Connected, 0=Idle)          |
|   - 0x20009000 : Ingress Packet Buffer                                |
+------------------------------------------------------------------------+
| 0x40000000 - 0x40010000 : Peripherals & Hardware IO (UART, Timers, DMA)|
+------------------------------------------------------------------------+
| 0xE000E000 - 0xE0010000 : ARM Cortex-M System Control & NVIC           |
+------------------------------------------------------------------------+
```

---

## 3. Communication Protocol & Dispatch Pipeline

### 3.1. Packet Framing

Packets arriving over the physical bus are framed into variable-length structures:
- `byte 0`: Payload length.
- `byte 1`: Target node role or broadcast address.
- `byte 2`: Command Opcode (Key).
- `byte 3..N`: Command parameters or payload.

The key decoding logic is evaluated at entry `0x0001B7C0` as:
$$\text{Key} = \text{packet}[0] \mid ((\text{packet}[1] \mathbin{\&} 0\text{xFE}) \ll 8)$$

### 3.2. Two-Stage Command Execution (Staging vs. Committing)

Shimano E-TUBE employs an **atomic two-phase staging mechanism**:

1. **Phase 1: In-Memory Staging (`0x16` Commands)**:
   - When a tuning parameter (such as Eco assist multiplier or max torque) is modified, the value is written to temporary SRAM cells (`0x20002E0D..0x20002E11`).
   - The motor immediately updates its runtime RAM tables so the user can feel the tuning change in real time.
   - Flash memory is not modified in this phase, protecting the flash from premature write exhaustion.
2. **Phase 2: Atomic Batch Commit (`0x17` Command)**:
   - Command `0x17` with operation `0x01` sets the flag at `ADDR_STAGED_COMMIT_REQUEST` (`0x20002DB8`).
   - The background scheduler detects this flag during an idle frame and flushes the entire staging buffer into the flash wear-leveling journal in a single atomic transaction.
   - If communication drops or power disconnects before `0x17` is acknowledged, the motor discards the staged SRAM values upon the next reboot.

This two-phase pattern is leveraged across Cranklink's Web and CLI interfaces to provide atomic batch commits and zero-risk rollback.

---

## 4. Assist Profiles & Tuning Engine

The EP800 firmware stores **two complete independent user profiles** (Profile 1 and Profile 2).

### 4.1. Parameter Matrix per Assist Mode

For each assist mode (ECO, TRAIL, BOOST), the firmware calculates assist assistance according to five core scalar parameters:

| Parameter | Address | Range | Function |
| :--- | :--- | :--- | :--- |
| **Assist Multiplier** | `0x20002E0D` | 1 – 10 | Proportion of motor torque added per unit of rider pedal torque. |
| **Max Torque** | `0x20002E0E` | 20 – 85 Nm | Absolute torque ceiling enforced on the motor shaft. |
| **Start Response** | `0x20002E0F` | 1 – 5 | Ramp-up rate (milliseconds to peak torque upon initial crank movement). |
| **Cadence Cutoff** | `0x20002E10` | 1 – 3 | Tapering curve above 90 RPM (steep drop vs. sustained high-cadence power). |
| **Max Power Mode** | `0x20002E12` | 1 – 3 | Electrical wattage limit (e.g., 250W continuous, 500W peak). |

### 4.2. Speed Limiter Implementation

Speed limiting in the firmware is evaluated in the motor feedback loop:
- The firmware computes wheel speed from cadence, rear wheel circumference (stored in millimeters at `0x20002CA0`), and tooth gear ratios (`front_teeth_set` and `queue_rear_teeth_pattern_reply`).
- When calculated road speed approaches the regional speed limit ceiling (25.0 km/h in EU, 20.0 mph / 32.0 km/h in US, 24.0 km/h in JP):
  - A proportional reduction curve attenuates the assist multiplier starting 1.5 km/h before the limit.
  - At $\ge \text{ceiling}$, the assist multiplier is clamped strictly to zero.

---

## 5. Storage Engine: Wear-Leveling Flash Journal

The D firmware does not store configuration at fixed flash addresses. Instead, it maintains a circular, wear-leveled journal:
- Flash storage rotates across **three circular 6 KiB banks**.
- Active page index is tracked in SRAM at `0x20002E2E`.
- Each record entry contains a magic sync word, sequence counter, record ID, and 16-bit CRC.
- **Compaction History Retention**: During bank compaction:
  - Record `0x05`: retains up to **7 historical snapshots**.
  - Record `0x06`: retains up to **3 historical snapshots**.
  - Record `0x0A`: retains up to **7 historical snapshots**.
  - All remaining 60 record types discard prior history and retain only their current active value.
- If a journal page is corrupted or interrupted mid-write, the firmware marks the damaged block and rolls back to the previous intact checkpoint.

---

## 6. Packet Admission & Security Infrastructure

### 6.1. Dual-Lane Ingress Queue Admission

The E-TUBE transport engine divides incoming packet fragments into two dedicated capacity lanes:
- **Primary Lane (Lane A)**: Capacity of **15 fragments**, tracked at `0x20002D7B`.
- **Secondary Lane (Lane B)**: Capacity of **5 fragments**, tracked at `0x20002D7F`.

Incoming packets are partitioned into 3-byte fragments based on payload size:
- $\le 3$ bytes: 1 fragment
- $4..6$ bytes: 2 fragments
- $\ge 7$ bytes: 3 fragments

**Admission Invariant**: Admission requires capacity for the entire packet before accepting the first fragment:
$$\text{count} + \text{fragments} \le \text{capacity}$$
If the required fragments exceed remaining lane capacity, the entire packet is rejected atomically, preventing partial fragment deadlocks.

### 6.2. Authentication Nonce Construction

Authentication challenge nonces (16 bytes) are generated by combining six component identity bytes with a wrapping 16-bit sequence counter (at `0x20002C40`):
- For even sequence values, a bit shift of 8 is applied; for odd sequence values, a shift of 0 is applied.
- The 16-byte output buffer at `0x2000273C` is populated with eight 16-bit composite words generated via additive operations with bitwise inversions across the identity and counter values.
