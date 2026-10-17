#!/usr/bin/env python3
"""Exercise the actual Rust BLE client against isolated modeled GATT peers.

Requires private decoded images supplied by a local fixture manifest. The report
contains profile IDs, image hashes and client results, never fixture paths or
challenge/proof material. No hardware or network is used.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
import json
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from fw_sim.gatt import GattEndpoint
from fw_sim.profiles import PROFILES

ROOT = Path(__file__).resolve().parents[2]
PAIRS = [('sc-em800-4.0.0','ep800-4.1.10'),
         ('sc-em800-4.4.0','ep800-4.2.0'),
         ('sc-em800-4.6.0','ep600-4.6.0')]
COMMANDS = [('units',),('probe',),('settings','read','lamp'),('settings','read','destination')]


def group(binary, fixtures, pair, cases):
    results = []
    with tempfile.TemporaryDirectory(prefix='cranklink-gatt-') as folder:
        socket_path = Path(folder)/'peer.sock'
        fault = cases[0].get('fault')
        dropped = cases[0].get('drop_replies',False)
        code = ('import sys;from fw_sim.gatt import serve;'
                'serve(sys.argv[1],sys.argv[2],sys.argv[3],sys.argv[4]=="1",'
                'None if sys.argv[5]=="none" else sys.argv[5])')
        env = dict(os.environ, PYTHONPATH=str(ROOT/'tools/firmware-sim'),CRANKLINK_PASSKEY='000000')
        with tempfile.TemporaryFile(mode='w+') as errors:
            server = subprocess.Popen([sys.executable,'-c',code,str(fixtures[pair[0]]),
                                       str(fixtures[pair[1]]),str(socket_path),
                                       '1' if dropped else '0',fault or 'none'],
                                      cwd=ROOT,env=env,stdout=subprocess.DEVNULL,stderr=errors)
            try:
                deadline = time.monotonic()+5
                while not socket_path.exists():
                    if server.poll() is not None: raise RuntimeError('modeled GATT server failed to start')
                    if time.monotonic() >= deadline: raise RuntimeError('modeled GATT server startup timed out')
                    time.sleep(0.01)
                for case in cases:
                    args = [str(binary),'--port',f'ble-sim:{socket_path}','--json']
                    args += ['--ble-master=normal'] if case.get('normal') else ['--ble-master']
                    if case.get('wrong_pin'): args += ['--passkey','000001']
                    trace_path = Path(folder)/('trace-'+str(len(results))+'.jsonl')
                    args += ['--trace',str(trace_path)]
                    args += list(case['command'])
                    start = time.monotonic()
                    try:
                        process = subprocess.run(args,cwd=ROOT,env=env,capture_output=True,text=True,timeout=65)
                        elapsed = round(time.monotonic()-start,3)
                        observed = None
                        error = process.stderr.strip()
                        expected = case.get('expect_error')
                        if expected:
                            passed = process.returncode != 0 and expected in error
                        else:
                            try: observed = json.loads(process.stdout)
                            except (ValueError,TypeError): observed = None
                            passed = process.returncode == 0 and isinstance(observed,dict)
                            if passed and case['command'][0]=='units':
                                passed = len(observed.get('units',[]))==1 and observed['units'][0].get('slot')==13
                            elif passed and case['command'][0]=='probe':
                                passed = observed.get('slot')==13 and all(k in observed for k in ('cargo','dcas_version','rx_version','serial'))
                            elif passed:
                                passed = 'value' in observed and bool(observed.get('raw'))
                        result = dict(gateway=pair[0],motor=pair[1],case=case['name'],
                                      command=list(case['command']),passed=passed,
                                      expected='error: '+expected if expected else 'success',
                                      exit_code=process.returncode,elapsed_seconds=elapsed)
                        if trace_path.exists():
                            frames = [json.loads(line) for line in trace_path.read_text().splitlines() if line]
                            tx = [f for f in frames if f.get('direction')=='tx']
                            pce_starts = sum(f.get('characteristic','').startswith('00002afa-') and f.get('bytes')=='000c01' for f in tx)
                            free_starts = sum(f.get('characteristic','').startswith('00002afe-') and f.get('bytes','').startswith('0d321001') for f in tx)
                            result['pc_start_contract'] = dict(gateway_starts=pce_starts,explicit_motor_starts=free_starts,total=pce_starts+free_starts)
                        if observed is not None: result['observed']=observed
                        if expected or not passed: result['diagnostic']=error
                    except subprocess.TimeoutExpired:
                        result = dict(gateway=pair[0],motor=pair[1],case=case['name'],
                                      command=list(case['command']),passed=False,error='CLI process timed out')
                    results.append(result)
                    print(f"{pair[0]} / {pair[1]} {case['name']}: {'PASS' if result['passed'] else 'FAIL'}",flush=True)
            finally:
                server.terminate()
                try: server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill();server.wait()
                errors.seek(0)
                server_diagnostics = errors.read().strip()
                if server_diagnostics and results:
                    results[-1]['server_diagnostics'] = server_diagnostics
                    results[-1]['passed'] = False
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures',type=Path,required=True)
    parser.add_argument('--binary',type=Path,default=ROOT/'target/debug/cranklink')
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    manifest = args.fixtures.resolve()
    fixtures = {k:(manifest.parent/v).resolve() for k,v in json.loads(manifest.read_text()).items()}
    for pair in PAIRS:
        for name in pair:
            if name not in fixtures: raise ValueError(f'fixture manifest lacks {name}')
    positive = [dict(name=' '.join(command),command=command) for command in COMMANDS]
    work = [(pair,positive) for pair in PAIRS]
    baseline = PAIRS[-1]
    work += [(baseline,[dict(name='wrong synthetic PIN',command=('probe',),wrong_pin=True,
                             expect_error='synthetic BLE challenge rejected')]),
             (baseline,[dict(name='normal topology unsupported',command=('units',),normal=True,
                             expect_error='logical bus-master-zero discovery is not implemented')]),
             (baseline,[dict(name='dropped normal responses',command=('probe',),drop_replies=True,
                             expect_error='BLE response timeout')]),
             (baseline,[dict(name='stale FREE notification before actual response',command=('probe',),fault='stale-free')]),
             (baseline,[dict(name='duplicate FREE notifications',command=('probe',),fault='duplicate-free')])]
    results = []
    with ThreadPoolExecutor(max_workers=4) as pool:
        pending = [pool.submit(group,args.binary.resolve(),fixtures,pair,cases) for pair,cases in work]
        for future in as_completed(pending): results.extend(future.result())
    lifecycle_gateways = [p for p in PROFILES if p['id'] in fixtures and p['kind']=='gateway' and 'pce_dispatcher' in p]
    lifecycle_motors = [p for p in PROFILES if p['id'] in fixtures and p['kind']=='motor']
    lifecycle_pairs = len(lifecycle_gateways)*len(lifecycle_motors)
    lifecycle_env = dict(os.environ,CRANKLINK_SIM_FIXTURES=str(manifest))
    lifecycle = subprocess.run([sys.executable,'-m','unittest','discover','-s','tools/firmware-sim',
                                '-p','test_gatt.py'],cwd=ROOT,env=lifecycle_env,
                               capture_output=True,text=True,timeout=120)
    lifecycle_report = dict(passed=lifecycle.returncode==0 and lifecycle_pairs>0,
                            gateway_motor_pairs=lifecycle_pairs,
                            profiles=[dict(id=p['id'],sha256=p['sha256']) for p in lifecycle_gateways+lifecycle_motors],
                            repeated_gets_per_pair=20,total_repeated_gets=lifecycle_pairs*20,
                            checks='modeled auth; actual PCE discovery/link; repeated firmware GETs; actual post-handoff motor stop; radio boundary negative tests',
                            diagnostic=lifecycle.stderr.strip())
    used = {name for pair in PAIRS for name in pair}
    report = dict(schema_version=1,client_binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),passed=all(r['passed'] for r in results) and lifecycle_report['passed'],
                  stock_lifecycle_validation=lifecycle_report,
                  hardware_access=False,network_access=False,
                  level='actual Cranklink BLE client with modeled GATT/radio and stock firmware functions',
                  modeled=list(GattEndpoint.modeled),
                  not_validated=['physical BLE/pairing/encryption','motor M firmware execution',
                                 'normal/slave logical-master-zero discovery','reset/boot','writes/firmware update',
                                 'persistent real-device NVM values','physical timing'],
                  profiles=[dict(id=p['id'],sha256=p['sha256']) for p in PROFILES if p['id'] in used],
                  cases=sorted(results,key=lambda r:(r['gateway'],r['motor'],r['case'])))
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(passed=report['passed'],cases=len(results),failures=sum(not r['passed'] for r in results))))
    if not report['passed']: raise SystemExit(1)


if __name__=='__main__': main()
