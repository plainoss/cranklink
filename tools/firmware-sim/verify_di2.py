#!/usr/bin/env python3
"""Run the real CLI against the offline battery-firmware timeout reproduction."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
from fw_sim.di2_gatt import Di2TimeoutEndpoint
from fw_sim.gatt import read_request_line


def reproduce(binary, image):
    endpoint = Di2TimeoutEndpoint(image)
    failures = []
    with tempfile.TemporaryDirectory(prefix='di2-offline-') as folder:
        path = Path(folder) / 'peer.sock'
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(path))
        listener.listen(1)
        listener.settimeout(15)
        def serve():
            try:
                connection, _ = listener.accept()
                with connection:
                    while line := read_request_line(connection):
                        response = endpoint.request(json.loads(line))
                        connection.sendall(json.dumps(response).encode() + b'\n')
                        if endpoint.closed:
                            break
            except Exception as error:
                failures.append(str(error))
            finally:
                endpoint.close()
        worker = threading.Thread(target=serve, daemon=True)
        worker.start()
        try:
            process = subprocess.run([str(binary.resolve()), '--port', 'ble-sim:' + str(path),
                                      '--json', 'units'], capture_output=True, text=True, timeout=25)
            worker.join(8)
            if worker.is_alive():
                raise RuntimeError('offline peer did not terminate')
        finally:
            listener.close()
        if failures:
            raise RuntimeError('; '.join(failures))
    return dict(exit_code=process.returncode, diagnostic=process.stderr.strip(),
                battery_connected=endpoint.battery.connected, events=endpoint.events,
                firmware_sha256=endpoint.battery.assessment['sha256'],
                firmware_executions=endpoint.executions,
                full_topology_validated=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--image', type=Path, required=True)
    parser.add_argument('--expect', choices=['stranded', 'receipt'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    result = reproduce(args.binary, args.image)
    executions = result['firmware_executions']
    started = bool(executions and executions[0]['returned'] and executions[0]['connected'])
    free = [event['bytes'] for event in result['events'] if event['kind'] == 'free']
    pce = [event['bytes'] for event in result['events'] if event['kind'] == 'modeled-pce']
    if args.expect == 'receipt':
        lifecycle = (len(executions) == 2 and executions[1]['receipt_boundary']
                     and not executions[1]['returned'] and executions[1]['error'] is None
                     and not result['battery_connected']
                     and free == ['00011c00', '00321000130000'] and pce[-1] == '000c00'
                     and next(i for i, e in enumerate(result['events'])
                              if e.get('bytes') == '00321000130000')
                     < next(i for i, e in enumerate(result['events'])
                            if e.get('bytes') == '000c00'))
    else:
        lifecycle = (len(executions) == 1 and result['battery_connected']
                     and free == ['00011c00'] and '000c00' not in pce)
    result['passed'] = (result['exit_code'] != 0 and 'BLE response timeout' in result['diagnostic']
                        and started and lifecycle)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print('PASS' if result['passed'] else 'FAIL', args.image.name,
          'battery session active after timeout:', result['battery_connected'])
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
