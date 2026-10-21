"""Stock codec/relay experiments. Set CRANKLINK_D_RELAY_CONFIG to local image JSON.

Config: {"radio": "/absolute/radio.bin", "gateways": ["/absolute/decoded.dat", ...]}.
No Bluetooth API is used. Missing fixtures skip explicitly, never fake execution.
"""
import json
import os
from pathlib import Path
import unittest

from fw_sim.d_relay import RadioPce, DGateway, BootPeer, fragment, buffer_packet


@unittest.skipUnless(os.environ.get('CRANKLINK_D_RELAY_CONFIG'), 'requires local stock D relay images')
class DRelayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = json.loads(Path(os.environ['CRANKLINK_D_RELAY_CONFIG']).read_text())
        if not cls.config['gateways']:
            raise ValueError('at least one gateway required')

    def machines(self, path):
        return RadioPce(self.config['radio']), DGateway(path)

    def upload(self, radio, packet=0, index=1, data=bytes(range(64))):
        for value in fragment(buffer_packet(packet, index, data)):
            radio.write(value)

    def transfer(self, radio, packet=0):
        radio.write(bytes([0, 0x0b, (packet + 1) % 256, 3, packet]))

    def test_early_start_matches_sequential_without_consuming_buffer_receipt(self):
        for path in self.config['gateways']:
            transcripts = []
            for early in (False, True):
                radio, gateway = self.machines(path)
                self.upload(radio)
                if not early:
                    gateway.deliver(radio.drain())
                    self.assertEqual(gateway.notifications, ['2b00820000'])
                self.transfer(radio)
                gateway.deliver(radio.drain())
                packets = gateway.accepted.copy()
                self.assertEqual(len(packets), 17)
                peer = BootPeer()
                peer.program(packets)
                gateway.complete_bus(packets)
                self.assertEqual(gateway.notifications, ['2b00820000', '2b01830100', '2b02c00000'])
                transcripts.append((gateway.output, gateway.notifications, peer.data))
            self.assertEqual(*transcripts)

    def test_fragment_loss_reorder_and_duplicate(self):
        for path in self.config['gateways']:
            original = fragment(buffer_packet(0, 1, bytes(range(64))))
            cases = {f'drop-{i}': original[:i] + original[i+1:] for i in range(4)}
            cases.update(reorder=[original[i] for i in (0, 2, 1, 3)],
                         duplicate_middle=[original[i] for i in (0, 1, 1, 2, 3)],
                         duplicate_terminal=original + [original[-1]])
            for name, values in cases.items():
                with self.subTest(gateway=path, fault=name):
                    radio, gateway = self.machines(path)
                    for value in values:
                        radio.write(value)
                    self.transfer(radio)
                    gateway.deliver(radio.drain())
                    if name.startswith('duplicate'):
                        self.assertEqual(len(gateway.accepted), 17)
                    else:
                        self.assertFalse(gateway.accepted)
                        self.assertTrue(gateway.notifications[-1].endswith('830101'))

    def test_rejected_upload_cannot_start_a_completed_block_after_packet_wrap(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            for index in range(1, 130):
                packet = (index - 1) * 2 % 256
                self.upload(radio, packet, index)
                self.transfer(radio, packet)
                before = len(gateway.accepted)
                gateway.deliver(radio.drain())
                packets = gateway.accepted[before:]
                self.assertEqual(len(packets), 17)
                gateway.complete_bus(packets)
            # Packet zero was consumed again at block129. Reject a new upload
            # using that same number, then attempt transfer without waiting82.
            radio.write(bytes([0, 0x0b, 0, 2, 0, 1]))
            self.transfer(radio, 0)
            before = len(gateway.accepted)
            gateway.deliver(radio.drain())
            self.assertEqual(len(gateway.accepted), before)
            self.assertTrue(gateway.notifications[-2].endswith('820001'))
            self.assertTrue(gateway.notifications[-1].endswith('830101'))

    def test_radio_queue_has_seven_slots_and_drops_eighth_record(self):
        radio = RadioPce(self.config['radio'])
        for packet in range(8):
            radio.write(bytes([0, 0x0b, packet, 3, packet]))
        records = radio.drain()
        self.assertEqual(len(records), 7)
        self.assertEqual([r[4] for r in records], list(range(7)))

    def test_two_buffers_are_not_a_safe_general_block_pipeline(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            # Buffer three uploads before transferring any: third overwrites
            # first. Stock82 reports success for all three; there is no credit.
            for index in range(1, 4):
                self.upload(radio, (index-1)*2, index)
            gateway.deliver(radio.drain())
            self.assertTrue(all(n.endswith('00') for n in gateway.notifications))
            self.transfer(radio, 0)
            gateway.deliver(radio.drain())
            self.assertFalse(gateway.accepted)
            self.assertTrue(gateway.notifications[-1].endswith('830101'))

    def test_bus_backpressure_rejects_a_later_transfer_after_partial_enqueue(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            accepted_counts = []
            for index in range(1, 9):
                self.upload(radio, (index - 1) * 2, index)
                self.transfer(radio, (index - 1) * 2)
                before = len(gateway.accepted)
                gateway.deliver(radio.drain())
                accepted_counts.append(len(gateway.accepted) - before)
                # Deliberately do not complete/consume the physical bus queue.
                if accepted_counts[-1] < 17:
                    break
            self.assertEqual(accepted_counts[:2], [17, 17])
            self.assertGreater(accepted_counts[-1], 0)
            self.assertLess(accepted_counts[-1], 17)
            self.assertTrue(gateway.notifications[-1].endswith('01'))

    def test_failed_upload_can_select_stale_unconsumed_buffer(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            self.upload(radio, 0, 1, b'\xaa' * 64)
            gateway.deliver(radio.drain())
            # A dirty session retains an accepted but untransferred packet0.
            # Reject a new malformed packet0 and speculatively transfer it.
            radio.write(bytes([0, 0x0b, 0, 2, 0, 1]))
            self.transfer(radio)
            gateway.deliver(radio.drain())
            self.assertTrue(gateway.notifications[-2].endswith('820001'))
            self.assertTrue(gateway.notifications[-1].endswith('830100'))
            peer = BootPeer()
            peer.program(gateway.accepted)
            self.assertEqual(peer.data, b'\xaa' * 64)

    def test_two_outstanding_starts_are_accepted_but_share_checksum_index(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            for index in (1, 2):
                self.upload(radio, (index-1)*2, index)
                self.transfer(radio, (index-1)*2)
            gateway.deliver(radio.drain())
            self.assertEqual(len(gateway.accepted), 34)
            self.assertEqual(gateway.checksum_index(), 2)
            gateway.complete_bus(gateway.accepted.copy())
            completions = [n for n in gateway.notifications if n[4:6] == 'c0']
            self.assertEqual(len(completions), 2)

    def test_larger_than_64_byte_block_is_rejected_by_stock_gateway(self):
        for path in self.config['gateways']:
            gateway = DGateway(path)
            # Bypass ATT limit to ask whether a larger MTU alone would help.
            data = bytes(range(68))
            gateway.dispatch(b'\x0b\x00\x02\x00' + data + bytes([sum(data) & 255, 1, 0]))
            self.assertTrue(gateway.notifications[-1].endswith('820001'))
            self.assertFalse(gateway.accepted)

    def test_prefetch_upload_preserves_current_checksum_index_and_data(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            self.upload(radio, 0, 1, b'\xaa' * 64)
            gateway.deliver(radio.drain())
            self.transfer(radio, 0)
            self.upload(radio, 2, 2, b'\xbb' * 64)
            gateway.deliver(radio.drain())
            self.assertEqual(gateway.checksum_index(), 1)
            first = gateway.accepted.copy()
            self.assertEqual(len(first), 17)
            gateway.complete_bus(first)
            self.transfer(radio, 2)
            gateway.deliver(radio.drain())
            self.assertEqual(gateway.checksum_index(), 2)
            peer = BootPeer()
            peer.program(first)
            peer.program(gateway.accepted[17:])
            self.assertEqual(peer.data, b'\xaa' * 64 + b'\xbb' * 64)

    def test_bad_block_checksum_is_not_rejected_by_buffer_acceptance(self):
        for path in self.config['gateways']:
            radio, gateway = self.machines(path)
            payload = bytearray(buffer_packet(0, 1, bytes(range(64))))
            payload[67] ^= 1
            for value in fragment(payload):
                radio.write(value)
            self.transfer(radio)
            gateway.deliver(radio.drain())
            self.assertTrue(gateway.notifications[0].endswith('820000'))
            with self.assertRaisesRegex(RuntimeError, 'checksum rejected'):
                BootPeer().program(gateway.accepted)


if __name__ == '__main__':
    unittest.main()
