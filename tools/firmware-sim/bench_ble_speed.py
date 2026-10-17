#!/usr/bin/env python3
"""Benchmark and validate BLE firmware update speed and pipelining using the firmware simulator.

Simulates wire-level BLE ATT transmission of the complete 128,192-byte Renesas M image
across multiple queue widths (1, 2, 4, 8) and BLE connection intervals (60ms, 30ms, 15ms).
Verifies complete image integrity, sequence ordering, and window checksums.
"""

import json
import os
import sys
import time
from pathlib import Path

# Add fw_sim to path
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools' / 'firmware-sim'))

from fw_sim.gatt import (
    GattEndpoint,
    AUTH_CONTROL,
    AUTH_NONCE,
    FEATURE,
    PCE_COMMAND,
    PCE_RESPONSE,
    FREE_COMMAND,
    FREE_RESPONSE,
    NONCE,
    USER_PROOF,
    APP_PROOF,
)


def authenticate(endpoint):
    endpoint.request(dict(op='write', id=AUTH_CONTROL, bytes=list(USER_PROOF)))
    endpoint.request(dict(op='write', id=AUTH_CONTROL, bytes=list(APP_PROOF)))
    endpoint.request(dict(op='write', id=FEATURE, bytes=[0xff, 0]))
    endpoint.request(dict(op='subscribe', id=FREE_RESPONSE))
    endpoint.request(dict(op='subscribe', id=PCE_RESPONSE))


def run_benchmark(image_path, queue_width, connection_interval_s=0.015, packets_per_interval=4):
    image_bytes = Path(image_path).read_bytes()
    assert len(image_bytes) == 128192, f"Expected 128192 bytes, got {len(image_bytes)}"
    total_checksum = sum(image_bytes) & 0xff

    # Create modeled endpoint with dummy gateway and motor paths
    gateway_dummy = ROOT / 'tools' / 'firmware-sim' / 'gateway'
    motor_dummy = ROOT / 'tools' / 'firmware-sim' / 'motor'
    
    # We mock machine setup so we can test GATT protocol cleanly without needing ARM image files
    from unittest.mock import Mock, patch
    from types import SimpleNamespace

    motor_mock = SimpleNamespace(
        connected=True,
        profile={'owner_ram': 0x20000000, 'auth_ram': 0x20000001},
        byte=Mock(return_value=0),
    )
    gateway_mock = SimpleNamespace(
        profile={'pce_dispatcher': 0x10000, 'free_forward': 0x10000, 'pce3': 0x10000, 'sched': 0x10000},
        restore=Mock(side_effect=lambda: setattr(motor_mock, 'connected', False)),
        slot=13,
        call=Mock(),
        link_stop=Mock(return_value=[]),
        notifications=[],
        send=Mock(return_value=dict(slot=13, payload='00')),
        receive=Mock(return_value=dict(slot=13, payload='018604010b00ff')),
    )

    with patch('fw_sim.gatt.setup', return_value=(gateway_mock, motor_mock)), \
         patch('fw_sim.gatt.Path.read_bytes', return_value=bytes(64)):
        endpoint = GattEndpoint(str(gateway_dummy), str(motor_dummy))

    endpoint.master = True
    authenticate(endpoint)
    endpoint.request(dict(
        op='configure_link',
        ci=connection_interval_s,
        ppi=packets_per_interval
    ))

    slot = 13
    # 1. Read bootloader version
    endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=[slot, 0, 0xf0, 0, 0x41, 0, 0, 0]))
    boot_reply = endpoint.request(dict(op='receive'))['notification']
    assert boot_reply['characteristic'] == FREE_RESPONSE
    assert boot_reply['value'][:4] == [slot, 0xf2, 1, 0x51], f"Unexpected boot reply: {boot_reply}"

    # 2. Erase
    endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=[slot, 0, 0xf0, 0, 0x21, 0, 0x0e, 0]))
    erase_reply = endpoint.request(dict(op='receive'))['notification']
    assert erase_reply['value'][:4] == [slot, 0xf2, 0, 0x31], f"Unexpected erase reply: {erase_reply}"

    # 3. Stream M image in 1024-byte windows
    window_len = 1024
    sequence = 0
    t_start = time.monotonic()

    for offset in range(0, len(image_bytes), window_len):
        chunk = image_bytes[offset:offset + window_len]
        # Build 4-byte packets
        packets = []
        for i in range(0, len(chunk), 4):
            sequence = (sequence + 1) % 8
            four_bytes = list(chunk[i:i + 4])
            packets.append([slot, 0, 0xf8, sequence] + four_bytes)

        # Batch packets according to queue_width
        for i in range(0, len(packets), queue_width):
            batch = packets[i:i + queue_width]
            if len(batch) == 1:
                endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=batch[0]))
            else:
                endpoint.request(dict(op='write_batch', id=FREE_COMMAND, packets=batch))

        # Check window checksum
        w_sum = sum(chunk) & 0xff
        endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=[slot, 0, 0xf0, 0, 0x25, w_sum, 0, 0]))
        w_reply = endpoint.request(dict(op='receive'))['notification']
        assert w_reply['value'][:4] == [slot, 0xf2, 0, 0x31], f"Window checksum rejected at offset {offset}: {w_reply}"

    # 4. Whole-image checksum verification
    endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=[slot, 0, 0xf0, 0, 0x27, total_checksum, 0, 0]))
    final_reply = endpoint.request(dict(op='receive'))['notification']
    assert final_reply['value'][:4] == [slot, 0xf2, 0, 0x31], f"Final checksum rejected: {final_reply}"

    # 5. Reset
    endpoint.request(dict(op='write', id=FREE_COMMAND, bytes=[slot, 0, 0xf0, 0, 0x28, 0, 0, 0]))
    reset_reply = endpoint.request(dict(op='receive'))['notification']
    assert reset_reply['value'][:4] == [slot, 0xf2, 0, 0x31], f"Reset rejected: {reset_reply}"

    stats = endpoint.request(dict(op='stats'))
    sim_time = stats['simulated_seconds']
    throughput = stats['throughput_bps']
    windows_verified = stats['rx_windows_verified']
    total_bytes = stats['rx_total_bytes']

    endpoint.close()

    return {
        'queue_width': queue_width,
        'connection_interval_ms': connection_interval_s * 1000,
        'simulated_seconds': sim_time,
        'simulated_minutes': round(sim_time / 60, 2),
        'throughput_bps': throughput,
        'total_bytes': total_bytes,
        'windows_verified': windows_verified,
        'packets_sent': stats['rx_stream_packets'],
    }


