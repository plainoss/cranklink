#!/usr/bin/env python3
"""JSONL bridge for an actual browser/WASM client and a powered virtual bike.

The process retains the same firmware/RAM/storage across GATT reconnects.
Radio connection-mode shutdown is optional and explicitly modeled. stdout is
reserved for request/response JSON; no challenge/proof bytes enter snapshots.
"""
import argparse
import json
import sys
from pathlib import Path
from fw_sim.gatt import GattDevice
from fw_sim.machine import require


class BrowserPeer:
    def __init__(self, gateway, motor, state=None, radio_lifecycle=False):
        self.gateway_path, self.motor_path = gateway,motor
        self.state_path, self.radio_lifecycle = state,radio_lifecycle
        self.device = self._new_device()
        self.endpoint = None

    def _new_device(self):
        return GattDevice(self.gateway_path,self.motor_path,storage_path=self.state_path,
                          allow_saves=True,radio_lifecycle=self.radio_lifecycle)

    def request(self, message):
        require(isinstance(message,dict), 'browser peer request must be an object')
        operation = message.get('op')
        notifications = []
        if operation == 'connect':
            require(self.endpoint is None or self.endpoint.closed, 'browser GATT connection already active')
            self.endpoint = self.device.connect()
            result = dict(connection=self.endpoint.connection_id)
        elif operation == 'disconnect':
            endpoint, self.endpoint = self.endpoint,None
            if endpoint is not None: endpoint.close()
            result = dict(disconnected=True)
        elif operation == 'request':
            require(self.endpoint is not None and not self.endpoint.closed, 'browser GATT is not connected')
            try:
                result = self.endpoint.request(message.get('request'))
                notifications = list(self.endpoint.queue)
                self.endpoint.queue.clear()
            except Exception:
                # The real browser transport fences a rejected GATT operation.
                # Keep the powered device/storage; invalidate this connection.
                endpoint, self.endpoint = self.endpoint,None
                endpoint.close()
                raise
        elif operation == 'snapshot':
            result = self.device.snapshot()
        elif operation == 'reenable':
            self.device.reenable_radio()
            result = dict(radio_available=True)
        elif operation == 'power_cycle':
            endpoint, self.endpoint = self.endpoint,None
            if endpoint is not None: endpoint.close()
            history, connections, attempts = list(self.device.lifecycle),self.device.connections,self.device.connect_attempts
            self.device = self._new_device()
            self.device.connections = connections
            self.device.connect_attempts = attempts
            self.device.lifecycle = history+[dict(kind='modeled-controlled-power-cycle')]
            result = self.device.snapshot()
        else:
            raise RuntimeError('unknown browser peer operation')
        return dict(id=message.get('id'),ok=True,result=result,notifications=notifications)

    def close(self):
        if self.endpoint is not None:
            self.endpoint.close()
            self.endpoint = None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway',required=True,type=Path)
    parser.add_argument('--motor',required=True,type=Path)
    parser.add_argument('--state',type=Path,help='modeled storage JSON retained across process/power cycles')
    parser.add_argument('--radio-lifecycle',action='store_true',
                        help='model connection mode ending on actual gateway PC-stop receipt; reenable explicitly')
    args = parser.parse_args()
    peer = BrowserPeer(args.gateway,args.motor,args.state,args.radio_lifecycle)
    try:
        while line := sys.stdin.buffer.readline(4097):
            message = None
            try:
                require(len(line) <= 4096 and line.endswith(b'\n'), 'browser peer request line exceeds limit')
                message = json.loads(line)
                response = peer.request(message)
            except (ValueError,RuntimeError,KeyError,TypeError,OSError) as error:
                response = dict(id=message.get('id') if isinstance(message,dict) else None,
                                ok=False,error=str(error),notifications=[])
            print(json.dumps(response,separators=(',',':')),flush=True)
            if len(line)>4096 or not line.endswith(b'\n'): break
    finally:
        peer.close()


if __name__=='__main__': main()
