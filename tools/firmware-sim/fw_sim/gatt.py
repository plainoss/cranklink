"""Modeled GATT peer for the real Cranklink BLE client.

Radio, pairing, PIN challenge and notification delivery are behavior models.
PCE commands and motor reads execute the hash-pinned gateway/motor firmware.
The virtual topology currently supports master routing to motor slot 13 only.
"""
import json
import os
import socket
import sys
import time
from collections import deque
from pathlib import Path
from .scenarios import setup
from .bus import Bus
from .machine import INPUT, require

AUTH_CONTROL, AUTH_NONCE = 0x2AF3, 0x2AF4
PCE_RESPONSE, PCE_COMMAND = 0x2AF9, 0x2AFA
FREE_RESPONSE, FREE_COMMAND, FEATURE = 0x2AFD, 0x2AFE, 0x2AFF
NONCE = bytes(range(16))
# AES-128 test vectors for this synthetic nonce and the public simulator PIN
# 000000, using the client's existing user/application key derivation.
USER_PROOF = bytes.fromhex('013a72c6d6f29630be3ec9dd85f5bd6be3')
APP_PROOF = bytes.fromhex('02e1b6c78c19964a801c0bbf1c6cc7cea8')


class GattDevice:
    """Powered virtual bike state shared across radio connections.

    Gateway/motor CPU RAM and installed storage are never recreated on a GATT
    reconnect. Each connection still has fresh authentication and notifications.
    This does not model advertising delays or invent disconnect failures.
    """
    def __init__(self, gateway_path, motor_path, storage_path=None, allow_saves=False, radio_lifecycle=False):
        self.gateway, self.motor = setup(gateway_path,motor_path)
        require('pce_dispatcher' in self.gateway.profile,
                'GATT requires a recovered PCE dispatcher for this firmware profile')
        self.bus = Bus(self.gateway,self.motor)
        self.gateway.restore()
        self.rx_version = Path(motor_path).read_bytes()[47:50]
        self.allow_saves = allow_saves
        if allow_saves:
            self.motor.install_storage(storage_path)
        self.radio_lifecycle = radio_lifecycle
        self.radio_available = True
        self.active_endpoint = None
        self.connections = 0
        self.connect_attempts = 0
        self.lifecycle = []

    def connect(self, drop_replies=False, fault=None):
        return GattEndpoint(None,None,drop_replies,fault,device=self)

    def end_connection_mode(self, connection, reason):
        if self.radio_lifecycle:
            self.radio_available = False
            self.lifecycle.append(dict(kind='modeled-radio-connection-mode-ended',connection=connection,reason=reason))

    def reenable_radio(self):
        require(self.active_endpoint is None, 'disconnect before re-enabling modeled connection mode')
        self.radio_available = True
        self.lifecycle.append(dict(kind='modeled-radio-connection-mode-reenabled'))

    def snapshot(self):
        counts = {}
        for event in self.lifecycle:
            key = event['kind']
            if key == 'pce-command': key += ':'+hex(event['command'])
            counts[key] = counts.get(key,0)+1
        state = dict(connections=self.connections,connect_attempts=self.connect_attempts,
                     connection_active=self.active_endpoint is not None,
                     motor_connected=self.motor.connected,
                     radio_available=self.radio_available,
                     modeled_radio_lifecycle=self.radio_lifecycle,
                     lifecycle_counts=counts,events=list(self.lifecycle))
        if 'owner_ram' in self.motor.profile: state['motor_pc_owner'] = self.motor.byte(self.motor.profile['owner_ram'])
        if 'auth_ram' in self.motor.profile: state['motor_regulation_auth_state'] = self.motor.byte(self.motor.profile['auth_ram'])
        if getattr(self.motor,'storage',None) is not None: state['storage'] = self.motor.storage.snapshot()
        return state


