#!/usr/bin/env python3
"""Run hash-pinned stock firmware through a deterministic virtual packet bus."""
import hashlib
import argparse
import json
import sys
from pathlib import Path
from fw_sim.profiles import PROFILES
from fw_sim.scenarios import run, FAULTS
from fw_sim.pull import (
    pull_firmware,
    pull_all,
    list_catalog,
    resolve_firmware_target,
    resolve_hardware_pair,
    get_default_cache_dir,
    HARDWARE_PAIRS,
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('profiles', help='list executable image hashes')

    from fw_sim.cpu import CPU_MODELS, DEFAULT_CPU
    resident = sub.add_parser('resident', help='run EP800 resident SPI/key recovery regressions offline')
    resident.add_argument('--image', required=True, type=Path, help='local acquired 64 KiB resident dump')
    resident.add_argument('--cpu', choices=sorted(CPU_MODELS), default=DEFAULT_CPU,
                          help='execution model only; does not establish the physical CPU/ISA')
    resident.add_argument('--output', required=True, type=Path)

    pull_cmd = sub.add_parser('pull', help='auto-pull and cache firmware from official Shimano URLs')
    pull_cmd.add_argument('names', nargs='*', help='firmware profile IDs, models, or filenames (e.g. ep800-4.2.0, sc-em800)')
    pull_cmd.add_argument('--all', action='store_true', help='pull all 18 profiled firmware images')
    pull_cmd.add_argument('--hardware', help=f'pull default pair for hardware ({", ".join(sorted(HARDWARE_PAIRS.keys()))})')
    pull_cmd.add_argument('--list', action='store_true', help='list all available firmware targets and cache status')
    pull_cmd.add_argument('--cache-dir', type=Path, help='custom local cache directory')
    pull_cmd.add_argument('--force', action='store_true', help='re-download and overwrite cached files')
    pull_cmd.add_argument('--fixtures-output', type=Path, help='write fixture manifest JSON for matrix')

    catalog = sub.add_parser('inventory', help='classify all supplied images and expose coverage gaps')
    catalog.add_argument('--root', type=Path, action='append', required=True)
    catalog.add_argument('--output', type=Path, required=True)
    catalog.add_argument('--fixtures-output', type=Path, help='optional local manifest for matrix')

    single = sub.add_parser('run', help='run one firmware pair with all fault scenarios')
    single.add_argument('--gateway', help='gateway image path or profile/model name (auto-pulled if needed)')
    single.add_argument('--motor', help='motor image path or profile/model name (auto-pulled if needed)')
    single.add_argument('--hardware', help='run with default gateway and motor for given hardware (e.g. ep800, ep600)')
    single.add_argument('--cache-dir', type=Path, help='local firmware cache directory')
    single.add_argument('--slot', type=int, default=13)
    single.add_argument('--output', required=True, type=Path)

    matrix = sub.add_parser('matrix', help='execute every gateway/motor pair in a local fixture manifest')
    matrix.add_argument('--fixtures', type=Path, required=True, help='JSON object mapping profile IDs to image paths')
    matrix.add_argument('--output', type=Path, required=True)

    serve = sub.add_parser('serve', help='serve a local native simulator socket')
    serve.add_argument('--gateway', help='gateway image path or profile/model name (auto-pulled if needed)')
    serve.add_argument('--motor', help='motor image path or profile/model name (auto-pulled if needed)')
    serve.add_argument('--hardware', help='serve with default gateway and motor for given hardware (e.g. ep800, ep600)')
    serve.add_argument('--cache-dir', type=Path, help='local firmware cache directory')
    serve.add_argument('--socket', required=True, type=Path)
    serve.add_argument('--gatt-fault', choices=('stale-free','duplicate-free'), help='inject modeled radio notifications')
    serve.add_argument('--gatt', action='store_true', help='serve modeled GATT for the real BLE session engine')
    serve.add_argument('--drop-replies', action='store_true', help='drop normal GET replies for client timeout testing')

    args = parser.parse_args()

    if args.command == 'profiles':
        print(json.dumps(PROFILES, indent=2))
        return

    if args.command == 'resident':
        from fw_sim.resident import run_regression
        report = run_regression(args.image, cpu=args.cpu)
        args.output.write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(dict(passed=report['passed'], scenarios=len(report['cases']),
                              cpu_model=report['cpu_model'], hardware_access=False)))
        return

    if args.command == 'pull':
        cache_dir = args.cache_dir or get_default_cache_dir()
        if args.list:
            items = list_catalog(cache_dir=cache_dir)
            print(json.dumps(items, indent=2))
            return
        if args.all:
            manifest = pull_all(cache_dir=cache_dir, fixtures_output=args.fixtures_output)
            print(json.dumps(dict(pulled=len(manifest), cache_dir=str(cache_dir.resolve())), indent=2))
            return
        if args.hardware:
            gw_p, mot_p = resolve_hardware_pair(args.hardware, cache_dir=cache_dir)
            print(json.dumps(dict(hardware=args.hardware, gateway=str(gw_p.resolve()), motor=str(mot_p.resolve())), indent=2))
            return
        if not args.names:
            print("Error: Specify firmware names to pull, or pass --all, --hardware, or --list", file=sys.stderr)
            raise SystemExit(1)
        results = {}
        for name in args.names:
            p = pull_firmware(name, cache_dir=cache_dir, force=args.force)
            results[name] = str(p.resolve())
        print(json.dumps(results, indent=2))
        return

    if args.command == 'inventory':
        from fw_sim.inventory import inventory
        report, fixtures = inventory(args.root)
        args.output.write_text(json.dumps(report,indent=2)+'\n')
        if args.fixtures_output:
            args.fixtures_output.write_text(json.dumps(fixtures,indent=2)+'\n')
        print(json.dumps(dict(images=report['unique_images'],profiled=report['profiled_images'])))
        return

    if args.command == 'serve':
        cache_dir = args.cache_dir or get_default_cache_dir()
        if args.hardware:
            gw_path, mot_path = resolve_hardware_pair(
                args.hardware,
                gateway_override=args.gateway,
                motor_override=args.motor,
                cache_dir=cache_dir,
            )
        else:
            if not args.gateway or not args.motor:
                print("Error: Specify both --gateway and --motor, or use --hardware", file=sys.stderr)
                raise SystemExit(1)
            gw_path = resolve_firmware_target(args.gateway, kind='gateway', cache_dir=cache_dir)
            mot_path = resolve_firmware_target(args.motor, kind='motor', cache_dir=cache_dir)

        if args.gatt:
            from fw_sim.gatt import serve as serve_gatt
            serve_gatt(gw_path, mot_path, args.socket, args.drop_replies, args.gatt_fault)
        else:
            if args.gatt_fault:
                raise ValueError('--gatt-fault requires --gatt')
            from fw_sim.endpoint import serve as serve_endpoint
            serve_endpoint(gw_path, mot_path, args.socket, args.drop_replies)
        return

    cases, failures = [], []
    if args.command == 'run':
        cache_dir = args.cache_dir or get_default_cache_dir()
        if args.hardware:
            gw_path, mot_path = resolve_hardware_pair(
                args.hardware,
                gateway_override=args.gateway,
                motor_override=args.motor,
                cache_dir=cache_dir,
            )
        else:
            if not args.gateway or not args.motor:
                print("Error: Specify both --gateway and --motor, or use --hardware", file=sys.stderr)
                raise SystemExit(1)
            gw_path = resolve_firmware_target(args.gateway, kind='gateway', cache_dir=cache_dir)
            mot_path = resolve_firmware_target(args.motor, kind='motor', cache_dir=cache_dir)
        pairs = [(gw_path, mot_path)]
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
