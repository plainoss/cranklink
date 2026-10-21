#!/usr/bin/env python3
"""Reproduce stock D relay experiments and the actual Rust sender matrix offline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from fw_sim.d_relay import D_PROFILES, RADIO_SHA256
from fw_sim.profiles import identify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    config = json.loads(args.config.read_text())
    profiles = {}
    for path in config['gateways']:
        _, profile = identify(path, 'gateway')
        profiles[path] = profile
    env = dict(os.environ, CRANKLINK_D_RELAY_CONFIG=str(args.config.resolve()),
               CRANKLINK_SIM_PYTHON=sys.executable, CRANKLINK_D_RELAY_IMAGE_BYTES='156300')
    commands = [
        [sys.executable, '-m', 'unittest', 'discover', '-s', 'tools/firmware-sim', '-p', 'test_d_relay*.py', '-v'],
        ['cargo', 'test', '--locked', '--lib', 'stock_d_relay_actual_sender_matrix', '--', '--ignored', '--nocapture'],
    ]
    checks = []
    cases = []
    declared_cases = 0
    for command in commands:
        result = subprocess.run(command, cwd=root, env=env, capture_output=True, text=True, timeout=1200)
        combined = result.stdout + result.stderr
        declared = re.search(r'D relay matrix: (\d+) cases passed', combined)
        if declared:
            declared_cases = int(declared[1])
        check = dict(command=command[1:] if command[0] == sys.executable else command,
                     passed=result.returncode == 0,
                     output_sha256=hashlib.sha256(combined.encode()).hexdigest())
        if result.returncode:
            check['diagnostic'] = combined[-6000:]
        checks.append(check)
        for line in combined.splitlines():
            match = re.match(r'D relay gateway=(".*") prefetch=(true|false) fragment_batch=(true|false) ack_ms=(\d+) fault=(\S+) length=(\d+): (\{.*\})', line)
            if match:
                path, prefetch, fragment_batch, ack_ms, fault, length, stats = match.groups()
                path = json.loads(path)
                cases.append(dict(gateway=profiles[path]['id'], prefetch=prefetch == 'true', fragment_batch=fragment_batch == 'true', ack_write_ms=int(ack_ms), fault=fault, image_bytes=int(length),
                                  stats=json.loads(stats)))
        print(json.dumps(dict(check=command[1] if command[0] == sys.executable else command[0],
                              passed=check['passed'])), flush=True)
    timings = []
    for baseline in cases:
        if baseline['fault'] != 'none' or baseline['prefetch']:
            continue
        candidate = next((c for c in cases if c['gateway'] == baseline['gateway'] and c['fault'] == 'none'
                          and c['prefetch'] and c['fragment_batch'] == baseline['fragment_batch'] and c['ack_write_ms'] == baseline['ack_write_ms']), None)
        if candidate:
            old, new = baseline['stats']['stream_seconds'], candidate['stats']['stream_seconds']
            timings.append(dict(gateway=baseline['gateway'], image_bytes=baseline['image_bytes'],
                                ack_write_ms=baseline['ack_write_ms'], fragment_batch=baseline['fragment_batch'], sequential_seconds=old,
                                prefetch_seconds=new, speedup=old / new, time_saved_percent=100 * (old - new) / old))
    calibration = (Path(__file__).parent / 'd-transfer-analysis.json').read_bytes()
    report = dict(schema_version=2, passed=all(c['passed'] for c in checks) and len(cases) == declared_cases > 0 and bool(timings),
                  hardware_access=False, production_d_schedule_changed=True,
                  production_behavior='CLI selects sequential/prefetched BLE DCASX and optional ordered fragment batching; wired and older formats stay sequential. All paths purge stale83/C0 before transfer dispatch.',
                  timing_model='Trace-calibrated mean upload and transfer phases; virtual events, not emulator wall time. ACK write return duration is assumed, not measured.',
                  timing_calibration_sha256=hashlib.sha256(calibration).hexdigest(),
                  timings=timings,
                  radio_sha256=RADIO_SHA256,
                  gateways=[dict(id=p['id'], sha256=p['sha256'], **D_PROFILES[p['id']]) for p in profiles.values()],
                  stock_executed=['Nordic4.9.0 PCE fragment codec and UART enqueue/ring bounds',
                                  'SC-EM800 PCE dispatch, two upload buffers, transfer bus enqueue',
                                  'bus TX ID completion matching and C0 construction'],
                  modeled=['UART busy state and explicit ring consumer', 'host TX ring consumer',
                           'physical bus queue consumption and TX completion callbacks',
                           'motor boot identity, erase/program/checksum/finish/reset replies',
                           'direct modeled motor replies to BLE notification queue',
                           'ordered GATT writes; no OS Bluetooth stack'],
                  not_validated=['installed radio version', 'RF/SoftDevice/ATT delivery',
                                 'negotiated connection parameters', 'motor bootloader execution',
                                 'reset-to-main startup', 'wall-clock speedup', 'legacy unindexed DCAS2'],
                  checks=checks, rust_cases=len(cases), cases=cases)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(passed=report['passed'], rust_cases=len(cases), output=str(args.output))))
    if not report['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