class GattEndpoint:
    modeled = ('GATT radio/pairing/notification delivery', 'fixed synthetic PIN/challenge',
               'PnP announcement delivery', 'motor logical address relocation at gateway role handoff',
               'slave-radio stop acknowledgment boundary', 'initialized RAM/plain MMIO', 'logical timing',
               'absent M application version query from decoded D header cache')

    def __init__(self, gateway_path, motor_path, drop_replies=False, fault=None, device=None):
        require(fault in (None,'stale-free','duplicate-free'), 'unknown modeled GATT fault')
        self.device = device if device is not None else GattDevice(gateway_path,motor_path)
        self.device.connect_attempts += 1
        self.device.lifecycle.append(dict(kind='gatt-connect-attempt',attempt=self.device.connect_attempts,
                                          radio_available=self.device.radio_available))
        require(self.device.radio_available, 'Gateway connection mode ended; explicitly re-enable the modeled radio')
        require(self.device.active_endpoint is None, 'modeled GATT device already has an active connection')
        self.gateway, self.motor, self.bus = self.device.gateway,self.device.motor,self.device.bus
        self.rx_version = self.device.rx_version
        self.fault, self.drop_replies = fault,drop_replies
        self.subscriptions, self.queue = set(),deque()
        self.auth_stage, self.access, self.closed = 0,False,False
        self.master = False
        self.events = []
        self.device.connections += 1
        self.connection_id = self.device.connections
        self.device.active_endpoint = self
        self.device.lifecycle.append(dict(kind='gatt-connect',connection=self.connection_id))

    def _lifecycle(self, kind, **fields):
        self.device.lifecycle.append(dict(kind=kind,connection=self.connection_id,**fields))

    def notify(self, characteristic, value):
        if characteristic in self.subscriptions:
            require(len(self.queue) < 256, 'modeled GATT notification queue overflow')
            self.queue.append(dict(characteristic=characteristic, value=list(value)))

    def _notify_free(self, value):
        if self.drop_replies: return
        # Mutations occur at the modeled radio delivery boundary, after actual
        # firmware routing/reassembly. Preserve the original firmware bytes.
        if self.fault == 'stale-free':
            self.notify(FREE_RESPONSE,bytes([(value[0]+1)%32])+value[1:])
        self.notify(FREE_RESPONSE,value)
        if self.fault == 'duplicate-free': self.notify(FREE_RESPONSE,value)

    def _pump(self, packets, handoff_stop=False):
        before = len(self.bus.replies)
        for packet in packets:
            if (handoff_stop and packet['target'] == 0
                    and bytes.fromhex(packet['payload'])[:3] == b'\x32\x10\0'):
                # After gateway role handoff the modeled bus motor occupies
                # logical master zero. Execute the exact gateway-generated stop
                # in motor firmware; its owner-zero ACK terminates at this
                # explicitly modeled slave-radio boundary, not a FREE reply.
                replies = self.motor.receive(bytes.fromhex(packet['payload']),packet['source'])
                require(not self.motor.connected, 'motor did not accept post-handoff PC stop')
                require(all(bytes.fromhex(r['payload'])[:3] == b'\x32\x12\0' for r in replies),
                        'unexpected post-handoff motor stop reply')
                self.events.append(dict(kind='modeled-master-address-handoff-stop',
                                        packet=dict(packet), replies=replies, motor_stopped=True))
                continue
            require(packet['payload'] != '3248ff', 'PnP event requires modeled discovery delivery')
            self.bus.enqueue(packet)
        self.bus.drain()
        for reply in self.bus.replies[before:]:
            self._notify_free(bytes([reply['slot']]) + bytes.fromhex(reply['payload']))

    def _inventory(self):
        # Query actual motor stock code and run the stock gateway announcement
        # parser. The absent physical PnP transport is an explicit modeled event.
        before = len(self.bus.replies)
        self.bus.enqueue(self.gateway.send(b'\x01\x1c\x00'))
        self.bus.drain()
        replies = self.bus.replies[before:]
        require(len(replies) == 1, 'motor stock unavailable during modeled discovery')
        stock = bytes.fromhex(replies[0]['payload'])
        require(stock[:2] == b'\x01\x1e', 'motor stock query rejected')
        self.gateway.u.mem_write(INPUT, bytes([6,255,1,0x18])+stock[2:4]+b'\xff\xff')
        self.gateway.call(self.gateway.profile['stock_handler'], self.gateway.slot, INPUT)
        self.events.append(dict(kind='modeled-pnp-delivery', slot=self.gateway.slot,
                                stock=stock[2:4].hex()))

    def pce(self, data):
        require(len(data) >= 2 and data[0] == 0, 'only unfragmented PCE writes are supported')
        command, params = data[1], data[2:]
        require(command in (3,4,5,6,0x0C,0x0D,0x13,0x1B,0x1D),
                'simulator blocks gateway reset, update and unsupported PCE commands')
        if command == 3:
            require(params in (b'\0',b'\x20'), 'only normal slave/master routing is supported')
        elif command in (4,5,0x1B):
            require(not params, 'unexpected PCE parameters')
        elif command == 6:
            require(params in (b'\0',bytes([self.gateway.slot])), 'unknown fixture target')
        elif command == 0x0C:
            require(params in (b'\0',b'\1'), 'unsupported gateway PC-link action')
        elif command in (0x0D,0x1D):
            require(params == b'\0', 'unsupported discovery action')
        elif command == 0x13:
            require(params == b'\x01\x1c\0', 'only gateway stock GET is supported')
        require(not (command == 0x0C and params == b'\1' and not self.master),
                'modeled topology requires --ble-master; logical bus-master-zero discovery is not implemented')
        require(command != 0x0D,
                'modeled topology requires --ble-master; logical bus-master-zero discovery is not implemented')
        self._lifecycle('pce-command',command=command,params=params.hex())
        g, p = self.gateway, self.gateway.profile
        notification_start, packet_start = len(g.notifications), len(g.output)
        raw = data[1:]
        g.u.mem_write(INPUT, raw)
        g.call(p['pce_dispatcher'], INPUT, len(raw))
        if command == 3:
            g.call(p['sched'])
            self.master = params == b'\x20'
            # Master->slave acknowledgment is asynchronous in these images.
            # A second stock mode request after the scheduler settles generates
            # the stock 23/00 receipt, without manufacturing a response.
            if len(g.notifications) == notification_start:
                g.u.mem_write(INPUT, raw)
                g.call(p['pce_dispatcher'], INPUT, len(raw))
        if command == 0x1D:
            # Consume the real broadcast discovery query at this modeled peer.
            packets = g.output[packet_start:]
            require(all(q['payload'] == '3248ff' for q in packets), 'unexpected PnP packet')
            self._inventory()
        elif command == 0x1B:
            # Dispatcher requests an asynchronous bitmap; run its stock worker.
            g.call(p['pce1b'])
        else:
            self._pump(g.output[packet_start:], handoff_stop=command == 0x0C and params == b'\0' and not self.master)
        for response in g.notifications[notification_start:]:
            self.notify(PCE_RESPONSE, bytes.fromhex(response))
        if command == 0x0C and params == b'\0' and '2c00' in g.notifications[notification_start:]:
            self.device.end_connection_mode(self.connection_id,'actual-stock-PCE-0C-stop-receipt')

    def free(self, data):
        require(4 <= len(data) <= 10, 'normal GATT packet must contain slot plus 3..9 bytes')
        slot, payload = data[0], data[1:]
        require(self.master and slot == self.gateway.slot,
                'modeled topology requires --ble-master and motor slot 13')
        if payload[:2] == b'\x32\x10':
            require(len(payload) == 6 and payload[2] in (0,1) and payload[3:] == b'\0\0\0',
                    'only PC owner zero start/stop is supported')
        elif payload[:2] == b'\x32\x30':
            require(len(payload) == 6 and payload[4:] == b'\0\0', 'invalid PC-link key packet')
        else:
            allowed_save = getattr(self.motor,'storage',None) is not None and tuple(payload[:2]) in ((0x10,0x10),(0x35,0),(0x16,0xd8),(0x16,0xe0))
            require(payload[1] & 7 == 4 or allowed_save,
                    'simulator is read-only unless verified persistent-storage operations are explicitly enabled')
        fields = dict(slot=slot,group=payload[0],command=payload[1],parameter_count=len(payload)-2)
        if payload[:2] == b'\x32\x10': fields['link_action'] = payload[2]
        self._lifecycle('free-command',**fields)
        if payload[:2] == b'\x01\x84':
            require(payload == b'\x01\x84\x01', 'unsupported modeled M application query')
            # The absent Renesas M application is a declared version-only peer;
            # its reply still traverses actual gateway fragment reassembly.
            reply = self.gateway.receive(b'\x01\x86' + self.rx_version + b'\xff')
            self.events.append(dict(kind='modeled-M-version-query'))
            if reply:
                self._notify_free(bytes([reply['slot']])+bytes.fromhex(reply['payload']))
        else:
            self._pump([self.gateway.send(payload)])
        if payload[0] == 0x32 or tuple(payload[:2]) in ((0x10,0x10),(0x35,0),(0x16,0xd8),(0x16,0xe0)):
            self._lifecycle('motor-state-after-protocol',group=payload[0],command=payload[1],
                            connected=self.motor.connected,
                            owner=self.motor.byte(self.motor.profile['owner_ram']),
                            regulation_auth=self.motor.byte(self.motor.profile['auth_ram']))

    def request(self, request):
        require(isinstance(request, dict), 'GATT request must be an object')
        operation = request.get('op')
        if operation == 'close':
            self.close()
            return dict(ok=True)
        require(not self.closed, 'GATT session is closed')
        if operation == 'receive':
            timeout = request.get('timeout_ms', 0)
            require(type(timeout) is int and 0 <= timeout <= 60000, 'invalid receive timeout')
            if not self.queue and timeout:
                time.sleep(timeout / 1000)
            return dict(ok=True, notification=self.queue.popleft() if self.queue else None)
        characteristic = request.get('id')
        require(type(characteristic) is int, 'GATT characteristic must be an integer')
        if operation == 'subscribe':
            require(characteristic in (AUTH_CONTROL,PCE_RESPONSE,FREE_RESPONSE), 'characteristic cannot notify')
            self.subscriptions.add(characteristic)
            return dict(ok=True)
        if operation == 'read':
            require(characteristic in (AUTH_NONCE,AUTH_CONTROL,FEATURE), 'characteristic cannot be read')
            value = NONCE if characteristic == AUTH_NONCE else bytes([self.auth_stage]) if characteristic == AUTH_CONTROL else b'\xff\0'
            return dict(ok=True, bytes=list(value))
        require(operation == 'write', 'unknown GATT operation')
        raw = request.get('bytes')
        require(isinstance(raw,list) and len(raw) <= 20 and all(type(v) is int and 0 <= v <= 255 for v in raw),
                'invalid GATT write bytes')
        data = bytes(raw)
        if characteristic == AUTH_CONTROL:
            require(not request.get('without_response',False), 'authentication requires write response')
            expected = USER_PROOF if self.auth_stage == 0 else APP_PROOF if self.auth_stage == 1 else None
            require(expected is not None and data == expected, 'synthetic BLE challenge rejected; simulator PIN is 000000')
            self.auth_stage += 1
            self._lifecycle('synthetic-auth-stage',stage=self.auth_stage)
            self.notify(AUTH_CONTROL, bytes([self.auth_stage,0]))
        elif characteristic == FEATURE:
            require(self.auth_stage == 2 and data == b'\xff\0', 'feature write rejected; reset/update is disabled')
            self.access = True
        else:
            require(self.access, 'authenticate the modeled GATT peer first')
            if characteristic == PCE_COMMAND: self.pce(data)
            elif characteristic == FREE_COMMAND: self.free(data)
            else: raise RuntimeError('characteristic is not writable')
        return dict(ok=True)

    def close(self):
        if self.closed: return
        self.closed = True
        # A radio disconnect does not transmit E-TUBE stop commands or power
        # cycle the bike. Preserve firmware RAM, including a stranded PC owner,
        # so a missing client stop remains observable across reconnects.
        self.device.active_endpoint = None
        self._lifecycle('gatt-disconnect',cleanup_confirmed=not self.motor.connected)


