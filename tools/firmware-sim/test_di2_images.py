"""Research input integrity/format checks; these are not lifecycle simulations."""
import hashlib
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fw_sim.di2_images import IMAGES, MAX_BYTES, acquire, assess_image, intel_hex_memory


def record(kind, address=0, data=b''):
    body = bytes([len(data)]) + address.to_bytes(2, 'big') + bytes([kind]) + data
    return b':' + (body + bytes([-sum(body) & 255])).hex().encode() + b'\n'


class IntelHexTests(unittest.TestCase):
    def test_extended_addresses_and_identical_overlap(self):
        data = (record(4, data=b'\x00\x01') + record(0, 0xd000, b'\xaa\xbb')
                + record(0, 0xd001, b'\xbb') + record(5, data=b'\0\x01\xd0\x01') + record(1))
        self.assertEqual(intel_hex_memory(data), {0x1d000: 0xaa, 0x1d001: 0xbb})

    def test_uicr_is_sparse_and_never_flattened(self):
        data = record(4, data=b'\x10\x00') + record(0, 0x1000, b'\x12') + record(1)
        self.assertEqual(intel_hex_memory(data), {0x10001000: 0x12})

    def test_corruption_truncation_conflicting_overlap_and_after_eof_rejected(self):
        good = record(0, 0, b'\x01')
        for data in (good[:-3]+b'00\n'+record(1), good,
                     good+record(0, 0, b'\x02')+record(1),
                     good+record(1)+good, record(1),
                     record(4, data=b'\x20\x00')+record(0, 0, b'\0')+record(1),
                     record(0, 0xffff, b'\x01\x02')+record(1)):
            with self.subTest(data=data[:40]), self.assertRaises(ValueError):
                intel_hex_memory(data)

    def test_bad_record_shapes_and_size_limit_rejected(self):
        for data in (b'bad\n', record(4, data=b'\0')+record(1),
                     record(5, data=b'\0')+record(1), record(2)+record(1),
                     b' '*(MAX_BYTES+1)):
            with self.subTest(size=len(data)), self.assertRaises(ValueError):
                intel_hex_memory(data)


class ResearchInputTests(unittest.TestCase):
    def test_research_hashes_do_not_grant_execution_profiles(self):
        from fw_sim.profiles import PROFILES
        self.assertTrue({row['sha256'] for row in IMAGES}.isdisjoint(
            profile['sha256'] for profile in PROFILES))

    def test_mutated_input_rejected_before_format_inspection(self):
        for meta in IMAGES:
            with self.subTest(file=meta['filename']), self.assertRaisesRegex(ValueError, 'SHA-256'):
                assess_image(meta, b'not the pinned firmware')

    def test_missing_and_corrupt_offline_cache_never_access_network(self):
        with tempfile.TemporaryDirectory() as folder, patch('urllib.request.urlopen') as network:
            with self.assertRaisesRegex(ValueError, 'missing offline'):
                acquire(folder, offline=True)
            path = Path(folder)/IMAGES[0]['filename']
            path.write_bytes(b'corrupt cache')
            with self.assertRaisesRegex(ValueError, 'SHA-256'):
                acquire(folder, offline=True)
            self.assertEqual(path.read_bytes(), b'corrupt cache')
            self.assertFalse((Path(folder)/'di2-acquisition.json').exists())
            network.assert_not_called()

    def test_download_validates_before_persisting_and_can_be_rechecked_offline(self):
        raw = record(0, 0, b'\x12\x34') + record(1)
        meta = dict(filename='research-test.hex', model='synthetic format test',
                    sha256=hashlib.sha256(raw).hexdigest())
        with tempfile.TemporaryDirectory() as folder, \
                patch('fw_sim.di2_images.IMAGES', (meta,)), \
                patch('urllib.request.urlopen', return_value=io.BytesIO(raw)) as network:
            report = acquire(folder)
            self.assertEqual((Path(folder)/meta['filename']).read_bytes(), raw)
            self.assertFalse(report['topology_validated'])
            network.assert_called_once()
            request = network.call_args.args[0]
            self.assertEqual(request.full_url,
                             'https://api.shimano.com/etube/public/data/upload/published/research-test.hex')
            network.reset_mock()
            self.assertEqual(acquire(folder, offline=True), report)
            network.assert_not_called()

    def test_bad_download_is_not_saved(self):
        with tempfile.TemporaryDirectory() as folder, \
                patch('urllib.request.urlopen', return_value=io.BytesIO(b'corrupt download')):
            with self.assertRaisesRegex(ValueError, 'SHA-256'):
                acquire(folder)
            self.assertEqual(list(Path(folder).iterdir()), [])


class AcquiredImageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root = os.environ.get('CRANKLINK_DI2_IMAGES')
        if not root:
            raise unittest.SkipTest('set CRANKLINK_DI2_IMAGES for acquired research input checks')
        cls.root = Path(root)

    def test_exact_images_validate_but_do_not_become_executable(self):
        from fw_sim.profiles import identify
        for meta in IMAGES:
            path = self.root/meta['filename']
            raw = path.read_bytes()
            row = assess_image(meta, raw)
            with self.subTest(file=path.name):
                self.assertEqual(hashlib.sha256(raw).hexdigest(), meta['sha256'])
                self.assertFalse(row['topology_validated'])
                self.assertEqual(row['execution'], 'unprofiled')
                if path.suffix == '.dat':
                    self.assertGreaterEqual(len(row['mn101_rti_vector_offsets']), 18)
                    self.assertEqual(row['board'], raw[0xcd])
                    with self.assertRaisesRegex(RuntimeError, 'unsupported'):
                        identify(path)
                elif '-ap.' in path.name:
                    self.assertTrue(row['arm_vector_candidate'])

    def test_offline_report_preserves_coverage_boundary(self):
        with patch('urllib.request.urlopen') as network:
            report = acquire(self.root, offline=True)
        self.assertEqual(report['executable_profiles_added'], 0)
        self.assertFalse(report['topology_validated'])
        self.assertEqual(len(report['images']), 6)
        network.assert_not_called()


if __name__ == '__main__':
    unittest.main()
