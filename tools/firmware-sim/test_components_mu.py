"""Exact MU stock request, validation and declared storage-boundary regressions."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fw_sim.component_endpoint import ComponentEndpoint
from fw_sim.component_mu import MuUr500, PROFILE


class MuStockFirmwareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_COMPONENT_FIXTURES')
        if not manifest:
            raise unittest.SkipTest('set CRANKLINK_COMPONENT_FIXTURES for stock MU firmware tests')
        path = Path(manifest)
        fixtures = {key:(path.parent/value).resolve() for key,value in json.loads(path.read_text()).items()}
        if not {'sc-em800-4.6.0','mu-ur500-3.3.1'} <= fixtures.keys():
            raise unittest.SkipTest('MU tests require SC-EM8004.6.0 and MU-UR5003.3.1')
        cls.gateway_path = fixtures['sc-em800-4.6.0']
        cls.image = fixtures['mu-ur500-3.3.1'].read_bytes()

    def endpoint(self, **kwargs):
        component = MuUr500(self.image, **kwargs)
        endpoint = ComponentEndpoint(self.gateway_path, component)
        self.addCleanup(endpoint.close)
        self.assertTrue(component.connected)
        return endpoint,component

    def request(self, endpoint, payload):
        replies = endpoint.unit(bytes.fromhex(payload))
        self.assertEqual(len(replies),1)
        return bytes.fromhex(replies[0]['payload'])

    def test_get_request_dispatch_and_stock_reply_for_every_seed(self):
        for adjustment in range(-4,5):
            for gears in (5,8,11):
                with self.subTest(adjustment=adjustment,gears=gears):
                    endpoint,component = self.endpoint(adjustment=adjustment,gears=gears)
                    self.assertEqual(self.request(endpoint,'0b1400')[:4],bytes([0x0b,0x16,adjustment&255,4]))
                    self.assertEqual(self.request(endpoint,'0b1c00')[:4],bytes([0x0b,0x1e,1,gears]))
                    self.assertGreater(component.instructions,1000)

    def test_gear_set_stages_stock_checksum_and_reads_after_disk_reload(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)/'state.json'
            endpoint,component = self.endpoint(allow_saves=True,storage_path=state)
            for index,gears in enumerate((8,5,11,8),1):
                with self.subTest(gears=gears):
                    self.assertEqual(self.request(endpoint,f'0b1801{gears:02x}0000')[:4],bytes([0x0b,0x1a,1,gears]))
                    self.assertEqual(self.request(endpoint,'0b1c00')[:4],bytes([0x0b,0x1e,1,gears]))
                    self.assertEqual(component.storage.commits,index)
                    self.assertEqual(sum(component.storage.record)&255,0)
            reloaded,fresh = self.endpoint(allow_saves=True,storage_path=state)
            self.assertEqual(self.request(reloaded,'0b1c00')[:4],b'\x0b\x1e\x01\x08')
            self.assertEqual(fresh.storage.commits,0)
            self.assertIsNot(fresh,component)

    def test_firmware_rejects_every_unknown_gear_count_without_committing(self):
        endpoint,component = self.endpoint(allow_saves=True)
        initial = component.storage.record
        for gears in range(256):
            if gears in (5,8,11):
                continue
            with self.subTest(gears=gears):
                self.assertEqual(self.request(endpoint,f'0b1801{gears:02x}0000')[:2],b'\x0b\x1b')
                self.assertEqual(self.request(endpoint,'0b1c00')[:4],b'\x0b\x1e\x01\x0b')
                self.assertEqual(component.storage.record,initial)
                self.assertEqual(component.storage.commits,0)

    def test_adjustment_ack_remains_pending_until_unmodeled_actuator_completes(self):
        endpoint,component = self.endpoint(allow_saves=True)
        for value in range(-4,5):
            with self.subTest(value=value):
                self.assertEqual(self.request(endpoint,f'0b10{value&255:02x}040000')[:2],b'\x0b\x12')
                self.assertEqual(component.byte(PROFILE['target_adjustment_ram']),value&255)
                self.assertEqual(self.request(endpoint,'0b1400')[:4],b'\x0b\x16\x00\x04')
                self.assertEqual(component.storage.commits,0)
        for value in range(-128,128):
            if -4 <= value <= 4:
                continue
            with self.subTest(invalid=value):
                self.assertEqual(self.request(endpoint,f'0b10{value&255:02x}040000')[:2],b'\x0b\x13')
                self.assertEqual(self.request(endpoint,'0b1400')[:4],b'\x0b\x16\x00\x04')

    def test_equal_current_set_cancels_pending_adjustment_target(self):
        endpoint,component = self.endpoint(allow_saves=True)
        self.assertEqual(self.request(endpoint,'0b1400')[:4],b'\x0b\x16\x00\x04')
        self.assertEqual(self.request(endpoint,'0b1002040000')[:3],b'\x0b\x12\x00')
        self.assertEqual(component.byte(PROFILE['target_adjustment_ram']),2)
        self.assertEqual(self.request(endpoint,'0b1400')[:4],b'\x0b\x16\x00\x04')
        # A target equal to the current reading remains a meaningful command:
        # it replaces pending movement that the current-value GET cannot show.
        self.assertEqual(self.request(endpoint,'0b1000040000')[:3],b'\x0b\x12\x00')
        self.assertEqual(component.byte(PROFILE['target_adjustment_ram']),0)
        self.assertEqual(self.request(endpoint,'0b1400')[:4],b'\x0b\x16\x00\x04')
        self.assertEqual(component.storage.commits,0)

    def test_commit_failure_cannot_emit_a_successful_ack_or_persist_new_value(self):
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)/'state.json'
            endpoint,component = self.endpoint(allow_saves=True,storage_path=state)
            self.request(endpoint,'0b1801080000')
            before = len(component.output)
            with patch.object(component.storage,'save',side_effect=OSError('modeled disk failure')):
                with self.assertRaisesRegex(OSError,'disk failure'):
                    endpoint.unit(bytes.fromhex('0b1801050000'))
            self.assertEqual(len(component.output),before)
            self.assertEqual(component.storage.snapshot()['gear_count'],8)
            self.assertEqual(component.storage.commits,1)
            reloaded,_ = self.endpoint(allow_saves=True,storage_path=state)
            self.assertEqual(self.request(reloaded,'0b1c00')[:4],b'\x0b\x1e\x01\x08')

    def test_hash_storage_and_command_admission_fail_closed(self):
        with self.assertRaisesRegex(RuntimeError,'hash mismatch'):
            MuUr500(self.image[:-1]+bytes([self.image[-1]^1]))
        endpoint,component = self.endpoint()
        for payload in ('0b1801080000','0b1001040000','32f000','01c000'):
            instructions = component.instructions
            with self.assertRaises(RuntimeError):
                component.receive(bytes.fromhex(payload),0)
            self.assertEqual(component.instructions,instructions)
        _,writable = self.endpoint(allow_saves=True)
        for payload in ('0b18','0b180108','0b10','0b100104'):
            instructions = writable.instructions
            with self.assertRaisesRegex(RuntimeError,'complete four-byte'):
                writable.receive(bytes.fromhex(payload),0)
            self.assertEqual(writable.instructions,instructions)
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory)/'state.json'
            state.write_text(json.dumps(dict(schema_version=1,firmware_sha256=PROFILE['sha256'],gear_record='08ffffffffffff00')))
            with self.assertRaisesRegex(RuntimeError,'checksum'):
                MuUr500(self.image,allow_saves=True,storage_path=state)


if __name__ == '__main__':
    unittest.main()
