#!/usr/bin/env python3
"""Actual native CLI feature/guard checks; firmware inputs and raw reports stay local."""
import argparse
import concurrent.futures
import json
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile
import time


CASES = (
    'units', 'destination-read', 'snapshot-export', 'schema-offline', 'read-batch',
    'lamp-dry-run', 'serial-mismatch', 'component-mismatch', 'read-only-write',
    'no-auth-oem-write', 'endpoint-write-rejection', 'restore-preflight',
)


def check(case, binary, gateway, motor):
    with tempfile.TemporaryDirectory(prefix='clfeature-') as directory:
        root = Path(directory)
        socket = root / 's'
        snapshot, backup, plan = root/'snapshot.json', root/'backup.json', root/'plan.json'
        plan.write_text(json.dumps([{'op':'read','name':'lamp'}, {'op':'read','name':'destination'}]))
        broken = root / 'broken.json'
        broken.write_text('{')
        command, expected = {
            'units': (['units'], None),
            'destination-read': (['settings','read','destination'], None),
            'snapshot-export': (['settings','read','--output',str(snapshot)], None),
            'schema-offline': (['settings','schema','lamp'], None),
            'read-batch': (['settings','batch',str(plan),'--backup-prefix',str(root/'batch')], None),
            'lamp-dry-run': (['-s','000000000000','--no-auth','settings','set','lamp','1','--dry-run'], None),
            'serial-mismatch': (['-s','111111111111','probe'], 'motor serial mismatch'),
            'component-mismatch': (['--component','DU-EP801','probe'], 'matching slots []'),
            'read-only-write': (['--read-only','settings','set','lamp','1'], '--read-only rejects'),
            'no-auth-oem-write': (['-s','000000000000','--no-auth','settings','set','destination','1',
                                   '--backup',str(backup)], '--no-auth was specified'),
            'endpoint-write-rejection': (['-s','000000000000','--no-auth','settings','set','lamp','1',
                                         '--backup',str(backup)], 'firmware simulator closed the session'),
            'restore-preflight': (['settings','restore',str(broken)], 'while parsing'),
        }[case]
        process = subprocess.Popen([sys.executable,str(Path(__file__).with_name('sim.py')),'serve',
                                    '--gateway',str(gateway),'--motor',str(motor),'--socket',str(socket)],
                                   stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        started = time.monotonic()
        try:
            selector = selectors.DefaultSelector()
            selector.register(process.stdout, selectors.EVENT_READ)
            if not selector.select(10):
                raise RuntimeError('simulator did not become ready within 10s')
            ready = process.stdout.readline()
            selector.close()
            if not ready.startswith('Simulator ready:'):
                raise RuntimeError('simulator startup failed')
            # Prove these are offline/preflight paths: no server exists at this
            # second socket, so an accidental open would fail the assertion.
            port = root/'absent' if case in ('schema-offline','read-only-write','restore-preflight') else socket
            completed = subprocess.run([str(binary),'--json','--port',f'sim:{port}',*command],
                                       capture_output=True,text=True,timeout=75,cwd=root)
        finally:
            process.terminate()
            _, server_errors = process.communicate(timeout=5)
        result = dict(case=case, exit_code=completed.returncode, expected_error=expected,
                      stdout=completed.stdout, stderr=completed.stderr, server_stderr=server_errors,
                      seconds=round(time.monotonic()-started,2), artifacts=sorted(p.name for p in root.glob('*.json*')))
        if expected:
            result['passed'] = completed.returncode != 0 and expected.lower() in completed.stderr.lower()
            if case == 'endpoint-write-rejection':
                result['passed'] = result['passed'] and 'simulator endpoint is read-only' in server_errors
            else:
                result['passed'] = result['passed'] and not server_errors.strip()
            if case in ('read-only-write', 'no-auth-oem-write', 'restore-preflight'):
                result['passed'] = result['passed'] and not backup.exists()
        else:
            result['passed'] = completed.returncode == 0 and not server_errors.strip()
            if result['passed']:
                try:
                    value = json.loads(completed.stdout)
                    result['json_type'] = type(value).__name__
                    if case == 'units':
                        result['passed'] = any(u.get('slot') == 13 for u in value['units'])
                    if case == 'snapshot-export':
                        result['passed'] = (snapshot.exists() and json.loads(snapshot.read_text()) == value
                                            and 'lamp' in value['values'] and 'destination' in value['values'])
                        result['available_settings'] = len(value['values'])
                        result['unavailable_settings'] = value['unavailable']
                    if case == 'destination-read':
                        result['passed'] = 'value' in value and bool(bytes.fromhex(value['raw']))
                    if case == 'schema-offline':
                        result['passed'] = value.get('name') == 'lamp' and value.get('oem') is False
                    if case == 'lamp-dry-run':
                        result['passed'] = value.get('dry_run') is True and not list(root.glob('backup*.json'))
                    if case == 'read-batch':
                        result['passed'] = value.get('completed') == 2 and len(value['results']) == 2
                except (ValueError, OSError, AttributeError) as error:
                    result['passed'], result['validation_error'] = False, str(error)
        return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--fixtures',type=Path,required=True)
    parser.add_argument('--gateway',default='sc-em800-4.6.0')
    parser.add_argument('--motor',default='ep800-4.2.0')
    parser.add_argument('--output',type=Path,required=True,help='local raw results; do not commit')
    parser.add_argument('--case',choices=CASES,action='append',dest='cases')
    args=parser.parse_args()
    entries=json.loads(args.fixtures.read_text())
    gateway=(args.fixtures.parent/entries[args.gateway]).resolve()
    motor=(args.fixtures.parent/entries[args.motor]).resolve()
    cases=args.cases or CASES
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        futures=[pool.submit(check,case,args.binary.resolve(),gateway,motor) for case in cases]
        results=[]
        for case,future in zip(cases,futures):
            try: results.append(future.result())
            except Exception as error: results.append(dict(case=case,passed=False,error=str(error)))
    report=dict(passed=all(r['passed'] for r in results),gateway=args.gateway,motor=args.motor,
                scope='actual native CLI; synthetic NVM; modeled USB adapter and M version',cases=results)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(passed=report['passed'],cases=len(results),
                         failures=[r['case'] for r in results if not r['passed']])))
    return 0 if report['passed'] else 1


if __name__=='__main__':
    raise SystemExit(main())