def main():
    if len(sys.argv) != 2:
        print('usage: bench_ble_speed.py /path/to/DUEP800-M.4.1.11.dat (128192 bytes)', file=sys.stderr)
        sys.exit(2)
    m_path = Path(sys.argv[1])
    if not m_path.exists():
        print(f"Error: {m_path} does not exist", file=sys.stderr)
        sys.exit(1)

    print("==========================================================================================")
    print("BLE FIRMWARE UPDATE SPEED BENCHMARK & SIMULATOR VALIDATION")
    print("Image: DUEP800-M.4.1.11.dat (128,192 bytes, 126 checksum windows, 32,048 packets)")
    print("Wired Reference (SM-PCE02 CAN): 101.71s (1.70 mins) @ 1,260.4 B/s")
    print("Physical BLE Reference (macOS Queue=1): ~1,935.7s (~32.26 mins) @ ~66.2 B/s")
    print("==========================================================================================")
    print()

    configs = [
        # (queue_width, connection_interval_s, packets_per_interval, label)
        (1, 0.060, 4, "Queue=1 (macOS Default 60ms Interval, Baseline)"),
        (2, 0.060, 4, "Queue=2 (macOS Default 60ms Interval)"),
        (4, 0.060, 4, "Queue=4 (macOS Default 60ms Interval)"),
        (8, 0.060, 4, "Queue=8 (macOS Default 60ms Interval)"),
        (1, 0.030, 4, "Queue=1 (30ms Interval)"),
        (2, 0.030, 4, "Queue=2 (30ms Interval)"),
        (4, 0.030, 4, "Queue=4 (30ms Interval)"),
        (8, 0.030, 4, "Queue=8 (30ms Interval)"),
        (1, 0.015, 4, "Queue=1 (Fast 15ms Interval)"),
        (2, 0.015, 4, "Queue=2 (Fast 15ms Interval)"),
        (4, 0.015, 4, "Queue=4 (Fast 15ms Interval)"),
        (6, 0.015, 6, "Queue=6 (Fast 15ms Interval + 6 pkts/interval)"),
        (8, 0.015, 8, "Queue=8 (Fast 15ms Interval + 8 pkts/interval)"),
    ]

    baseline_time = None
    results = []

    print(f"{'Config / Scenario':<52} | {'Time (s)':<9} | {'Time (min)':<10} | {'Speed (B/s)':<11} | {'Speedup':<8}")
    print("-" * 100)

    for q, ci, ppi, label in configs:
        res = run_benchmark(m_path, q, ci, ppi)
        res['label'] = label
        if baseline_time is None:
            baseline_time = res['simulated_seconds']
        speedup = baseline_time / res['simulated_seconds']
        res['speedup'] = round(speedup, 2)
        results.append(res)
        print(f"{label:<52} | {res['simulated_seconds']:<9.2f} | {res['simulated_minutes']:<10.2f} | {res['throughput_bps']:<11.1f} | {speedup:<8.2f}x")

    print("-" * 100)
    print()

    # Compare with wired
    wired_time = 101.71
    print("WIRED CAN COMPARISON:")
    for res in results:
        ratio = res['simulated_seconds'] / wired_time
        pct_of_wired = (wired_time / res['simulated_seconds']) * 100
        if res['queue_width'] in (4, 6, 8) and res['connection_interval_ms'] <= 30:
            status = "FASTER than wired CAN!" if ratio < 1.0 else f"{ratio:.2f}x of wired CAN speed ({pct_of_wired:.1f}%)"
            print(f"  * {res['label']}: {res['simulated_minutes']} mins -> {status}")

    # Output JSON summary
    out_path = ROOT / 'tools' / 'firmware-sim' / 'ble-speed-benchmark-results.json'
    out_path.write_text(json.dumps(results, indent=2))
    print()
    print(f"Saved benchmark results to {out_path}")


if __name__ == '__main__':
    main()
