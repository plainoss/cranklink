"""Modeled radio/GATT boundary with genuine gateway and component execution.

Reuses the existing synthetic PIN peer and stock gateway PCE/routing engine.
Remote admission follows the supplied component's exact command policy; there
is no motor RX-version peer or drive-unit authentication-success substitute.
"""
import json
import os
from pathlib import Path
import socket
import sys

from .bus import Bus
from .gateway import Gateway
from .gatt import GattDevice, GattEndpoint, read_request_line
from .machine import require
from .profiles import identify


class ComponentGattDevice(GattDevice):
    def __init__(self, gateway_path, component):
        image, profile = identify(gateway_path, 'gateway')
        require('pce_dispatcher' in profile, 'component GATT needs a profiled PCE dispatcher')
        self.gateway = Gateway(image, profile)
        self.motor = component
        self.bus = Bus(self.gateway, component)
        self.gateway.restore()
        self.rx_version = b''
        self.allow_saves = bool(getattr(component, 'storage', None))
        self.radio_lifecycle = False
        self.radio_available = True
        self.active_endpoint = None
        self.connections = self.connect_attempts = 0
        self.lifecycle = []

    def connect(self, drop_replies=False, fault=None):
        return ComponentGattEndpoint(None, None, drop_replies, fault, device=self)


class ComponentGattEndpoint(GattEndpoint):
    def free(self, data):
        require(4 <= len(data) <= 10, 'unsupported component GATT packet length')
        slot, payload = data[0], data[1:]
        require(self.master and slot == self.gateway.slot,
                'component topology requires master routing and slot 13')
        require(tuple(payload[:2]) in self.motor.allowed_commands, 'component GATT command not admitted')
        if payload[:2] == b'\x32\x10':
            require(len(payload) == 6 and payload[2] in (0, 1) and payload[3:] == b'\0\0\0',
                    'only component PC owner zero is modeled')
        elif payload[:2] == b'\x32\x30':
            require(len(payload) == 6 and payload[4:] == b'\0\0', 'invalid component PC key')
        self._lifecycle('component-free-command', slot=slot, group=payload[0], command=payload[1])
        self._pump([self.gateway.send(payload)])


def serve_component_gatt(gateway_path, component_factory, socket_path):
    path = Path(socket_path).absolute()
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    old_umask = os.umask(0o077)
    try:
        listener.bind(str(path))
    finally:
        os.umask(old_umask)
    try:
        listener.listen(1)
        print(f'Component GATT ready: --port ble-sim:{path} --ble-master (synthetic PIN 000000)', flush=True)
        while True:
            connection, _ = listener.accept()
            endpoint = None
            try:
                endpoint = ComponentGattDevice(gateway_path, component_factory()).connect()
                connection.settimeout(30)
                with connection:
                    while line := read_request_line(connection):
                        failed = False
                        try:
                            response = endpoint.request(json.loads(line))
                        except (ValueError, RuntimeError, KeyError, TypeError) as error:
                            response = dict(ok=False, error=str(error))
                            failed = True
                        connection.sendall(json.dumps(response, separators=(',', ':')).encode() + b'\n')
                        if failed or endpoint.closed:
                            break
            except (OSError, RuntimeError, ValueError) as error:
                print(f'Component GATT session failed: {error}', file=sys.stderr, flush=True)
            finally:
                connection.close()
                if endpoint is not None:
                    try:
                        endpoint.close()
                    except RuntimeError as error:
                        print(f'Component GATT cleanup unconfirmed: {error}', file=sys.stderr, flush=True)
    finally:
        listener.close()
        path.unlink(missing_ok=True)
