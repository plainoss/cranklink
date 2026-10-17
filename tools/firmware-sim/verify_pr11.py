#!/usr/bin/env python3
"""A/B PR11's initial master-start contract against real firmware endpoints.

Checks pinned repository versions with only a local GATT transport injected.
No repository branch/files are switched; builds and traces stay in a temp dir.
This is a sequence-contract check, not a reproduction of browser settings saves.
"""
import argparse
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
BASE='0cfb180f576544d75a63a7fbe36f38670ae8679c'
FIXED='158e8f5bc8d5e379c08ee3fa4ca4d59c9af10789'


def source_at(ref, target):
    archive=subprocess.run(['git','archive',ref,'Cargo.toml','Cargo.lock','src'],cwd=ROOT,
                           check=True,capture_output=True).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        for entry in tar:
            if not entry.isfile(): continue
            out=target/entry.name
            if not out.resolve().is_relative_to(target.resolve()): raise ValueError('unsafe archive path')
            out.parent.mkdir(parents=True,exist_ok=True)
            out.write_bytes(tar.extractfile(entry).read())
    # This adapter is the only injected behavior. Protocol/session code remains
    # from the pinned revision, including its ownership/receipt handling.
    shutil.copyfile(ROOT/'src/ble/simulator.rs',target/'src/ble/simulator.rs')
    p=target/'src/ble.rs';text=p.read_text()
    text=text.replace('mod native;', 'mod native;\n#[cfg(all(unix,not(target_arch="wasm32")))]\nmod simulator;',1)
    text=text.replace('port == "webble" || port.starts_with("ble:")',
                      'port == "webble" || port.starts_with("ble:") || port.starts_with("ble-sim:")',1)
    declaration='pub fn open(port: &str, trace: Option<&Path>, passkey: Option<String>) -> Result<BleSession> {'
    if declaration not in text: raise ValueError('unexpected pinned source API')
    text=text.replace(declaration,declaration+'''
    #[cfg(all(unix,not(target_arch="wasm32")))]
    if let Some(path)=port.strip_prefix("ble-sim:") {
        return BleSession::new(Box::new(simulator::SimulatorGatt::open(path)?.with_passkey(passkey)),trace);
    }
''',1);p.write_text(text)


def check(ref, binary, gateway, motor, directory):
    socket=directory/'gatt.sock';trace=directory/'trace.jsonl'
    server=subprocess.Popen([sys.executable,str(ROOT/'tools/firmware-sim/sim.py'),'serve','--gatt',
                             '--gateway',str(gateway),'--motor',str(motor),'--socket',str(socket)],
                            stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
    try:
        ready=server.stdout.readline()
        if 'ready:' not in ready: raise RuntimeError('GATT startup failed')
        result=subprocess.run([str(binary),'--json','--port',f'ble-sim:{socket}',
                               '--ble-master','--trace',str(trace),'probe'],
                              text=True,capture_output=True,timeout=40)
        if result.returncode: raise RuntimeError(result.stderr)
        packets=[json.loads(line) for line in trace.read_text().splitlines()]
        sent=[p for p in packets if p['direction']=='tx']
        broadcasts=sum(p['characteristic'].startswith('00002afa') and p['bytes']=='000c01' for p in sent)
        duplicates=sum(p['characteristic'].startswith('00002afe') and p['bytes'].startswith('0d321001') for p in sent)
        return dict(revision=ref,probe_passed=True,gateway_start_requests=broadcasts,
                    extra_free_motor_start_requests=duplicates,
                    one_initial_start_contract_passed=broadcasts==1 and duplicates==0,
                    trace_sha256=hashlib.sha256(trace.read_bytes()).hexdigest())
    finally:
        server.terminate()
        _,errors=server.communicate(timeout=5)
        if errors.strip(): raise RuntimeError(errors)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',type=Path,required=True)
    parser.add_argument('--motor',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='cl-pr11-') as root:
        root=Path(root);results=[]
        for name,ref in [('before',BASE),('after',FIXED)]:
            source=root/name;source.mkdir();source_at(ref,source)
            build=subprocess.run(['cargo','build','--locked','--no-default-features','--bin','cranklink',
                                  '--target-dir',str(root/'target')],cwd=source,capture_output=True,text=True)
            if build.returncode: raise RuntimeError(build.stderr)
            run_dir=root/(name+'-run');run_dir.mkdir()
            results.append(check(ref,root/'target/debug/cranklink',args.gateway.resolve(),args.motor.resolve(),run_dir))
        if results[0]['one_initial_start_contract_passed'] or not results[1]['one_initial_start_contract_passed']:
            raise RuntimeError(f'PR11 contract did not distinguish revisions: {results}')
    report=dict(passed=True,pr='https://github.com/plainoss/cranklink/pull/11',cases=results,
                conclusion='Read-only probe passes both revisions. Added start-count contract fails before PR11 and passes after.',
                not_reproduced=['repeated browser saves','privileged write/auth/NVM behavior','cached browser PIN clearing','RF reliability'])
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))


if __name__=='__main__': main()
