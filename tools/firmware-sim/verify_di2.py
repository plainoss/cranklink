#!/usr/bin/env python3
"""Check CLI traffic against partial battery firmware, without modeling radio teardown."""
import argparse
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
from fw_sim.di2_gatt import Di2TimeoutEndpoint
from fw_sim.gatt import read_request_line


def reproduce(binary, image, case="identity-timeout"):
    endpoint = Di2TimeoutEndpoint(image, case=case)
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
                                      '--json', 'units'] + (['--stock-only'] if case == 'stock-only' else []),
                                     capture_output=True, text=True, timeout=25)
            worker.join(8)
            if worker.is_alive():
                raise RuntimeError('offline peer did not terminate')
        finally:
            listener.close()
        if failures:
            raise RuntimeError('; '.join(failures))
    return dict(case=case, exit_code=process.returncode, diagnostic=process.stderr.strip(),
                output=process.stdout,
                battery_connected=endpoint.battery.connected, events=endpoint.events,
                firmware_sha256=endpoint.battery.assessment['sha256'],
                firmware_executions=endpoint.executions,
                full_topology_validated=False, radio_disconnect_firmware_executed=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--image', type=Path, required=True)
    parser.add_argument('--expect', choices=['stranded', 'receipt', 'radio-close', 'stock'], required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--case', choices=['identity-timeout', 'metadata-timeout', 'component-timeout', 'stock-only'],
                        default='identity-timeout')
    args = parser.parse_args()
    if (args.expect == 'stock') != (args.case == 'stock-only'):
        parser.error('stock-only requires --expect stock; other cases require stranded, receipt or radio-close')
    result = reproduce(args.binary, args.image, case=args.case)
    executions = result['firmware_executions']
    started = bool(executions and executions[0]['returned'] and executions[0]['connected'])
    free = [event['bytes'] for event in result['events'] if event['kind'] == 'free']
    pce = [event['bytes'] for event in result['events'] if event['kind'] == 'modeled-pce']
    expected_free = [] if args.case == 'metadata-timeout' else ['00011c00']
    if args.case in ('component-timeout', 'stock-only'):
        expected_free += ['00011400', '0002140d']
    if args.case == 'component-timeout':
        expected_free += ['0d329c00']
    successful_stock = args.expect == 'stock' and args.case == 'stock-only'
    if args.expect == 'receipt':
        lifecycle = (len(executions) == 2 and executions[1]['receipt_boundary']
                     and not executions[1]['returned'] and executions[1]['error'] is None
                     and not result['battery_connected']
                     and free == expected_free + ['00321000130000'] and pce[-1] == '000c00'
                     and next(i for i, e in enumerate(result['events'])
                              if e.get('bytes') == '00321000130000')
                     < next(i for i, e in enumerate(result['events'])
                            if e.get('bytes') == '000c00'))
    elif args.expect in ('radio-close', 'stock'):
        # Hardware and official client traffic analysis establish this traffic
        # policy. This partial peer deliberately does NOT implement the gateway
        # radio-disconnect callback or manufacture a firmware stop/RAM reset.
        lifecycle = (len(executions) == 1 and result['battery_connected']
                     and free == expected_free and '000c00' not in pce
                     and '0010' not in pce
                     and sum(e['kind'] == 'radio-close' for e in result['events']) == 1)
    else:
        lifecycle = (len(executions) == 1 and result['battery_connected']
                     and free == expected_free and '000c00' not in pce)
    if successful_stock:
        output = json.loads(result['output'])
        outcome = result['exit_code'] == 0 and [u['slot'] for u in output['units']] == [0, 13]
        outcome = outcome and output['units'][1]['model'] is None and 'RD-RX815' in output['units'][1]['model_candidates']
    else:
        outcome = result['exit_code'] != 0 and 'BLE response timeout' in result['diagnostic']
    result['passed'] = outcome and started and lifecycle
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print('PASS' if result['passed'] else 'FAIL', args.image.name,
          'mode retained in partial peer:', result['battery_connected'],
          '(gateway radio-disconnect firmware is not executed)')
    if not result['passed']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