def read_request_line(connection, timeout=5, idle_timeout=30):
    """Bound idle waiting and the entire partial-frame assembly separately.

    Discovery legitimately leaves five seconds between requests. Once a frame
    starts, a slow peer cannot extend its deadline by sending another byte.
    """
    original_timeout = connection.gettimeout()
    deadline = time.monotonic() + idle_timeout
    line = bytearray()
    try:
        while True:
            remaining = deadline - time.monotonic()
            require(remaining > 0, 'GATT request deadline exceeded')
            connection.settimeout(remaining)
            chunk = connection.recv(1)
            if not chunk:
                require(not line, 'GATT peer disconnected during request')
                return b''
            if not line:
                deadline = time.monotonic() + timeout
            line.extend(chunk)
            require(len(line) <= 4096, 'GATT request line exceeds limit')
            if chunk == b'\n':
                return bytes(line)
            require(len(line) < 4096, 'GATT request line exceeds limit')
    finally:
        connection.settimeout(original_timeout)


def serve(gateway_path, motor_path, socket_path, drop_replies=False, fault=None, persistent=False):
    path = Path(socket_path).absolute()
    listener = socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    old_umask = os.umask(0o077)
    try: listener.bind(str(path))
    finally: os.umask(old_umask)
    try:
        listener.listen(1)
        device = GattDevice(gateway_path,motor_path) if persistent else None
        print(f'Modeled GATT ready: --port ble-sim:{path} --ble-master (PIN 000000; synthetic radio/auth)',flush=True)
        while True:
            connection, _ = listener.accept()
            endpoint = None
            try:
                endpoint = GattEndpoint(gateway_path,motor_path,drop_replies,fault,device=device)
                connection.settimeout(30)
                with connection:
                    while line := read_request_line(connection):
                        failed = False
                        try: response = endpoint.request(json.loads(line))
                        except (ValueError,RuntimeError,KeyError,TypeError) as error:
                            response = dict(ok=False,error=str(error))
                            failed = True
                        connection.sendall(json.dumps(response,separators=(',',':')).encode()+b'\n')
                        if failed or endpoint.closed: break
            except (OSError,ValueError,RuntimeError) as error:
                print(f'Modeled GATT session failed: {error}',file=sys.stderr,flush=True)
            finally:
                connection.close()
                if endpoint is not None:
                    try: endpoint.close()
                    except RuntimeError as error: print(f'GATT cleanup unconfirmed: {error}',file=sys.stderr)
    finally:
        listener.close()
        path.unlink(missing_ok=True)
