#!/usr/bin/env python3
"""Exercise the actual native CLI against independent local firmware endpoints."""
import argparse
import concurrent.futures
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def check(binary, gateway, motor, drop=False):
    with tempfile.TemporaryDirectory(prefix='clsim-') as directory:
        socket = Path(directory) / 's'
        server = subprocess.Popen([sys.executable,str(Path(__file__).with_name('sim.py')),'serve',
                                   '--gateway',str(gateway),'--motor',str(motor),'--socket',str(socket),
                                   *(['--drop-replies'] if drop else [])],
                                  stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        result = dict(gateway=Path(gateway).name,motor=Path(motor).name,drop_replies=drop,commands=[])
        try:
            # Ready line is emitted only after bind; CLI timeouts remain active.
            ready = server.stdout.readline()
            if 'Simulator ready:' not in ready: raise RuntimeError('simulator startup failed')
            for command in ([['probe']] if drop else [['probe'],['settings','read','lamp']]):
                completed = subprocess.run([str(binary),'--json','--port',f'sim:{socket}',*command],
                                           capture_output=True,text=True,timeout=45)
                if drop:
                    if completed.returncode == 0 or 'timeout' not in completed.stderr:
                        raise RuntimeError(f'dropped response did not reach client timeout: {completed.stderr}')
                    result['commands'].append(dict(command=command,expected_timeout=True))
                else:
                    if completed.returncode:
                        if 'not a recognized motor' in completed.stderr and 'DUE6100' in str(motor):
                            result['commands'].append(dict(command=command,expected_model_rejection=True))
                            continue
                        if command[0]=='settings' and 'settings schema is not verified' in completed.stderr and 'DUE6100' in str(motor):
                            result['commands'].append(dict(command=command,expected_schema_rejection=True))
                            continue
                        raise RuntimeError(f'{gateway.name}/{motor.name} {command}: {completed.stderr}')
                    output = json.loads(completed.stdout)
                    result['commands'].append(dict(command=command,output=output))
            result['passed'] = True
        finally:
            server.terminate()
            _, errors = server.communicate(timeout=5)
            if errors.strip(): raise RuntimeError(f'simulator reported error: {errors}')
        return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--fixtures',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    args=p.parse_args()
    raw=json.loads(args.fixtures.read_text())
    fixtures={k:(args.fixtures.parent/v).resolve() for k,v in raw.items()}
    pairs=[('sc-em800-4.6.0','ep800-4.2.0'),('sc-e6100-4.1.0','ep600-4.6.0'),
           ('sc-e7000-4.1.0','ep800-4.1.11'),('sc-e7000-4.1.0','e6100-4.8.0')]
    jobs=[]
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        for g,m in pairs:
            if g in fixtures and m in fixtures:
                jobs.append(pool.submit(check,args.binary.resolve(),fixtures[g],fixtures[m]))
        if not jobs: raise ValueError('no native integration fixture pair present')
        if pairs[0][0] in fixtures and pairs[0][1] in fixtures:
            jobs.append(pool.submit(check,args.binary.resolve(),fixtures[pairs[0][0]],fixtures[pairs[0][1]],True))
        results=[job.result() for job in jobs]
    report=dict(passed=True,scope='actual native CLI; synthetic NVM and modeled M version',cases=results)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(dict(passed=True,cases=len(results))))


if __name__=='__main__':main()
