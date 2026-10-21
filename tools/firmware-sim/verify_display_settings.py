#!/usr/bin/env python3
"""Current native/BLE CLI display saves against genuine gateway/target firmware.

Physical NVM commit and boot cache hydration are declared simulator boundaries.
Every connection gets a fresh target CPU and reloads its exact-hash state file.
The report contains synthetic settings and counts, never firmware/proof bytes.
"""
import argparse
from collections import deque
from concurrent.futures import ProcessPoolExecutor, as_completed
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading

from fw_sim.component_endpoint import ComponentEndpoint
from fw_sim.component_gatt import ComponentGattDevice
from fw_sim.components_display import Display, DISPLAY_PROFILES, WRITES
from fw_sim.gatt import FREE_COMMAND, FREE_RESPONSE, read_request_line
from fw_sim.machine import require
from fw_sim.profiles import PROFILES, identify
from fw_sim.wire import Decoder, encode

ROOT = Path(__file__).resolve().parents[2]


class Peer:
    def __init__(self, gateway, target, transport, root):
        self.gateway, self.target, self.transport, self.root = gateway, target, transport, root
        self.model = identify(target, 'gateway')[1]['model']
        self.state, self.path = root/'state.json', root/'peer.sock'
        self.backups, self.sets, self.sessions, self.errors = [], [], [], []
        self.drop_next_set_ack = False
        self.dropped_set_acks = 0
        self.stop = threading.Event()
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        self.listener.listen(1)
        self.listener.settimeout(0.2)
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def audit(self, payload):
        if len(payload) < 3:
            return
        group, command = payload[1:3]
        require((group, command) not in ((1, 0x84), (0x16, 0x7c), (0x16, 0xd8), (0x33, 0x68)),
                'component client issued a motor-only operation')
        if (group, command) not in WRITES:
            return
        matches = []
        for path in self.backups:
            if path.is_file():
                backup = json.loads(path.read_text())
                if (backup.get('format') == 'cranklink-setting-backup-v1'
                        and backup.get('settings_model') == self.model
                        and backup.get('serial') == '000000000000'
                        and (backup.get('group'), backup.get('command')) == (group, command)
                        and backup.get('planned_parameters') == payload[3:].hex()):
                    matches.append(path)
        require(len(matches) == 1, 'display SET lacks a unique complete pre-existing backup')
        self.sets.append(dict(backup=matches[0].name, group=group, command=command,
                             complete_backup_observed_before_set=True))
        return group, command

    def serve(self):
        while not self.stop.is_set():
            try:
                connection, _ = self.listener.accept()
            except socket.timeout:
                continue
            endpoint = component = None
            try:
                component = Display.from_path(self.target, allow_writes=True, storage_path=self.state)
                if self.transport == 'gatt':
                    endpoint = ComponentGattDevice(self.gateway, component).connect()
                    with connection:
                        while line := read_request_line(connection):
                            request = json.loads(line)
                            setter = None
                            if request.get('op') == 'write' and request.get('id') == FREE_COMMAND:
                                setter = self.audit(bytes(request['bytes']))
                            response = endpoint.request(request)
                            if setter and self.drop_next_set_ack:
                                group, command = setter
                                kept = deque(item for item in endpoint.queue
                                             if not (item['characteristic'] == FREE_RESPONSE
                                                     and bytes(item['value'][:3]) == bytes([13, group, command+2])))
                                require(len(kept) < len(endpoint.queue), 'no genuine display ACK to drop')
                                endpoint.queue = kept
                                self.drop_next_set_ack = False
                                self.dropped_set_acks += 1
                            connection.sendall(json.dumps(response, separators=(',', ':')).encode()+b'\n')
                            if endpoint.closed:
                                break
                else:
                    endpoint = ComponentEndpoint(self.gateway, component)
                    decoder = Decoder()
                    with connection:
                        while chunk := connection.recv(4096):
                            for control, payload in decoder.feed(chunk):
                                setter = None
                                if control == 0x48:
                                    setter = self.audit(payload)
                                replies = endpoint.request(control, payload)
                                if setter and self.drop_next_set_ack:
                                    group, command = setter
                                    require(any(code == 0x48 and body[:3] == bytes([0, group, command+2])
                                                for code, body in replies), 'no genuine display ACK to drop')
                                    replies = []
                                    self.drop_next_set_ack = False
                                    self.dropped_set_acks += 1
                                for code, body in replies:
                                    connection.sendall(encode(code, body))
            except (OSError, ValueError, RuntimeError, KeyError, TypeError) as error:
                self.errors.append(str(error))
            finally:
                connection.close()
                if endpoint is not None:
                    try:
                        endpoint.close()
                        self.sessions.append(dict(cleanup_confirmed=not component.connected,
                                                  target_instructions=component.instructions,
                                                  gateway_instructions=endpoint.gateway.instructions,
                                                  committed_records=component.commits))
                    except RuntimeError as error:
                        self.errors.append(str(error))

    def close(self):
        self.stop.set()
        self.thread.join(timeout=5)
        require(not self.thread.is_alive(), 'display peer did not stop')
        self.listener.close()
        self.path.unlink(missing_ok=True)


