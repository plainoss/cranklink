#!/usr/bin/env python3
"""Current CLI writes against stock EP800 handlers and modeled persistent NVM.

Firmware, credentials, raw proofs and transport frames never enter the report.
The opt-in fixture remains fenced to exact EP800 D4.2.0 firmware. No hardware
EEPROM, full boot or RF behavior is claimed by this integration check.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time

from fw_sim.gatt import FREE_COMMAND, GattDevice, read_request_line
from fw_sim.machine import require


class PersistentPeer:
    def __init__(self, gateway, motor, root):
        self.gateway, self.motor, self.root = gateway, motor, root
        self.state = root/'state.json'
        self.path = root/'gatt.sock'
        self.device = self.new_device()
        self.backups = []
        self.sets = []
        self.errors = []
        self.stop = threading.Event()
        self.listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.listener.bind(str(self.path))
        self.listener.listen(1)
        self.listener.settimeout(0.2)
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    def new_device(self):
        return GattDevice(self.gateway, self.motor, storage_path=self.state, allow_saves=True)

    def observe(self, request):
        if request.get('op') != 'write' or request.get('id') != FREE_COMMAND:
            return
        raw = bytes(request.get('bytes', []))
        if raw[1:3] not in (b'\x10\x10', b'\x35\0'):
            return
        matches = []
        for path in self.backups:
            if not path.is_file():
                continue
            backup = json.loads(path.read_text())
            if (backup.get('format') == 'cranklink-setting-backup-v1'
                    and backup.get('serial') == '000000000000'
                    and (backup.get('group'), backup.get('command')) == tuple(raw[1:3])
                    and backup.get('planned_parameters') == raw[3:].hex()):
                matches.append((path, backup))
        require(len(matches) == 1, 'SET lacks one complete matching pre-existing backup')
        path, backup = matches[0]
        self.sets.append(dict(backup=path.name, setting=backup['setting'], before=backup['before'],
                             complete_backup_observed_before_set=True,
                             regulation_auth=self.device.motor.byte(self.device.motor.profile['auth_ram'])))

    def serve(self):
        while not self.stop.is_set():
            try:
                connection, _ = self.listener.accept()
            except socket.timeout:
                continue
            endpoint = None
            try:
                endpoint = self.device.connect()
                with connection:
                    while line := read_request_line(connection):
                        request = json.loads(line)
                        self.observe(request)
                        response = endpoint.request(request)
                        connection.sendall(json.dumps(response, separators=(',', ':')).encode()+b'\n')
                        if endpoint.closed:
                            break
            except (OSError, ValueError, RuntimeError, KeyError, TypeError) as error:
                self.errors.append(str(error))
            finally:
                connection.close()
                if endpoint is not None:
                    try:
                        endpoint.close()
                    except RuntimeError as error:
                        self.errors.append(str(error))

    def power_cycle(self):
        require(self.device.active_endpoint is None, 'cannot power-cycle active CLI connection')
        self.device = self.new_device()

    def close(self):
        self.stop.set()
        self.thread.join(timeout=5)
        require(not self.thread.is_alive(), 'persistent GATT server did not stop')
        self.listener.close()
        self.path.unlink(missing_ok=True)


def run(binary, gateway, motor):
    cases = []
    with tempfile.TemporaryDirectory(prefix='clwrite-') as directory:
        root = Path(directory)
        peer = PersistentPeer(gateway, motor, root)
        try:
            def command(name, arguments, backups=(), expect_error=None, expected_value=None,
                        changed=None, set_count=None):
                peer.backups = [root/path for path in backups]
                before_sets = len(peer.sets)
                before_commits = peer.device.motor.storage.commits
                env = dict(os.environ)
                env.pop('CRANKLINK_PASSKEY', None)
                env.pop('CRANKLINK_PASSKEY_FILE', None)
                started = time.monotonic()
                process = subprocess.run([str(binary), '--json', '--port', f'ble-sim:{peer.path}',
                                          '--ble-master', '--passkey', '000000', '-s', '000000000000',
                                          *arguments], cwd=root, env=env, capture_output=True,
                                         text=True, timeout=90)
                result = dict(case=name, exit_code=process.returncode,
                              seconds=round(time.monotonic()-started, 3),
                              setting_sets=len(peer.sets)-before_sets,
                              committed_records=peer.device.motor.storage.commits-before_commits)
                if expect_error:
                    result['expected_error'] = expect_error
                    result['passed'] = process.returncode != 0 and expect_error.lower() in process.stderr.lower()
                    result['passed'] = result['passed'] and result['setting_sets'] == 0 and result['committed_records'] == 0
                else:
                    result['passed'] = process.returncode == 0
                    if result['passed']:
                        value = json.loads(process.stdout)
                        if expected_value is not None:
                            result['observed_value'] = value.get('value')
                            result['passed'] = value.get('value') == expected_value
                        if changed is not None:
                            result['changed'] = value.get('changed')
                            result['passed'] = result['passed'] and value.get('changed') == changed
                        if name == 'mixed-set-read-restore-batch':
                            result['completed'] = value.get('completed')
                            result['passed'] = result['passed'] and value.get('completed') == 6
                            result['passed'] = result['passed'] and value['results'][1]['value'] == 2
                            result['passed'] = result['passed'] and value['results'][3]['value'] == 2151
                if set_count is not None:
                    result['passed'] = result['passed'] and result['setting_sets'] == set_count
                snapshot = peer.device.snapshot()
                result['cleanup_confirmed'] = not snapshot['connection_active'] and not snapshot['motor_connected']
                result['passed'] = result['passed'] and result['cleanup_confirmed'] and not peer.errors
                if not result['passed']:
                    result['diagnostic'] = process.stderr.strip()
                    result['server_errors'] = list(peer.errors)
                cases.append(result)
                print(f"{name}: {'PASS' if result['passed'] else 'FAIL'}", flush=True)
                require(result['passed'], f'current CLI persistent-write check failed: {name}: {process.stderr}')
                return result

            command('lamp-set-1', ['--no-auth', 'settings', 'set', 'lamp', '1', '--backup', 'lamp-original.json'],
                    ['lamp-original.json'], changed=True, set_count=1)
            command('lamp-read-1', ['--no-auth', 'settings', 'read', 'lamp'], expected_value=1, set_count=0)
            command('lamp-set-2', ['--no-auth', 'settings', 'set', 'lamp', '2', '--backup', 'lamp-second.json'],
                    ['lamp-second.json'], changed=True, set_count=1)
            command('lamp-restore-0', ['--no-auth', 'settings', 'restore', 'lamp-original.json', '--backup', 'lamp-restore.json'],
                    ['lamp-restore.json'], changed=True, set_count=1)
            command('tire-set-2150', ['settings', 'set', 'tire-circumference', '2150', '--backup', 'tire-original.json'],
                    ['tire-original.json'], changed=True, set_count=1)
            command('tire-read-2150', ['settings', 'read', 'tire-circumference'], expected_value=2150, set_count=0)
            command('tire-set-2151', ['settings', 'set', 'tire-circumference', '2151', '--backup', 'tire-second.json'],
                    ['tire-second.json'], changed=True, set_count=1)
            command('tire-restore-2149', ['settings', 'restore', 'tire-original.json', '--backup', 'tire-restore.json'],
                    ['tire-restore.json'], changed=True, set_count=1)
            plan = [dict(op='set', name='lamp', value=2), dict(op='read', name='lamp'),
                    dict(op='set', name='tire-circumference', value=2151), dict(op='read', name='tire-circumference'),
                    dict(op='restore', file='lamp-second.json'), dict(op='restore', file='tire-second.json')]
            (root/'plan.json').write_text(json.dumps(plan))
            command('mixed-set-read-restore-batch', ['settings', 'batch', 'plan.json', '--backup-prefix', 'batch'],
                    [f'batch.{index:03}.backup.json' for index in (1,3,5,6)], set_count=4)
            command('no-auth-protected-write', ['--no-auth', 'settings', 'set', 'tire-circumference', '2150', '--backup', 'blocked.json'],
                    expect_error='--no-auth was specified', set_count=0)
            command('existing-backup-refuses-write', ['--no-auth', 'settings', 'set', 'lamp', '1', '--backup', 'lamp-original.json'],
                    expect_error='create settings backup', set_count=0)
            before_power_cycle = peer.device.snapshot()
            require(before_power_cycle['storage']['commits'] == 10, 'unexpected persistent setting commit count')
            require(all(s['regulation_auth'] == 2 for s in peer.sets if s['setting'] == 'tire-circumference'),
                    'protected SET sent without successful stock firmware authorization')
            require(all(s['regulation_auth'] == 0 for s in peer.sets[:3]), 'no-auth lamp unexpectedly authorized')
            peer.power_cycle()
            # Both retained values differ from the boot seeds (0/2149), so a
            # fixture that ignored the saved file cannot satisfy these reads.
            command('post-power-cycle-lamp', ['--no-auth', 'settings', 'read', 'lamp'], expected_value=1, set_count=0)
            command('post-power-cycle-tire', ['settings', 'read', 'tire-circumference'], expected_value=2150, set_count=0)
            return dict(passed=True, cases=cases, backup_before_set_observations=peer.sets,
                        before_power_cycle=before_power_cycle, after_power_cycle=peer.device.snapshot())
        except Exception as error:
            return dict(passed=False, cases=cases, error=str(error),
                        backup_before_set_observations=peer.sets, server_errors=list(peer.errors))
        finally:
            peer.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--fixtures', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    manifest = args.fixtures.resolve()
    fixtures = {k:(manifest.parent/v).resolve() for k,v in json.loads(manifest.read_text()).items()}
    report = run(args.binary.resolve(), fixtures['sc-em800-4.6.0'], fixtures['ep800-4.2.0'])
    report.update(client_binary_sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                  scope='actual current Rust CLI; unchanged stock SC-EM800/EP800 firmware functions; modeled EEPROM commit/boot hydration and GATT radio',
                  not_validated=['physical EEPROM and electrical power loss', 'full reset boot', 'RF/pairing',
                                 'other firmware versions', 'newly catalogued component firmware'])
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(dict(passed=report['passed'], cases=len(report['cases']))))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
