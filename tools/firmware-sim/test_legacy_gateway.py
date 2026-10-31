"""Stock gateway tests require a locally supplied, exact decoded image."""
import os
from pathlib import Path
import unittest
from fw_sim.legacy_gateway import LegacyGateway
from fw_sim.mn101 import BASE, MN101Machine
from test_mn101 import code


class HandyAddressTests(unittest.TestCase):
    def test_repeated_byte_add_uses_encoded_count_plus_one(self):
        cpu = MN101Machine(code('a010 21a 330 01'))
        result = cpu.run(BASE * 2)
        self.assertTrue(result['returned'], result)
        self.assertEqual(cpu.d[0], 8)
        self.assertEqual(sum(i['op'] == 'add_d_d' for i in result['trace']), 3)

    def test_byte_word_and_address_writeback(self):
        for store, expected, width in [('d3', 0x78, 1), ('95', 0x7856, 2), ('97', 0x5678, 2)]:
            with self.subTest(store=store):
                cpu = MN101Machine(code('48 ' + store + ' 01'))
                cpu.a[0] = 0x1100
                cpu.write(0x1100, 0x42, 2)
                result = cpu.run(BASE * 2)
                self.assertTrue(result['returned'], result)
                self.assertEqual(cpu.read(0x1100, width), expected)
                self.assertEqual(cpu.ha, 0x1100)

    def test_unmapped_handy_address_still_traps(self):
        cpu = MN101Machine(code('d0 01'))
        cpu.ha = 0x3000
        result = cpu.run(BASE * 2)
        self.assertFalse(result['returned'])
        self.assertIn('unmodeled memory write', result['error'])

    def test_wrong_image_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, 'SHA-256'):
            LegacyGateway(bytes(98110))


@unittest.skipUnless(os.environ.get('CRANKLINK_SC_E8000_IMAGE'),
                     'set CRANKLINK_SC_E8000_IMAGE for stock gateway probes')
class StockLegacyGatewayTests(unittest.TestCase):
    def setUp(self):
        self.gateway = LegacyGateway(Path(os.environ['CRANKLINK_SC_E8000_IMAGE']).read_bytes())

    def command(self, packet, replies):
        got, result = self.gateway.command(bytes.fromhex(packet))
        self.assertEqual(got, [bytes.fromhex(reply) for reply in replies])
        self.assertTrue(result['returned'])

    def test_stock_master_mode_and_restore(self):
        self.command('05', ['25400700'])
        self.command('04', ['248d'])
        self.command('0320', ['2300'])
        self.command('04', ['24a0'])
        # Stock master-to-slave changes role without an immediate 23 receipt.
        self.command('0300', [])
        self.command('04', ['248d'])
        self.assertTrue(self.gateway.boundaries)

    def test_discovery_bitmap_reply_waits_for_stock_countdown(self):
        self.command('0320', ['2300'])
        self.command('1d00', ['3d0000'])
        self.assertEqual(self.gateway.read(0x1e6a, 2), 2000)
        self.command('1b', [])
        for _ in range(2000):
            replies, _ = self.gateway.tick()
            self.assertEqual(replies, [])
        self.assertEqual(self.gateway.read(0x1e6a, 2), 0)
        # No bus announcements were injected: an empty bitmap is expected.
        replies, _ = self.gateway.tick()
        self.assertEqual(replies, [bytes.fromhex('3b00000000')])
        self.command('1b', ['3b00000000'])

    def test_pc_link_peripheral_gap_is_not_silently_accepted(self):
        self.command('0320', ['2300'])
        self.command('1d00', ['3d0000'])
        with self.assertRaisesRegex(RuntimeError, 'unmodeled IO operation bset_io'):
            self.gateway.command(bytes.fromhex('0c01'))
        self.assertNotIn(bytes.fromhex('2c00'), self.gateway.output)
