#!/usr/bin/env python3
"""Offline stock codec/gateway peer with a declared virtual latency model.

No wall-clock speed is inferred from emulator execution. Mean phase latencies
come from the saved hardware trace; acknowledged-write return time is varied
because that trace did not measure it. Motor bootloader replies remain modeled.
"""
import heapq
import json
from pathlib import Path
import sys
from collections import deque

from fw_sim.d_relay import RadioPce, DGateway, BootPeer


class Peer:
    def __init__(self, radio, gateway, fault, length, ack_write_ms=0):
        self.radio, self.gateway = RadioPce(radio), DGateway(gateway)
        self.motor = BootPeer()
        self.fault = fault
        self.queue = deque()
        self.seen_notifications = 0
        self.finished = self.reset = False
        self.transfers = self.uploads = self.erases = self.write_calls = 0
        self.start_before_buffer_read = 0
        self.buffer_reads = set()
        self.injected = False
        self.now = 0.0
        self.events = []
        self.event_sequence = 0
        self.program_pending = False
        self.stream_start = self.stream_end = None
        self.ack_write_ms = float(ack_write_ms)
        if not 0 <= self.ack_write_ms <= 1000:
            raise ValueError('invalid acknowledged write return duration')
        trace = json.loads(Path(__file__).with_name('d-transfer-analysis.json').read_text())
        phases = trace['phases']
        self.fragment_ms = phases['first_to_last_fragment']['total_ms'] / phases['first_to_last_fragment']['count']
        self.upload_ms = phases['last_fragment_to_buffer_ack']['total_ms'] / phases['last_fragment_to_buffer_ack']['count']
        self.dispatch_ms = phases['buffer_ack_to_transfer_dispatch']['total_ms'] / phases['buffer_ack_to_transfer_dispatch']['count']
        self.transfer_ms = phases['transfer_dispatch_to_all_receipts']['total_ms'] / phases['transfer_dispatch_to_all_receipts']['count']
        self.gap_ms = (trace['stream_ms'] - phases['block_total']['total_ms']) / (trace['complete_indexed_blocks'] - 1)
        self.expected = bytearray(i % 251 for i in range(length))
        self.expected[40:44] = bytes([0x23, 0, 4, 1])

    def schedule(self, delay, operation, data):
        self.event_sequence += 1
        heapq.heappush(self.events, (self.now + delay, self.event_sequence, operation, data))

    def advance(self, deadline):
        while self.events and self.events[0][0] <= deadline:
            self.now, _, operation, data = heapq.heappop(self.events)
            if operation == 'notify':
                self.queue.append(data)
            elif operation == 'program':
                packets, block = data
                replies = []
                try:
                    self.motor.program(packets)
                    replies.extend([b'\x88\x31\x00\x00', self.motor.receipts[-1]])
                except RuntimeError:
                    replies.append(b'\x88\x32\x01')
                self.gateway.complete_bus(packets)
                replies.extend(self.take_gateway_notifications())
                self.program_pending = False
                self.notify(replies, block)
            else:
                raise ValueError('unknown clock event')
        self.now = deadline

    def boot(self, command):
        code = command[0]
        identities = {0x2e: [0x3a, 0x50, 6], 0x2f: [0x3b, 1, 1],
                      0x30: [0x3c, 0x23, 0], 0x29: [0x35, 1, 2, 3],
                      0x2a: [0x36, 4, 5, 6]}
        if code == 0x21:
            self.erases += 1
            assert self.erases == 1 and not self.motor.data, 'erase replay'
        elif code == 0x27:
            assert self.motor.data == self.expected.ljust(len(self.motor.data), b'\xff'), 'image differs'
            assert len(self.motor.data) == (len(self.expected) + 63) // 64 * 64, 'incomplete image'
            assert command[1] == sum(self.expected) & 255, 'whole image checksum differs'
            self.finished = True
        elif code == 0x28:
            assert self.finished, 'reset before finish'
            self.reset = True
        return bytes([0x88] + identities.get(code, [0x31, 0, 0]))

    @staticmethod
    def receipt_kind(value):
        if len(value) >= 5 and value[0] & 11 == 11:
            return {0x82: '82', 0x83: '83', 0xc0: 'c0'}.get(value[2], '')
        return 'motor' if value[:2] == b'\x88\x31' and value[2:4] != b'\0\0' else ''

    def notify(self, replies, block, delay=0):
        if self.fault == 'reorder':
            replies = list(reversed(replies))
        for value in replies:
            kind = self.receipt_kind(value)
            if self.fault.startswith('duplicate-drop-') and block == 129 and kind == self.fault.split('-')[2]:
                continue
            if self.fault == f'drop-{kind}' or (self.fault == 'drop-prefetch-82' and kind == '82' and block == 2):
                continue
            if self.fault == f'nak-{kind}' or (self.fault == 'nak-prefetch-82' and kind == '82' and block == 2):
                value = b'\x88\x32\x01'
            delivery_delay = delay + (1 if self.fault == 'reorder' and kind == '83' else 0)
            self.schedule(delivery_delay, 'notify', value)
            if kind and (self.fault == 'duplicate' or (self.fault.startswith('duplicate-drop-') and block == 1)):
                self.schedule(delivery_delay, 'notify', value)

    def take_gateway_notifications(self):
        replies = [bytes.fromhex(n) for n in self.gateway.notifications[self.seen_notifications:]]
        self.seen_notifications = len(self.gateway.notifications)
        return replies

    def flush_uart(self):
        for record in self.radio.drain():
            logical = record[3:-1]
            before = len(self.gateway.accepted)
            self.gateway.deliver([record])
            packets = self.gateway.accepted[before:]
            replies = self.take_gateway_notifications()
            if logical[0] == 0x88:
                expected = [logical[1:]]
                if logical[1] in (0x29, 0x2a):
                    expected.append(bytes([logical[1] + 1]) + logical[2:])
                elif logical[1] == 0x24:
                    expected = [b'\x24\x00\x00\x00']
                assert [bytes.fromhex(p['payload']) for p in packets] == expected, (logical.hex(), packets)
                replies.append(self.boot(logical[1:]))
                self.gateway.complete_bus(packets)
                replies.extend(self.take_gateway_notifications())
                self.notify(replies, 0, 60)
            elif logical[0] == 0x0b and logical[2] == 2:
                self.uploads += 1
                index = int.from_bytes(logical[-2:], 'little')
                self.notify(replies, index, self.upload_ms)
            elif logical[0] == 0x0b and logical[2] == 3:
                assert not self.program_pending, 'multiple programming transfers outstanding'
                self.transfers += 1
                self.program_pending = True
                self.notify(replies, self.transfers, self.transfer_ms)
                self.schedule(self.transfer_ms, 'program', (packets, self.transfers))

    def request(self, request):
        op = request['op']
        if op == 'write':
            self.write_calls += 1
            value = bytes(request['bytes'])
            if value[:2] == b'\x80\x0b':
                if self.stream_start is None:
                    self.stream_start = self.now
                self.buffer_reads.discard(value[2])
            if len(value) == 5 and value[:2] == b'\0\x0b' and value[3] == 3:
                self.start_before_buffer_read += value[4] not in self.buffer_reads
                self.advance(self.now + self.dispatch_ms + (self.gap_ms if self.transfers else 0))
            next_upload = self.uploads + 1
            if value[:2] == b'\xa0\x0b' and (self.fault == 'disconnect-fragment' or
                    (self.fault == 'disconnect-prefetch' and next_upload == 2)):
                raise RuntimeError('injected disconnect during fragment dispatch')
            if not self.injected and value[:2] == b'\xc0\x0b' and (
                    self.fault == 'corrupt-fragment' or (self.fault == 'corrupt-prefetch' and next_upload == 2)):
                value = value[:3] + bytes([value[3] ^ 1]) + value[4:]
                self.injected = True
            if not self.injected and value[:2] == b'\x60\x0b' and self.fault == 'drop-terminal':
                self.injected = True
                return dict(ok=True)
            self.radio.write(value)
            self.flush_uart()
            delay = self.fragment_ms / 3 if value[:2] in (b'\x80\x0b', b'\xa0\x0b', b'\xc0\x0b') else 0
            if not request.get('without_response', False):
                delay += self.ack_write_ms
            self.advance(self.now + delay)
            return dict(ok=True)
        if op == 'receive':
            timeout = min(float(request.get('timeout_ms', 0)), 60000)
            deadline = self.now + timeout
            while not self.queue and self.events and self.events[0][0] <= deadline:
                self.advance(self.events[0][0])
            if not self.queue:
                self.advance(deadline)
            value = self.queue.popleft() if self.queue else None
            if value and self.receipt_kind(value) == '82':
                self.buffer_reads.add(value[3])
            if value and self.receipt_kind(value) and self.stream_start is not None:
                self.stream_end = self.now
            return dict(ok=True, notification=None if value is None else
                        dict(characteristic=0x2af9, value=list(value)))
        if op == 'stats':
            seconds = None if self.stream_end is None else (self.stream_end - self.stream_start) / 1000
            return dict(ok=True, uploads=self.uploads, transfers=self.transfers,
                        bytes=len(self.motor.data), finish=self.finished, reset=self.reset,
                        early_starts=self.start_before_buffer_read, erases=self.erases,
                        write_calls=self.write_calls, ack_write_ms=self.ack_write_ms,
                        stream_seconds=seconds, image_bytes_per_second=None if not seconds or not self.finished else len(self.expected) / seconds)
        raise ValueError('unsupported peer operation')


if __name__ == '__main__':
    peer = Peer(sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]),
                float(sys.argv[5]) if len(sys.argv) > 5 else 0)
    for line in sys.stdin:
        try:
            reply = peer.request(json.loads(line))
        except Exception as error:
            reply = dict(ok=False, error=f'{type(error).__name__}: {error}')
        print(json.dumps(reply), flush=True)
