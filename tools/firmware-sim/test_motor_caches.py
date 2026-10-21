"""Stock cache-consumer checks for explicitly modeled missing boot/M state."""
import json
import os
from pathlib import Path
import unittest

from fw_sim.bus import Bus
from fw_sim.scenarios import setup


class StockMotorCacheTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_SIM_FIXTURES')
        if not manifest:
            raise unittest.SkipTest('set CRANKLINK_SIM_FIXTURES for stock cache tests')
        path = Path(manifest)
        cls.fixtures = {k:(path.parent/v).resolve() for k,v in json.loads(path.read_text()).items()}
        cls.motor_ids = ('ep500-4.1.0', 'ep500-4.1.1', 'e6100-4.8.0', 'e5100-4.1.0')
        if not {'sc-em800-4.6.0', 'ep600-4.4.1', *cls.motor_ids} <= cls.fixtures.keys():
            raise unittest.SkipTest('cache tests require the explicit profiled gateway/motors')

    def linked_motor(self, name):
        gateway, motor = setup(self.fixtures['sc-em800-4.6.0'], self.fixtures[name])
        bus = Bus(gateway, motor)
        for packet in gateway.start():
            bus.enqueue(packet)
        bus.drain()
        self.assertTrue(motor.connected)
        return motor

    def test_linked_version_query_consumes_header_compatibility_cache(self):
        for name in self.motor_ids:
            with self.subTest(motor=name):
                motor = self.linked_motor(name)
                image = self.fixtures[name].read_bytes()
                cache = motor.profile['rx_cache']
                self.assertEqual(bytes(motor.u.mem_read(cache, 3)), image[47:50])
                reply = bytes.fromhex(motor.receive(b'\x01\x2c\0', 0)[0]['payload'])
                self.assertEqual(reply[:5], b'\x01\x2e'+image[16:19])
                # Removing the explicit fixture restores the stock failure;
                # no query response or branch is replaced by the simulator.
                motor.u.mem_write(cache, b'\0\0\0')
                reply = bytes.fromhex(motor.receive(b'\x01\x2c\0', 0)[0]['payload'])
                self.assertTrue(reply.startswith(b'\x01\x2f\x39\0'))
                motor.u.mem_write(cache, image[47:50])
                reply = bytes.fromhex(motor.receive(b'\x01\x2c\0', 0)[0]['payload'])
                self.assertEqual(reply[:5], b'\x01\x2e'+image[16:19])

    def test_ep600_441_model_fixture_is_read_by_stock_get(self):
        motor = self.linked_motor('ep600-4.4.1')
        cache = motor.profile['cargo_ram']
        self.assertEqual(bytes(motor.u.mem_read(cache, 7)), b'00'+b'\0'*5)
        reply = bytes.fromhex(motor.receive(b'\x16\x7c\0', 0)[0]['payload'])
        self.assertEqual(reply, b'\x16\x7e00'+b'\0'*5)
        motor.u.mem_write(cache, b'01'+b'\0'*5)
        reply = bytes.fromhex(motor.receive(b'\x16\x7c\0', 0)[0]['payload'])
        self.assertEqual(reply, b'\x16\x7e01'+b'\0'*5)

    def test_additional_model_fixtures_are_stock_read_consumers(self):
        for name in self.motor_ids:
            with self.subTest(motor=name):
                motor = self.linked_motor(name)
                ascii_params = 'cargo_ram' in motor.profile
                expected = b'00'+b'\0'*5 if ascii_params else b'\x10'+b'\0'*6
                reply = bytes.fromhex(motor.receive(b'\x16\x7c\0', 0)[0]['payload'])
                self.assertEqual(reply, b'\x16\x7e'+expected)
                address = motor.profile['cargo_ram'] if ascii_params else motor.profile['product_params_ram']
                width = 7 if ascii_params else 4
                adjacent = bytes(motor.u.mem_read(address+width, 3))
                replacement = b'01'+b'\0'*5 if ascii_params else b'\x11'+b'\0'*3
                motor.u.mem_write(address, replacement)
                reply = bytes.fromhex(motor.receive(b'\x16\x7c\0', 0)[0]['payload'])
                expected = replacement if ascii_params else replacement+b'\0'*3
                self.assertEqual(reply, b'\x16\x7e'+expected)
                self.assertEqual(bytes(motor.u.mem_read(address+width, 3)), adjacent)


if __name__ == '__main__':
    unittest.main()
