"""Native CLI adapter endpoint with stock gateway and a supplied component.

The component must expose receive(payload, source), connected, and an explicit
allowed_commands set. Its implementation owns firmware execution and declares
any modeled boundaries. No motor product/RX-version behavior peer is installed.
"""
import os
from pathlib import Path
import socket
import sys

from .bus import Bus
from .endpoint import Endpoint
from .gateway import Gateway
from .machine import require
from .profiles import identify
from .wire import Decoder, encode


class ComponentEndpoint(Endpoint):
    def __init__(self, gateway_path, component, drop_replies=False):
        image, profile = identify(gateway_path, 'gateway')
        self.gateway = Gateway(image, profile)
        # Bus/Endpoint use the historical attribute name for the remote peer.
        self.motor = component
        self.bus = Bus(self.gateway, component)
        self.drop_replies = drop_replies
        for packet in self.gateway.start():
            self.bus.enqueue(packet)
        self.bus.drain()
        require(component.connected, 'component bootstrap PC link failed')
        self.inventory()

    def unit(self, payload):
        require(len(payload) >= 2, 'short component request')
        require(tuple(payload[:2]) in self.motor.allowed_commands,
                f'component endpoint command not admitted: {payload[:2].hex()}')
        before = len(self.bus.replies)
        self.bus.enqueue(self.gateway.send(payload))
        self.bus.drain()
        return self.bus.replies[before:]

    def request(self, control, payload):
        if control != 0x48:
            return super().request(control, payload)
        require(len(payload) >= 3 and payload[0] == self.gateway.slot,
                'invalid component target')
        request = payload[1:]
        if request[:2] == b'\x32\x10':
            require(len(request) == 6 and request[2] in (0, 1) and request[3] == 0,
                    'unsupported component PC action/owner')
        replies = self.unit(request)
        if self.drop_replies and request[:2] not in (b'\x32\x10', b'\x32\x30'):
            return []
        return [(0x48, b'\x00' + bytes.fromhex(reply['payload'])) for reply in replies]


def serve_component(gateway_path, component_factory, socket_path, ready=None):
    """Serve sequential native sessions; the factory controls retained state.

    Socket binding refuses existing paths. Intended for offline verification,
    with a fresh exact-hash gateway per connection and explicit target policy.
    """
    path = Path(socket_path).absolute()
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    old_umask = os.umask(0o077)
    try:
        listener.bind(str(path))
    finally:
        os.umask(old_umask)
    try:
        listener.listen(1)
        if ready is not None:
            ready.set()
        else:
            print(f'Component simulator ready: --port sim:{path}', flush=True)
        while True:
            connection, _ = listener.accept()
            endpoint = None
            try:
                endpoint = ComponentEndpoint(gateway_path, component_factory())
                decoder = Decoder()
                with connection:
                    while chunk := connection.recv(4096):
                        for control, payload in decoder.feed(chunk):
                            for response, data in endpoint.request(control, payload):
                                connection.sendall(encode(response, data))
            except (OSError, RuntimeError, ValueError) as error:
                print(f'Component simulator session failed: {error}', file=sys.stderr, flush=True)
            finally:
                connection.close()
                if endpoint is not None:
                    try:
                        endpoint.close()
                    except RuntimeError as error:
                        print(f'Component cleanup unconfirmed: {error}', file=sys.stderr, flush=True)
    finally:
        listener.close()
        path.unlink(missing_ok=True)
