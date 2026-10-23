"""Strict CPU admission and optional acquired-firmware service regression cases."""
import json
import os
from pathlib import Path
import unittest
from fw_sim.mn101 import CPU, BASE, pack_pc_strict as pack_pc, unpack_pc_strict as unpack_pc
from fw_sim.di2_service import Di2Service, PROFILES, WIRELESS_INTERRUPT_WRITES


def encoded(nibbles):
    if len(nibbles) & 1:
        nibbles += '0'
    return bytes(int(nibbles[i], 16) | (int(nibbles[i + 1], 16) << 4)
                 for i in range(0, len(nibbles), 2))


class CpuAdmissionTests(unittest.TestCase):
    def test_unknown_and_unverified_operations_trap(self):
        for raw, error in [('ff', 'unsupported opcode'), ('219', 'unverified instruction'),
                           ('2fe', 'unverified instruction'), ('2ee', 'unverified instruction'),
                           ('238', 'unverified instruction'), ('d0', 'unmodeled IO operation')]:
            with self.subTest(raw=raw):
                result = CPU(encoded(raw)).run(BASE * 2)
                self.assertFalse(result['returned'])
                self.assertIn(error, result['error'])

    def test_pc_banks_are_rejected_instead_of_truncated(self):
        for pc in (0, 1, 0x7ffff):
            self.assertEqual(unpack_pc(pack_pc(pc)), pc)
        for packed in (0x40000, 0x840000, -1):
            with self.assertRaisesRegex(RuntimeError, 'PC width'):
                unpack_pc(packed)
        with self.assertRaisesRegex(RuntimeError, 'PC width'):
            pack_pc(0x80000)

    def test_mov_and_odd_half_call_return_preserve_stack(self):
        cpu = CPU(encoded('10100' + '01' + 'a0ba' + '01'))
        result = cpu.run(BASE * 2)
        self.assertTrue(result['returned'], result)
        self.assertEqual(result['final']['d'][0], 0xab)
        self.assertEqual(result['final']['sp'], result['initial']['sp'])

    def test_reports_are_independent_across_retained_ram_calls(self):
        cpu = CPU(encoded('a0ba01'))
        first = cpu.run(BASE * 2)
        snapshot = json.dumps(first, sort_keys=True)
        cpu.run(BASE * 2 + 4)
        self.assertEqual(json.dumps(first, sort_keys=True), snapshot)
        self.assertEqual(len(cpu.trace), 1)

    def test_unknown_mmio_and_budget_are_hard_failures(self):
        cpu = CPU(encoded('024f04' + '01'))
        result = cpu.run(BASE * 2)
        self.assertIn('unmodeled memory write 0x3ff4', result['error'])
        with self.assertRaises(ValueError):
            cpu.run(BASE * 2, budget=100001)
        result = CPU(encoded('00' * 8)).run(BASE * 2, budget=2)
        self.assertIn('budget exhausted', result['error'])

    def test_pause_is_not_reported_as_handler_return(self):
        result = CPU(encoded('01')).run(BASE * 2, until=lambda: True)
        self.assertTrue(result['paused'])
        self.assertFalse(result['returned'])
        self.assertEqual(result['instructions'], 0)


@unittest.skipUnless(os.environ.get('CRANKLINK_DI2_IMAGES'), 'acquired Di2 images not supplied')
class StockDi2ServiceTests(unittest.TestCase):
    def test_both_battery_boards_reproduce_stranded_start_and_actual_stop_receipt(self):
        root = Path(os.environ['CRANKLINK_DI2_IMAGES'])
        for name in list(PROFILES)[:2]:
            for owner in (3, 11, 19, 27):
                with self.subTest(image=name, owner=owner):
                    peer = Di2Service(root / name)
                    start = peer.link(1, owner)
                    self.assertTrue(start['returned'], start['error'])
                    self.assertEqual(start['instructions'], 128)
                    self.assertTrue(peer.connected)
                    self.assertEqual(peer.owner, owner)
                    self.assertEqual(start['packets'], [dict(target=63,
                        payload=bytes([0x32, 0x10, 1, owner, 0xff, 0xff]).hex())])
                    # A lost query reply / radio close changes no firmware RAM.
                    before = bytes(peer.ram)
                    self.assertTrue(peer.connected)
                    self.assertEqual(bytes(peer.ram), before)
                    stop = peer.link(0, owner)
                    self.assertTrue(stop['receipt_boundary'], stop['error'])
                    self.assertFalse(stop['connected'])
                    self.assertFalse(stop['returned'])  # Tail was NOT executed.
                    self.assertEqual(stop['packets'][-1], dict(target=owner,
                        payload=bytes([0x32, 0x12, 0, owner, 0xff, 0xff]).hex()))
                    self.assertFalse(stop['full_topology_validated'])
                    with self.assertRaisesRegex(RuntimeError, 'suspended'):
                        peer.link(1, owner)

    def test_battery_stop_receipt_echoes_requester_without_validating_stored_owner(self):
        peer = Di2Service(Path(os.environ['CRANKLINK_DI2_IMAGES']) / 'BTDN110.4.5.0.dat')
        self.assertTrue(peer.link(1, 19)['returned'])
        stop = peer.link(0, 3)
        self.assertTrue(stop['receipt_boundary'])
        self.assertFalse(stop['connected'])
        self.assertEqual(stop['owner'], 19)
        self.assertEqual(stop['packets'][-1], dict(target=3, payload='32120003ffff'))

    def test_wireless_stop_returns_and_restarts_with_explicit_interrupt_fixture(self):
        root = Path(os.environ['CRANKLINK_DI2_IMAGES'])
        peer = Di2Service(root / 'EWWU101.4.6.0.dat', model_wireless_interrupts=True)
        for owner in (3, 19, 27):
            self.assertTrue(peer.link(1, owner)['returned'])
            stop = peer.link(0, owner, complete=True)
            self.assertIsNone(stop['error'])
            self.assertTrue(stop['returned'])
            self.assertFalse(stop['paused_at_receipt'])
            self.assertTrue(stop['receipt_boundary'])
            self.assertFalse(stop['connected'])
            self.assertEqual(stop['modeled_interrupt_writes'], list(WIRELESS_INTERRUPT_WRITES))
            self.assertEqual(stop['final']['sp'], stop['initial']['sp'])
            self.assertFalse(stop['full_topology_validated'])
            self.assertEqual(stop['packets'], [dict(target=owner,
                payload=bytes([0x32, 0x12, 0, owner, 0xff, 0xff]).hex())])

    def test_wireless_interrupt_fixture_is_not_allowed_on_battery_images(self):
        with self.assertRaisesRegex(ValueError, 'wireless main image'):
            Di2Service(Path(os.environ['CRANKLINK_DI2_IMAGES']) / 'BTDN110.4.5.0.dat',
                       model_wireless_interrupts=True)

    def test_wireless_main_start_executes_but_stop_traps_at_unmodeled_peripheral(self):
        peer = Di2Service(Path(os.environ['CRANKLINK_DI2_IMAGES']) / 'EWWU101.4.6.0.dat')
        start = peer.link(1, 19)
        self.assertTrue(start['returned'], start['error'])
        self.assertTrue(peer.connected)
        self.assertEqual(peer.owner, 19)
        result = peer.link(0, 19)
        self.assertFalse(result['returned'])
        self.assertFalse(result['receipt_boundary'])
        self.assertIn('unmodeled memory write 0x3ff4', result['error'])
        self.assertFalse(result['full_topology_validated'])


if __name__ == '__main__':
    unittest.main()
