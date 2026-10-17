#!/usr/bin/env python3
"""Run hash-pinned stock firmware through a deterministic virtual packet bus."""
import hashlib
import argparse
import json
import sys
from pathlib import Path
from fw_sim.profiles import PROFILES
from fw_sim.scenarios import run, FAULTS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('profiles', help='list executable image hashes')
    catalog = sub.add_parser('inventory', help='classify all supplied images and expose coverage gaps')
    catalog.add_argument('--root', type=Path, action='append', required=True)
    catalog.add_argument('--output', type=Path, required=True)
    catalog.add_argument('--fixtures-output', type=Path, help='optional local manifest for matrix')
    single = sub.add_parser('run', help='run one firmware pair with all fault scenarios')
    single.add_argument('--gateway', required=True, type=Path)
    single.add_argument('--motor', required=True, type=Path)
    single.add_argument('--slot', type=int, default=13)
    single.add_argument('--output', required=True, type=Path)
    matrix = sub.add_parser('matrix', help='execute every gateway/motor pair in a local fixture manifest')
    matrix.add_argument('--fixtures', type=Path, required=True, help='JSON object mapping profile IDs to image paths')
    matrix.add_argument('--output', type=Path, required=True)
    serve = sub.add_parser('serve', help='serve a local native simulator socket')
    serve.add_argument('--gateway', required=True, type=Path)
    serve.add_argument('--motor', required=True, type=Path)
    serve.add_argument('--socket', required=True, type=Path)
    serve.add_argument('--gatt-fault', choices=('stale-free','duplicate-free'), help='inject modeled radio notifications')
    serve.add_argument('--gatt', action='store_true', help='serve modeled GATT for the real BLE session engine')
    serve.add_argument('--drop-replies', action='store_true', help='drop normal GET replies for client timeout testing')
    args = parser.parse_args()
    if args.command == 'profiles':
        print(json.dumps(PROFILES, indent=2)); return
    if args.command == 'inventory':
        from fw_sim.inventory import inventory
        report, fixtures = inventory(args.root)
        args.output.write_text(json.dumps(report,indent=2)+'\n')
        if args.fixtures_output:
            args.fixtures_output.write_text(json.dumps(fixtures,indent=2)+'\n')
        print(json.dumps(dict(images=report['unique_images'],profiled=report['profiled_images'])))
        return
    if args.command == 'serve':
        if args.gatt:
            from fw_sim.gatt import serve
        else:
            from fw_sim.endpoint import serve
        if args.gatt:
            serve(args.gateway, args.motor, args.socket, args.drop_replies, args.gatt_fault)
        else:
            if args.gatt_fault: raise ValueError('--gatt-fault requires --gatt')
            serve(args.gateway, args.motor, args.socket, args.drop_replies)
        return
    cases, failures = [], []
    if args.command == 'run':
        pairs = [(args.gateway, args.motor)]
    else:
        fixtures = json.loads(args.fixtures.read_text())
        known = {p['id']: p for p in PROFILES}
        for name in fixtures:
            if name not in known: raise ValueError(f'unknown fixture profile {name}')
        # Resolve relative fixture paths against the manifest, not the caller.
        fixtures = {k: (args.fixtures.parent / v).resolve() for k,v in fixtures.items()}
        pairs = [(g,m) for gn,g in fixtures.items() if known[gn]['kind']=='gateway'
                 for mn,m in fixtures.items() if known[mn]['kind']=='motor']
        if not pairs: raise ValueError('fixture manifest needs a gateway and a motor')
    for gateway, motor in pairs:
        try:
            for fault in FAULTS:
                cases.append(run(gateway, motor, fault, getattr(args, 'slot', 13)))
        except Exception as error:
            failures.append(dict(gateway=gateway.name, motor=motor.name, error=str(error)))
    report = dict(schema_version=2, passed=not failures, hardware_access=False,
                  profiles_sha256=hashlib.sha256(json.dumps(PROFILES,sort_keys=True).encode()).hexdigest(),
                  level='function-level packet co-simulation',
                  modeled=['initialized RAM/plain-memory MMIO', 'motor RX entry',
                           'motor packet TX acceptance', 'logical time', 'FIFO fragment delivery',
                           'host/radio TX ring consumption'],
                  not_validated=['reset/boot', 'physical bus timing', 'M/RX MCU', 'radio', 'persistent writes'],
                  pairs=len(pairs), scenarios=len(cases), cases=cases, failures=failures)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('passed','pairs','scenarios','failures')}))
    if failures: raise SystemExit(1)


if __name__ == '__main__':
    main()