def run(binary, gateway_name, gateway, target_name, target, transport, fault_tests=False):
    cases = []
    model = identify(target, 'gateway')[1]['model']
    with tempfile.TemporaryDirectory(prefix='cranklink-display-') as folder:
        folder = Path(folder)
        peer = Peer(gateway, target, transport, folder)
        env = dict(os.environ)
        env.pop('CRANKLINK_PASSKEY', None)
        base = [str(binary), '--json', '--port', f'{"ble-sim" if transport == "gatt" else "sim"}:{peer.path}',
                '-c', model, '-s', '000000000000']
        if transport == 'gatt':
            base += ['--ble-master', '--passkey', '000000']
        try:
            def command(name, arguments, backups=(), expected=None, error=None, set_count=None):
                peer.backups = [folder/path for path in backups]
                before = len(peer.sets)
                process = subprocess.run([*base, *arguments], cwd=folder, env=env,
                                         capture_output=True, text=True, timeout=45)
                observed = None
                if error:
                    passed = process.returncode != 0 and error in process.stderr
                else:
                    try:
                        observed = json.loads(process.stdout)
                    except ValueError:
                        pass
                    passed = process.returncode == 0 and isinstance(observed, dict)
                    if passed and expected is not None:
                        passed = observed.get('value') == expected
                row = dict(case=name, passed=passed, setting_sets=len(peer.sets)-before,
                           exit_code=process.returncode, expected_error=error)
                if set_count is not None:
                    row['passed'] = row['passed'] and row['setting_sets'] == set_count
                if observed is not None:
                    row['observed'] = observed
                if not row['passed']:
                    row['diagnostic'] = process.stderr.strip()
                cases.append(row)
                require(row['passed'], f'{name}: {process.stderr.strip()}')
                return observed

            probe = command('component-probe', ['probe'], set_count=0)
            require(probe['cargo'] == model and probe['rx_version'] == 'not applicable', 'wrong display probe identity')
            snapshot = command('initial-snapshot', ['settings', 'read'], set_count=0)
            require(snapshot['unavailable'] == {}, 'incomplete display settings snapshot')
            require(snapshot['identity']['model_unit'] == 2, 'wrong component unit metadata')
            originals = {name: item['value'] for name, item in snapshot['values'].items()}
            planned = {'display-beep': 0, 'display-distance-unit': 1, 'display-language': 2}
            if 'display-brightness' in originals:
                planned['display-brightness'] = 100
            if 'display-backlight' in originals:
                planned['display-backlight'] = 2
            require(planned.keys() == originals.keys(), 'uncovered display schema fields')
            plan = [dict(op='set', name=name, value=value) for name, value in planned.items()] + [dict(op='read')]
            (folder/'save-plan.json').write_text(json.dumps(plan))
            backups = [f'save.{index:03}.backup.json' for index in range(1, len(planned)+1)]
            saved = command('mixed-setting-batch', ['settings', 'batch', 'save-plan.json', '--backup-prefix', 'save'], backups, set_count=len(planned))
            require(saved['completed'] == len(planned)+1 and len(saved['results']) == len(planned)+1,
                    'incomplete save batch')
            require({name: item['value'] for name, item in saved['results'][-1]['values'].items()} == planned, 'stock display batch readback differs')
            # A fresh target CPU reloads the modeled disk record for every CLI
            # invocation. Each read below is also a reboot-cache regression.
            for name, value in planned.items():
                command('reload-'+name, ['settings', 'read', name], expected=value, set_count=0)
            command('independent-language-change', ['settings', 'set', 'display-language', '3', '--backup', 'language.json'], ['language.json'], set_count=1)
            command('restore-beep-preserves-language', ['settings', 'restore', backups[0], '--backup', 'beep-restore.json'], ['beep-restore.json'], set_count=1)
            command('language-still-3', ['settings', 'read', 'display-language'], expected=3, set_count=0)
            restore_plan = [dict(op='restore', file=path) for path in backups] + [dict(op='read')]
            (folder/'restore-plan.json').write_text(json.dumps(restore_plan))
            restored = command('restore-batch', ['settings', 'batch', 'restore-plan.json', '--backup-prefix', 'restore'],
                               [f'restore.{index:03}.backup.json' for index in range(1, len(planned)+1)], set_count=len(planned)-1)
            require(restored['completed'] == len(planned)+1 and len(restored['results']) == len(planned)+1,
                    'incomplete restore batch')
            require({name: item['value'] for name, item in restored['results'][-1]['values'].items()} == originals,
                    'restore did not preserve original stock display values')
            command('invalid-language', ['settings', 'set', 'display-language', '6', '--backup', 'invalid.json'],
                    error='must be 0..=5', set_count=0)
            require(not (folder/'invalid.json').exists(), 'invalid input created backup')
            command('existing-backup-refused', ['settings', 'set', 'display-language', '1', '--backup', 'language.json'],
                    error='create settings backup', set_count=0)
            if fault_tests:
                peer.drop_next_set_ack = True
                command('lost-ack-after-stock-commit', ['settings', 'set', 'display-language', '4', '--backup', 'lost-ack.json'],
                        ['lost-ack.json'], error='outcome may be uncertain', set_count=1)
                require(peer.dropped_set_acks == 1, 'lost ACK was not injected after the stock setter')
                command('reload-proves-uncertain-write-applied', ['settings', 'read', 'display-language'], expected=4, set_count=0)
                command('restore-uncertain-write', ['settings', 'restore', 'lost-ack.json', '--backup', 'lost-ack-restore.json'],
                        ['lost-ack-restore.json'], set_count=1)
            final = command('final-reload-snapshot', ['settings', 'read'], set_count=0)
            require({name: item['value'] for name, item in final['values'].items()} == originals, 'new CPU reload lost restore')
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            cases.append(dict(case='verification-invariant', passed=False, diagnostic=str(error)))
        finally:
            peer.close()
        passed = all(row['passed'] for row in cases) and not peer.errors and all(row['cleanup_confirmed'] for row in peer.sessions)
        result = dict(gateway=gateway_name, target=target_name, transport=transport, passed=passed,
                      cases=cases, settings_sets=len(peer.sets), backups_observed_before_set=peer.sets,
                      modeled_dropped_set_acks=peer.dropped_set_acks,
                      sessions=peer.sessions, errors=peer.errors)
        print(f'{gateway_name}/{target_name}/{transport}: {"PASS" if passed else "FAIL"} ({len(cases)} CLI cases)', flush=True)
        return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--binary', type=Path, default=ROOT/'target/debug/cranklink')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--gateway-id', action='append')
    parser.add_argument('--target-id', action='append')
    parser.add_argument('--fault-tests', action='store_true', help='drop a genuine SET ACK after committing; require backup and uncertain-outcome reporting')
    args = parser.parse_args()
    manifest = args.fixtures.resolve()
    fixtures = {key: (manifest.parent/value).resolve() for key, value in json.loads(manifest.read_text()).items()}
    gateways = [p for p in PROFILES if p['kind'] == 'gateway' and p['id'] in fixtures
                and (not args.gateway_id or p['id'] in args.gateway_id)]
    targets = [name for name in DISPLAY_PROFILES if name in fixtures
               and (not args.target_id or name in args.target_id)]
    require(gateways and targets, 'no executable display targets/gateways supplied')
    for name in {p['id'] for p in gateways} | set(targets):
        require(identify(fixtures[name], 'gateway')[1]['id'] == name,
                f'fixture {name} identifies as a different firmware profile')
    jobs = [(p, target, transport) for p in gateways for target in targets
            for transport in ('native', 'gatt') if transport == 'native' or 'pce_dispatcher' in p]
    results = []
    binary_bytes = args.binary.read_bytes()
    binary_sha256 = hashlib.sha256(binary_bytes).hexdigest()
    # Per-instruction Unicorn hooks execute Python. Independent processes avoid
    # GIL contention turning simulator scheduling into false client timeouts.
    # Freeze the executable so a concurrent local rebuild cannot misattribute
    # later invocations to the final on-disk binary's hash.
    with tempfile.TemporaryDirectory(prefix='cranklink-display-binary-') as folder:
        binary = Path(folder)/'cranklink'
        binary.write_bytes(binary_bytes)
        binary.chmod(0o700)
        with ProcessPoolExecutor(max_workers=4) as pool:
            pending = {pool.submit(run, binary, p['id'], fixtures[p['id']], target, fixtures[target], transport, args.fault_tests):
                       (p['id'], target, transport) for p, target, transport in jobs}
            for future in as_completed(pending):
                try:
                    results.append(future.result())
                except Exception as error:
                    gateway, target, transport = pending[future]
                    results.append(dict(gateway=gateway, target=target, transport=transport,
                                        passed=False, cases=[], errors=[str(error)]))
    report = dict(schema_version=1, passed=all(row['passed'] for row in results),
                  binary_sha256=binary_sha256,
                  executed='current CLI plus stock gateway and display identity, PC-link, settings GET/SET, checksum staging and replies',
                  modeled=['USB or GATT/radio/PIN boundary', 'discovery delivery', 'boot readiness and setting caches', 'physical EEPROM commit/ready and disk reload hydration'],
                  not_validated=['board reset/full boot', 'display hardware', 'RF or physical EEPROM/timing'],
                  target_profiles=[dict(id=name, sha256=DISPLAY_PROFILES[name]['sha256']) for name in targets],
                  results=sorted(results, key=lambda row: (row['gateway'], row['target'], row['transport'])))
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(passed=report['passed'], configurations=len(results), cli_cases=sum(len(row['cases']) for row in results))))
    if not report['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
