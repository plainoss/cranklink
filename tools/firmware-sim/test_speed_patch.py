"""Exact-image admission tests, plus optional real firmware auth/persistence."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from fw_sim import speed_patch

HAVE_UNICORN = importlib.util.find_spec('unicorn') is not None
if HAVE_UNICORN:
    from fw_sim.motor import Motor
    from fw_sim.persistence import PersistentRecords


def code_for(ceiling):
    hi, lo = divmod(ceiling, 256)
    return bytes([hi, 0x20, 0, 2, lo, 0x30, 0x70, 0x47]) if lo else bytes([
        hi, 0x20, 0, 2, 0x70, 0x47, 0, 0xbf])


def candidate(image, ceiling):
    offset = speed_patch.PATCH_OFFSET
    return image[:offset] + code_for(ceiling) + image[offset + 8:]


class SpeedPatchAdmissionTests(unittest.TestCase):
    def setUp(self):
        body = bytearray(speed_patch.PATCH_OFFSET + 64)
        body[speed_patch.PATCH_OFFSET:speed_patch.PATCH_OFFSET + 8] = speed_patch.ORIGINAL_CODE
        self.stock = bytes(body)
        self.digest = hashlib.sha256(self.stock).hexdigest()
        self.pin = patch.object(speed_patch, 'STOCK_SHA256', self.digest)
        self.pin.start()
        self.addCleanup(self.pin.stop)

    def test_only_canonical_in_range_patch_is_recognized(self):
        self.assertIsNone(speed_patch.speed_patch_ceiling(self.stock))
        for ceiling in (1500, 1536, 2500, 6000, 6500, 9900):
            with self.subTest(ceiling=ceiling):
                self.assertEqual(speed_patch.speed_patch_ceiling(candidate(self.stock, ceiling)), ceiling)
        for ceiling in (1499, 9901, 32767, 65535):
            with self.subTest(ceiling=ceiling):
                self.assertIsNone(speed_patch.speed_patch_ceiling(candidate(self.stock, ceiling)))

    def test_mutation_outside_patch_or_extra_bytes_cannot_authorize(self):
        good = candidate(self.stock, 6500)
        for offset in (0, 0x5256, speed_patch.PATCH_OFFSET - 1,
                       speed_patch.PATCH_OFFSET + 8, len(good) - 1):
            changed = bytearray(good)
            changed[offset] ^= 1
            with self.subTest(offset=offset):
                self.assertIsNone(speed_patch.speed_patch_ceiling(bytes(changed)))
        self.assertIsNone(speed_patch.speed_patch_ceiling(good + b'\0'))
        self.assertIsNone(speed_patch.speed_patch_ceiling(good[:-1]))
        self.assertIsNone(speed_patch.speed_patch_ceiling(b''))

    def test_wrong_instruction_and_noncanonical_zero_form_are_rejected(self):
        good = candidate(self.stock, 6500)
        for offset in (1, 2, 3, 5, 6, 7):
            changed = bytearray(good)
            changed[speed_patch.PATCH_OFFSET + offset] ^= 1
            with self.subTest(offset=offset):
                self.assertIsNone(speed_patch.speed_patch_ceiling(bytes(changed)))
        changed = bytearray(good)
        changed[speed_patch.PATCH_OFFSET:speed_patch.PATCH_OFFSET + 8] = bytes.fromhex('1820000200307047')
        self.assertIsNone(speed_patch.speed_patch_ceiling(bytes(changed)))

    @unittest.skipUnless(HAVE_UNICORN, 'Unicorn required for simulator storage admission')
    def test_forged_profile_does_not_enable_storage(self):
        motor = Mock()
        motor._image = candidate(self.stock, 6500) + b'corrupt'
        motor.profile = {'id': 'ep800-4.2.0-patched', 'base_profile': 'ep800-4.2.0',
                         'sha256': speed_patch.STOCK_SHA256}
        motor.speed_patch_ceiling = 6500
        with self.assertRaisesRegex(RuntimeError, 'exact EP800'):
            PersistentRecords(motor)
        motor.byte.assert_not_called()
        motor.call.assert_not_called()


@unittest.skipUnless(HAVE_UNICORN, 'Unicorn required for real firmware tests')
class PatchedSpeedFirmwareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_SIM_FIXTURES')
        if not manifest:
            raise unittest.SkipTest('set CRANKLINK_SIM_FIXTURES for real speed-write tests')
        path = Path(manifest)
        fixtures = {key: (path.parent / value).resolve()
                    for key, value in json.loads(path.read_text()).items()}
        if not {'ep800-4.2.0', 'sc-em800-4.6.0'} <= fixtures.keys():
            raise unittest.SkipTest('requires EP800 D4.2.0 and SC-EM8004.6.0')
        from fw_sim.profiles import identify
        cls.stock, cls.profile = identify(fixtures['ep800-4.2.0'], 'motor')
        cls.gateway, cls.gateway_profile = identify(fixtures['sc-em800-4.6.0'], 'gateway')

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.state = Path(self.directory.name) / 'state.json'

    def new_motor(self, ceiling=6500):
        from fw_sim.gateway import Gateway
        from fw_sim.bus import Bus
        image = candidate(self.stock, ceiling)
        profile = dict(self.profile, sha256=hashlib.sha256(image).hexdigest())
        motor = Motor(image, profile)
        motor.install_storage(self.state)
        gateway = Gateway(self.gateway, self.gateway_profile)
        bus = Bus(gateway, motor)
        for packet in gateway.start():
            bus.enqueue(packet)
        bus.drain()
        self.assertTrue(motor.connected)
        return motor

    def test_authentication_commit_reload_and_state_image_binding(self):
        from test_persistence import authenticate
        motor = self.new_motor()
        request = bytes.fromhex('16b0641901ff')  # 65 km/h
        unbound = Motor(motor._image, motor.profile)
        instructions = unbound.instructions
        with self.assertRaisesRegex(RuntimeError, 'only normal'):
            unbound.receive(request, 0)
        self.assertEqual(unbound.instructions, instructions)
        self.assertEqual(motor.storage.commits, 0)
        self.assertFalse(self.state.exists())
        self.assertTrue(authenticate(motor)[0]['payload'].startswith('16e2'))
        self.assertTrue(motor.receive(request, 0)[0]['payload'].startswith('16b2'))
        stored = json.loads(self.state.read_text())
        self.assertEqual(stored['firmware_sha256'], hashlib.sha256(motor._image).hexdigest())
        reloaded = self.new_motor()
        self.assertEqual(int.from_bytes(reloaded.u.mem_read(0x2000047c, 2), 'little'), 6500)
        self.assertEqual(reloaded.byte(reloaded.profile['auth_ram']), 0)
        with self.assertRaisesRegex(RuntimeError, 'match firmware'):
            self.new_motor(6000)

    def test_stock_and_malformed_speed_requests_keep_write_guard(self):
        stock = Motor(self.stock, self.profile)
        stock.install_storage()
        with self.assertRaisesRegex(RuntimeError, 'only normal'):
            stock.receive(bytes.fromhex('16b0641901ff'), 0)
        patched = self.new_motor()
        for payload in ('16b0641900ff', '16b0641901ff00', '16a8641901ff', '32f000'):
            instructions = patched.instructions
            with self.subTest(payload=payload), self.assertRaises(RuntimeError):
                patched.receive(bytes.fromhex(payload), 0)
            self.assertEqual(patched.instructions, instructions)


if __name__ == '__main__':
    unittest.main()
