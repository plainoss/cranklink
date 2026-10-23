"""Read-timeout reproduction: modeled BLE gateway, actual battery service code.

Only gateway setup, master metadata and target selection are modeled. The master
stock query deliberately loses its reply. PC start/stop run acquired firmware.
This peer does not claim to execute WU111 PCE/radio firmware or settings reads.
"""
from collections import deque
from .di2_service import Di2Service
from .gatt import GattEndpoint, PCE_RESPONSE, FREE_RESPONSE
from .machine import require


class Di2TimeoutEndpoint(GattEndpoint):
    def __init__(self, image, case="identity-timeout"):
        require(case in ('identity-timeout', 'metadata-timeout', 'component-timeout', 'stock-only'), 'unknown Di2 case')
        self.case = case
        self.battery = Di2Service(image)
        require(self.battery.profile['send_vector'] == 4, 'battery image required')
        self.queue, self.subscriptions = deque(), set()
        self.auth_stage, self.access, self.closed = 0, False, False
        self.events, self.executions = [], []
        self.target = None
        self.pc_slot = 19

    def _lifecycle(self, kind, **fields):
        self.events.append(dict(kind=kind, **fields))

    def pce(self, data):
        require(data[:1] == b'\0', 'only unfragmented PCE commands supported')
        if data == b'\0\x03\0':
            reply = b'\x23\0'
        elif data == b'\0\x04':
            reply = bytes([0x24, 0x80 | self.pc_slot])
        elif data == b'\0\x0c\x01':
            result = self.battery.link(1, self.pc_slot)
            self.executions.append(result)
            require(result['returned'], 'battery PC start did not return')
            if self.case == 'metadata-timeout':
                # Captured protocol start-notification fixture; the asynchronous
                # start ACK path itself is not executed by this service harness.
                self._lifecycle('modeled-master-start-notification')
                self.notify(FREE_RESPONSE, bytes([0, 0x32, 0x12, 1, self.pc_slot, 0xff, 0xff]))
            reply = b'\x2c\0'
        elif data == b'\0\x0d\0':
            if self.case == 'metadata-timeout':
                self._lifecycle('modeled-pce', bytes=data.hex())
                self._lifecycle('modeled-lost-master-metadata-reply')
                return
            reply = bytes([0x2d, 1, 0x11, 1])  # Modeled battery identity metadata.
        elif len(data) == 3 and data[:2] == b'\0\x06' and data[2] in (0, 13):
            self.target = data[2]
            reply = b'\x26\0'
        elif data == b'\0\x0c\0':
            # A gateway ACK does not imply that the battery stopped. Assert it
            # separately from the original firmware's retained mode byte.
            reply = b'\x2c\0'
        else:
            raise RuntimeError('unmodeled Di2 gateway command ' + data.hex())
        self._lifecycle('modeled-pce', bytes=data.hex())
        self.notify(PCE_RESPONSE, reply)

    def free(self, data):
        require(data and self.target == data[0], 'acknowledged target selection required')
        self._lifecycle('free', bytes=data.hex())
        if data == b'\0\x01\x1c\0':
            if self.case == 'identity-timeout':
                self._lifecycle('modeled-lost-identity-reply')
            else:
                # Identity/bitmap/cache are deliberately modeled fixtures, not
                # execution of battery GET or wireless gateway discovery code.
                self.notify(FREE_RESPONSE, bytes.fromhex('00011e1100ff'))
            return
        if self.case != 'identity-timeout':
            if data == bytes.fromhex('00011400'):
                self.notify(FREE_RESPONSE, bytes.fromhex('000116000000200000'))
                return
            if data == bytes.fromhex('0002140d'):
                self.notify(FREE_RESPONSE, bytes.fromhex('0002160d1604ff'))
                return
            if data == bytes.fromhex('0d329c00'):
                self._lifecycle('modeled-lost-component-part-reply')
                return
        require(data == bytes([0, 0x32, 0x10, 0, self.pc_slot, 0, 0]),
                'only read-timeout cleanup is profiled')
        result = self.battery.link(0, self.pc_slot)
        self.executions.append(result)
        require(result['receipt_boundary'] and not result['connected'],
                'battery stop did not reach its original receipt boundary')
        for packet in result['packets']:
            if packet['target'] == self.pc_slot:
                self.notify(FREE_RESPONSE, b'\0' + bytes.fromhex(packet['payload']))

    def close(self):
        if not self.closed:
            self._lifecycle('radio-close', battery_connected=self.battery.connected)
        self.closed = True  # No protocol cleanup or hidden RAM reset.
