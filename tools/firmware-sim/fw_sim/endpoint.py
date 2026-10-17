"""Local native Port endpoint: modeled USB adapter + stock gateway/motor D.

The optional M application is a behavior model for its version query only.
No EEPROM, erase, update or reset commands are admitted.
"""
import os
import socket
import sys
from pathlib import Path
from .scenarios import setup
from .bus import Bus
from .machine import require, INPUT
from .wire import Decoder, encode


class Endpoint:
    def __init__(self, gateway_path, motor_path, drop_replies=False):
        self.gateway, self.motor = setup(gateway_path, motor_path)
        self.bus = Bus(self.gateway, self.motor)
        self.drop_replies = drop_replies
        self.rx_version = Path(motor_path).read_bytes()[47:50]
        for packet in self.gateway.start(): self.bus.enqueue(packet)
        self.bus.drain()
        require(self.motor.connected, 'endpoint bootstrap PC link failed')
        self.inventory()

    def unit(self, payload):
        before = len(self.bus.replies)
        if payload[:2] == b'\x01\x84':
            # Explicit behavior peer for the absent Renesas M application.
            # Route its version bytes through real gateway reassembly, never
            # claim this query executed M firmware.
            reply = self.gateway.receive(b'\x01\x86' + self.rx_version + b'\xff')
            if reply: self.bus.replies.append(reply)
        else:
            self.bus.enqueue(self.gateway.send(payload))
            self.bus.drain()
        return self.bus.replies[before:]

    def inventory(self):
        replies = self.unit(b'\x01\x1c\x00')
        require(len(replies) == 1, 'stock identity unavailable')
        stock = bytes.fromhex(replies[0]['payload'])
        require(stock[:2] == b'\x01\x1e', 'stock identity rejected')
        # Modeled discovery event, using identity obtained from motor code.
        # Original stock announcement parser populates the gateway cache.
        p = self.gateway.profile
        self.gateway.u.mem_write(INPUT, bytes([6,255,1,0x18])+stock[2:4]+b'\xff\xff')
        self.gateway.call(p['stock_handler'], self.gateway.slot, INPUT)

    def request(self, control, payload):
        g, p = self.gateway, self.gateway.profile
        if control == 0x10: return [(0x30,b'\x00')]  # Modeled USB adapter reset.
        if control == 0x1A:
            require(payload in (b'\x01',b'\x02'), 'unsupported power operation')
            return [(0x3A,b'\x02\x01' if payload==b'\x02' else b'\x01\x00')]
        if control == 3:
            require(payload == b'\x20', 'only normal protocol supported')
            return [(0x23,b'\x00')]
        if control == 4: return [(0x24,b'\xa0')]
        if control == 6:
            require(payload in (bytes([g.slot]),b'\x00'), 'unknown fixture target')
            return [(0x26,b'\x00')]
        if control == 0x1D:
            require(payload == b'\x00', 'unsupported PnP action')
            self.inventory()
            # PnP physical delivery is the modeled discovery event above.
            return [(0x3D,b'\x00\x00')]
        if control == 0x1B:
            g.call(p['pce1b'])
            actual = bytes.fromhex(g.notifications[-1])
            require(actual[0] == 0x3B, 'inventory notification mismatch')
            return [(0x3B,actual[1:])]
        if control == 0x48:
            require(len(payload) >= 3 and payload[0] == g.slot, 'invalid normal target')
            request = payload[1:]
            if request[:2] in (b'\x32\x10',b'\x32\x30'):
                require(len(request) >= 4, 'truncated PC link')
                require(request[:2] != b'\x32\x10' or request[2] in (0,1), 'unsupported PC action')
                require(request[:2] != b'\x32\x10' or request[3] == 0, 'only owner zero supported')
            else:
                require(request[1] & 7 == 4, 'simulator endpoint is read-only')
            replies = self.unit(request)
            if self.drop_replies and request[:2] not in (b'\x32\x10',b'\x32\x30'):
                return []
            return [(0x48,bytes([0])+bytes.fromhex(r['payload'])) for r in replies]
        raise RuntimeError(f'unsupported adapter command {control:#x}')

    def close(self):
        for packet in self.gateway.link_stop(): self.bus.enqueue(packet)
        self.bus.drain()
        self.gateway.restore()
        require(not self.motor.connected, 'endpoint cleanup failed')


def serve(gateway_path, motor_path, socket_path, drop_replies=False):
    require(hasattr(socket, 'AF_UNIX'), 'native simulator currently requires Unix sockets')
    socket_path = Path(socket_path).absolute()
    # Bind refuses any existing path; never unlink a socket owned by someone else.
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    old_umask = os.umask(0o077)
    try:
        listener.bind(str(socket_path))
    finally:
        os.umask(old_umask)
    try:
        listener.listen(1)
        print(f'Simulator ready: --port sim:{socket_path} (synthetic NVM, modeled M version)', flush=True)
        while True:
            connection, _ = listener.accept()
            endpoint = None
            try:
                endpoint = Endpoint(gateway_path, motor_path, drop_replies)
                decoder = Decoder()
                with connection:
                    while chunk := connection.recv(4096):
                        for control, payload in decoder.feed(chunk):
                            for response, data in endpoint.request(control,payload):
                                connection.sendall(encode(response,data))
            except (OSError, RuntimeError, ValueError) as error:
                print(f'Simulator session failed: {error}', file=sys.stderr, flush=True)
            finally:
                connection.close()
                if endpoint is not None:
                    try: endpoint.close()
                    except RuntimeError as error: print(f'Cleanup unconfirmed: {error}', file=sys.stderr)
    finally:
        listener.close()
        socket_path.unlink(missing_ok=True)
