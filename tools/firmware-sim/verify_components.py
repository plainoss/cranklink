#!/usr/bin/env python3
"""Current CLI against exact stock gateway/MU firmware, through native or GATT.

Boot caches, USB/radio, and opt-in EEPROM physical commit are explicit fixtures.
Actual MU parsing, PC-link, validation, record staging/checksum and replies run
unchanged. Actuator completion and full reset/boot remain unvalidated.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

from fw_sim.component_endpoint import serve_component
from fw_sim.component_gatt import serve_component_gatt
from fw_sim.component_mu import MuUr500, PROFILE
from fw_sim.profiles import PROFILES
from fw_sim.wire import Decoder

ROOT = Path(__file__).resolve().parents[2]
FORBIDDEN = {(0x16,0x7c),(1,0x84),(0x16,0xd8),(0x16,0xe0)}


def trace_summary(path, transport):
    commands = []
    decoder = Decoder()
    if path.exists():
        for line in path.read_text().splitlines():
            frame = json.loads(line)
            if frame.get('direction') != 'tx':
                continue
            raw = bytes.fromhex(frame.get('bytes',''))
            if transport == 'gatt':
                if frame.get('characteristic','').startswith('00002afe-') and len(raw) >= 3:
                    commands.append(tuple(raw[1:3]))
            else:
                for control,payload in decoder.feed(raw):
                    if control == 0x48 and len(payload) >= 3:
                        commands.append(tuple(payload[1:3]))
    return dict(unit_commands=len(commands),
                adjustment_sets=commands.count((0x0b,0x10)),
                adjustment_gets=commands.count((0x0b,0x14)),
                gear_sets=commands.count((0x0b,0x18)),
                gear_gets=commands.count((0x0b,0x1c)),
                motor_cargo_rx_auth_commands=sum(command in FORBIDDEN for command in commands))


def read_cases(folder):
    plan = folder/'read-plan.json'
    plan.write_text(json.dumps([{'op':'read'},{'op':'read','name':'hub-adjustment'},{'op':'read','name':'hub-gear-count'}]))
    snapshot = folder/'snapshot.json'
    return [
        ('units',['units'],None),
        ('probe-model',['-c','MU-UR500','probe'],None),
        ('adjustment-read',['-c','MU-UR500','settings','read','hub-adjustment'],None),
        ('gear-count-read',['-c','MU-UR500','settings','read','hub-gear-count'],None),
        ('snapshot',['-c','MU-UR500','settings','read','--output',str(snapshot)],None),
        ('read-batch',['-c','MU-UR500','settings','batch',str(plan),'--backup-prefix',str(folder/'batch')],None),
        ('adjustment-dry-run',['-c','MU-UR500','-s','000000000000','settings','set','hub-adjustment','-3','--dry-run'],None),
        ('gear-dry-run',['-c','MU-UR500','-s','000000000000','settings','set','hub-gear-count','5','--dry-run'],None),
        ('model-and-slot',['-c','MU-UR500','--slot','13','settings','read','hub-adjustment'],None),
        ('slot-only',['--slot','13','settings','read','hub-gear-count'],None),
        ('wrong-serial',['-c','MU-UR500','-s','010203040506','settings','read'],'serial mismatch'),
        ('wrong-model',['-c','SM-BMR1','settings','read'],'matching slots []'),
        ('wrong-slot',['-c','MU-UR500','--slot','12','settings','read'],'slot 12 absent'),
        ('out-of-range',['-c','MU-UR500','-s','000000000000','settings','set','hub-adjustment','5','--dry-run'],'must be -4..=4'),
        ('unsupported-motor-field',['-c','MU-UR500','settings','read','profile1.eco.torque'],'unsupported for MU-UR500'),
        ('read-only-write',['--read-only','-c','MU-UR500','settings','set','hub-adjustment','1'],'--read-only rejects'),
    ]


def write_cases(folder):
    prefix = ['-c','MU-UR500','-s','000000000000','settings']
    backup = folder/'gear-before.json'
    plan = folder/'write-plan.json'
    plan.write_text(json.dumps([
        {'op':'set','name':'hub-gear-count','value':5},
        {'op':'read','name':'hub-gear-count'},
        {'op':'restore','file':str(backup)},
        {'op':'read','name':'hub-gear-count'},
    ]))
    return [
        ('gear-set-5',prefix+['set','hub-gear-count','5','--backup',str(backup)],None),
        ('gear-read-5-after-reload',prefix+['read','hub-gear-count'],None),
        ('gear-set-11',prefix+['set','hub-gear-count','11','--backup',str(folder/'gear-before-11.json')],None),
        ('restore-baseline-8',prefix+['restore',str(backup),'--backup',str(folder/'restore-before.json')],None),
        ('mixed-set-read-restore-batch',prefix+['batch',str(plan),'--backup-prefix',str(folder/'mixed')],None),
        ('backup-collision',prefix+['set','hub-gear-count','11','--backup',str(backup)],'create settings backup'),
        ('write-serial-guard',['-c','MU-UR500','-s','010203040506','settings','set','hub-gear-count','5','--backup',str(folder/'wrong-serial.json')],'serial mismatch'),
        ('pending-adjustment',prefix+['set','hub-adjustment','-3','--backup',str(folder/'adjustment-before.json')],'actuator completion unconfirmed'),
        ('idempotent-adjustment',prefix+['set','hub-adjustment','-2','--backup',str(folder/'idempotent-adjustment-before.json')],None),
        ('baseline-read-after-failed-writes',prefix+['read','hub-gear-count'],None),
    ]


def validate_success(name, observed, folder):
    if not isinstance(observed,dict):
        return False
    if name == 'units':
        return observed['units'][0]['model'] == 'MU-UR500'
    if name == 'probe-model':
        return observed['cargo'] == 'MU-UR500' and observed['rx_version'] == 'not applicable'
    if name in ('adjustment-read','model-and-slot'):
        return observed['value'] == -2
    if name in ('gear-count-read','slot-only','baseline-read-after-failed-writes'):
        return observed['value'] == 8
    if name == 'gear-read-5-after-reload':
        return observed['value'] == 5
    if name == 'snapshot':
        return observed['unavailable'] == {} and observed['identity']['model_unit'] == 4 and observed['identity']['model_name'] == 'MU-UR500' and json.loads((folder/'snapshot.json').read_text()) == {key:value for key,value in observed.items() if key != 'ble_routing'}
    if name == 'read-batch':
        return observed['completed'] == 3 and observed['results'][0]['unavailable'] == {}
    if name.endswith('dry-run'):
        return observed.get('dry_run') is True and observed.get('valid') is True
    if name in ('gear-set-5','gear-set-11','restore-baseline-8'):
        target = {'gear-set-5':5,'gear-set-11':11,'restore-baseline-8':8}[name]
        return observed['changed'] is True and observed['after'] == target and Path(observed['backup']).exists()
    if name == 'idempotent-adjustment':
        return observed['changed'] is False and observed['before'] == -2 and observed['after'] == -2 and Path(observed['backup']).exists()
    if name == 'mixed-set-read-restore-batch':
        return observed['completed'] == 4 and observed['results'][1]['value'] == 5 and observed['results'][3]['value'] == 8
    return True


def group(binary, gateway_name, gateway_path, component_path, transport, write_tests, case_names=None):
    results = []
    with tempfile.TemporaryDirectory(prefix='cranklink-mu-') as directory:
        folder = Path(directory)
        socket_path, state = folder/'peer.sock', folder/'state.json'
        cases = read_cases(folder) + (write_cases(folder) if write_tests else [])
        if case_names:
            unknown = set(case_names)-{case[0] for case in cases}
            if unknown:
                raise ValueError('unknown component cases: '+', '.join(sorted(unknown)))
            cases = [case for case in cases if case[0] in case_names]
        command = [sys.executable,str(Path(__file__).resolve()),'--serve','--gateway',str(gateway_path),
                   '--component-image',str(component_path),'--socket',str(socket_path),'--transport',transport]
        if write_tests:
            command += ['--write-tests','--state',str(state)]
        env = dict(os.environ)
        env.pop('CRANKLINK_PASSKEY',None)
        env.pop('CRANKLINK_PASSKEY_FILE',None)
        with tempfile.TemporaryFile(mode='w+') as server_errors:
            server = subprocess.Popen(command,cwd=ROOT,env=env,stdout=subprocess.DEVNULL,stderr=server_errors)
            try:
                deadline = time.monotonic()+10
                while not socket_path.exists():
                    if server.poll() is not None:
                        raise RuntimeError('component endpoint startup failed')
                    if time.monotonic() >= deadline:
                        raise RuntimeError('component endpoint startup timed out')
                    time.sleep(0.01)
                for index,(name,arguments,expected_error) in enumerate(cases):
                    trace = folder/f'trace-{index}.jsonl'
                    port = ('ble-sim:' if transport == 'gatt' else 'sim:') + str(socket_path)
                    invocation = [str(binary),'--json','--port',port,'--trace',str(trace)]
                    if transport == 'gatt':
                        invocation += ['--ble-master','--passkey','000000']
                    started = time.monotonic()
                    completed = subprocess.run(invocation+arguments,cwd=ROOT,env=env,capture_output=True,text=True,timeout=65)
                    elapsed = round(time.monotonic()-started,3)
                    observed = None
                    if expected_error:
                        passed = completed.returncode != 0 and expected_error.lower() in completed.stderr.lower()
                    else:
                        try:
                            observed = json.loads(completed.stdout)
                            passed = completed.returncode == 0 and validate_success(name,observed,folder)
                        except (ValueError,KeyError,TypeError,OSError):
                            passed = False
                    proof = trace_summary(trace,transport)
                    passed = passed and proof['motor_cargo_rx_auth_commands'] == 0
                    if name in ('gear-set-5','gear-set-11','restore-baseline-8'):
                        passed = passed and proof['gear_sets'] == 1
                    if name == 'mixed-set-read-restore-batch':
                        passed = passed and proof['gear_sets'] == 2
                    if name in ('backup-collision','write-serial-guard'):
                        passed = passed and proof['gear_sets'] == 0 and proof['adjustment_sets'] == 0
                    if name == 'write-serial-guard':
                        passed = passed and not (folder/'wrong-serial.json').exists()
                    if name == 'idempotent-adjustment':
                        passed = passed and proof['adjustment_sets'] == 1 and proof['adjustment_gets'] == 2
                    if name == 'pending-adjustment':
                        passed = passed and (folder/'adjustment-before.json').exists() and proof['adjustment_sets'] == 1 and proof['adjustment_gets'] >= 3 and elapsed >= 3
                    row = dict(gateway=gateway_name,component=PROFILE['id'],transport=transport,case=name,
                               passed=passed,exit_code=completed.returncode,elapsed_seconds=elapsed,
                               expected='error: '+expected_error if expected_error else 'success',trace_contract=proof)
                    if state.exists():
                        saved = json.loads(state.read_text())
                        row['modeled_persisted_gear_count'] = bytes.fromhex(saved['gear_record'])[0]
                    if observed is not None:
                        row['observed'] = observed
                    if expected_error or not passed:
                        row['diagnostic'] = completed.stderr.strip()
                    results.append(row)
                print(f'{transport} {gateway_name}: {sum(row["passed"] for row in results)}/{len(results)} CLI cases',flush=True)
            finally:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
                server_errors.seek(0)
                errors = server_errors.read().strip()
                if errors:
                    results.append(dict(gateway=gateway_name,transport=transport,case='server-cleanup',passed=False,diagnostic=errors))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures',type=Path)
    parser.add_argument('--component-image',type=Path,required=True)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/debug/cranklink')
    parser.add_argument('--output',type=Path)
    parser.add_argument('--transport',choices=('native','gatt'),default='native')
    parser.add_argument('--gateway-id',action='append',help='restrict to one or more exact gateway profile IDs')
    parser.add_argument('--case',action='append',dest='cases',help='run selected independent cases; restore/batch write checks require the full sequence')
    parser.add_argument('--write-tests',action='store_true',help='enable exact-image gear NVM fixture and pending-actuator failure tests')
    parser.add_argument('--serve',action='store_true',help=argparse.SUPPRESS)
    parser.add_argument('--gateway',type=Path,help=argparse.SUPPRESS)
    parser.add_argument('--socket',type=Path,help=argparse.SUPPRESS)
    parser.add_argument('--state',type=Path,help=argparse.SUPPRESS)
    args = parser.parse_args()
    dependent_cases = {'gear-read-5-after-reload','restore-baseline-8','mixed-set-read-restore-batch','backup-collision'}
    if args.cases and args.write_tests and set(args.cases) & dependent_cases:
        parser.error('selected write cases depend on the complete write sequence; omit --case')
    image = args.component_image.read_bytes()
    MuUr500(image)
    if args.serve:
        factory = lambda: MuUr500(image,adjustment=-2,gears=8,allow_saves=args.write_tests,storage_path=args.state)
        server = serve_component_gatt if args.transport == 'gatt' else serve_component
        server(args.gateway,factory,args.socket)
        return
    if not args.fixtures or not args.output:
        parser.error('--fixtures and --output are required')
    manifest = args.fixtures.resolve()
    fixtures = {key:(manifest.parent/value).resolve() for key,value in json.loads(manifest.read_text()).items()}
    gateways = [profile for profile in PROFILES if profile['kind']=='gateway' and profile['id'] in fixtures and (args.transport!='gatt' or 'pce_dispatcher' in profile)]
    if args.gateway_id:
        unknown = set(args.gateway_id)-{profile['id'] for profile in gateways}
        if unknown:
            parser.error('unsupported gateway profile(s) for transport: '+', '.join(sorted(unknown)))
        gateways = [profile for profile in gateways if profile['id'] in args.gateway_id]
    if not gateways:
        parser.error('manifest has no suitable gateway profiles')
    results = []
    with ThreadPoolExecutor(max_workers=4) as pool:
        pending = [pool.submit(group,args.binary.resolve(),profile['id'],fixtures[profile['id']],args.component_image.resolve(),args.transport,args.write_tests,args.cases) for profile in gateways]
        for future in as_completed(pending):
            results.extend(future.result())
    report = dict(schema_version=2,passed=all(row['passed'] for row in results),transport=args.transport,write_tests=args.write_tests,selected_cases=args.cases,
                  binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),target=dict(id=PROFILE['id'],sha256=PROFILE['sha256']),
                  gateway_profiles=[dict(id=p['id'],sha256=p['sha256']) for p in gateways],
                  executed='stock gateway/MU request parsing, identity, PC-link, settings GET, opt-in SET validation/record staging/checksum, scheduler and replies',
                  modeled=['USB adapter or GATT/radio/PIN','discovery delivery','synthetic boot caches','virtual packet delivery'] + (['physical NVM commit and boot hydration from local JSON'] if args.write_tests else []),
                  not_validated=['actuator movement/completion','reset-to-main boot','physical EEPROM programming/timing','other MU or Di2 firmware families'],
                  cases=sorted(results,key=lambda row:(row['gateway'],row['case'])))
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(passed=report['passed'],cases=len(results),failures=sum(not row['passed'] for row in results))))
    if not report['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
