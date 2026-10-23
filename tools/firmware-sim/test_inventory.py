"""Synthetic container checks; recognized metadata never enables execution."""
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from fw_sim.inventory import inventory
from fw_sim.metadata import ENCRYPTED_MARKER, legacy_metadata
from fw_sim.motor import Motor
from fw_sim.profiles import identify


def image(series=0x15, revision=3, board=1):
    data = bytearray(256)
    data[:4] = b'\xff' * 4
    for offset, address in ((4, 0x4100), (8, 0x4101), (12, 0x4102)):
        data[offset:offset + 4] = address.to_bytes(4, 'little')
    data[0xC0] = 5
    data[0xC1:0xC4] = bytes([0x34, 7, 2])
    data[0xC6:0xCA] = len(data).to_bytes(4, 'little')
    data[0xCA:0xCE] = bytes([series, 0, revision, board])
    data[0xD3:0xD6] = bytes([0x34, 6, 1])
    data[-2:] = b'et'
    return bytes(data)


class LegacyContainerInventoryTests(unittest.TestCase):
    def test_header_versions_series_board_and_kind_do_not_depend_on_filename(self):
        for series, family, revision, kind, board in [
            (0x08, 'DU-E6000', 2, 'dcas2', 1),
            (0x0C, 'DU-E6001', 1, 'dcas2', 1),
            (0x15, 'DU-E6002', 3, 'dcas2i', 2),
        ]:
            with self.subTest(series=series):
                metadata = legacy_metadata(image(series, revision, board))
                self.assertTrue(metadata['metadata_valid'])
                self.assertEqual(metadata['family'], family)
                self.assertEqual(metadata['kind'], kind)
                self.assertEqual(metadata['version'], '3.4.7.2')
                self.assertEqual(metadata['compatibility_version'], '3.4.6.1')
                self.assertEqual(metadata['firmware_series'], series)
                self.assertEqual(metadata['firmware_unit'], 0)
                self.assertEqual(metadata['board'], board)
                self.assertEqual(metadata['declared_bytes'], 256)

    def test_inventory_is_descriptive_and_does_not_create_executable_fixture(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'unrelated-name.bin'
            path.write_bytes(image())
            report, fixtures = inventory([path])
            self.assertEqual(fixtures, {})
            self.assertEqual(report['profiled_images'], 0)
            row = report['images'][0]
            self.assertEqual(row['status'], 'unprofiled')
            self.assertEqual(row['container']['family'], 'DU-E6002')
            self.assertFalse(row['execution']['supported'])
            self.assertEqual(row['execution']['cpu'], 'unverified')
            self.assertEqual(row['execution']['backend'], 'unsupported')
            self.assertIn('instruction set', row['reason'])
            with patch('fw_sim.machine.new_cpu') as cpu:
                with self.assertRaisesRegex(RuntimeError, 'DU-E6002 dcas2i D3.4.7.2'):
                    identify(path, 'motor')
                with self.assertRaisesRegex(RuntimeError, 'execution backend/profile are unverified'):
                    Motor(*identify(path, 'motor'))
                cpu.assert_not_called()

    def test_invalid_headers_remain_non_executable_and_report_validation_errors(self):
        for offset, value, error in [
            (0xC6, 1, 'declared size'),
            (255, 0, 'trailer'),
            (0xCC, 9, 'header revision'),
            (0xC1, 0xF4, 'packed firmware version'),
        ]:
            with self.subTest(error=error), tempfile.TemporaryDirectory() as tmp:
                data = bytearray(image())
                data[offset] = value
                metadata = legacy_metadata(bytes(data))
                self.assertFalse(metadata['metadata_valid'])
                self.assertTrue(any(error in reason for reason in metadata['validation_errors']))
                path = Path(tmp) / 'image.bin'
                path.write_bytes(data)
                report, fixtures = inventory([path])
                self.assertEqual(fixtures, {})
                self.assertFalse(report['images'][0]['execution']['supported'])
                with self.assertRaisesRegex(RuntimeError, 'invalid legacy container metadata'):
                    identify(path, 'motor')

    def test_truncated_unrelated_and_encrypted_input_does_not_claim_metadata(self):
        for data in [image()[:0xD5], b'unknown', image() + ENCRYPTED_MARKER]:
            self.assertIsNone(legacy_metadata(data))
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'DUE6002-D.3.4.0.dat'
            path.write_bytes(b'unknown')
            report, fixtures = inventory([path])
            self.assertNotIn('container', report['images'][0])
            self.assertEqual(fixtures, {})
            path.write_bytes(image() + ENCRYPTED_MARKER)
            report, fixtures = inventory([path])
            self.assertEqual(report['images'][0]['status'], 'needs-decoding')
            self.assertNotIn('container', report['images'][0])
            self.assertEqual(fixtures, {})


if __name__ == '__main__':
    unittest.main()
