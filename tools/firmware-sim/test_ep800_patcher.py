"""Regression and verification tests for the EP800 firmware speed limit patcher."""

from pathlib import Path
import tempfile
import unittest

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from patch_ep800_speed import (
    decode_container,
    encode_container,
    validate_dcas_header,
    find_signatures,
    patch_binary,
    verify_flash_simulation,
    verify_in_emulator,
)


class TestEp800SpeedPatcher(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d420_bin = Path("/Users/manu/development/esteps-recover/evidence/ble-firmware-20261016/DUEP800-D.4.2.0.decoded.bin")
        cls.d420_dat = Path("/Users/manu/development/esteps-recover/evidence/ble-firmware-20261016/DUEP800-D.4.2.0.dat")
        cls.d4110_bin = Path("/Users/manu/development/esteps-recover/evidence/ble-privileged-auth-20261015/DUEP800-D.4.1.10.decoded.bin")
        cls.gateway_bin = Path("/Users/manu/development/esteps-recover/evidence/ble-us-28-diagnosis-20261015/SCEM800.4.6.0.decoded.bin")

    def test_container_roundtrip(self):
        if not self.d420_dat.exists() or not self.d420_bin.exists():
            self.skipTest("Requires local D4.2.0 fixtures")
        raw_dat = self.d420_dat.read_bytes()
        decoded = decode_container(raw_dat)
        expected = self.d420_bin.read_bytes()
        self.assertEqual(decoded, expected)

        # Re-encode and decode back
        re_encoded = encode_container(decoded)
        re_decoded = decode_container(re_encoded)
        self.assertEqual(re_decoded, expected)

    def test_header_validation(self):
        if not self.d420_bin.exists():
            self.skipTest("Requires local D4.2.0 fixture")
        data = self.d420_bin.read_bytes()
        meta = validate_dcas_header(data)
        self.assertEqual(meta["version"], "4.2.0.0")
        self.assertEqual(meta["series"], 0x23)
        self.assertEqual(meta["unit"], 0x00)
        self.assertEqual(meta["board"], 1)

    def test_signature_detection_across_versions(self):
        fixtures = [p for p in (self.d420_bin, self.d4110_bin) if p.exists()]
        if not fixtures:
            self.skipTest("Requires EP800 fixtures")
        for path in fixtures:
            with self.subTest(path=path.name):
                data = path.read_bytes()
                sigs = find_signatures(data)
                self.assertIn("max_speed_fn", sigs)
                self.assertIn("check_speed_bhs", sigs)
                self.assertIn("boot_clamp_bhs", sigs)
                self.assertGreater(sigs["max_speed_fn"], 0x5000)

    def test_patch_application_and_bounds(self):
        if not self.d420_bin.exists():
            self.skipTest("Requires local D4.2.0 fixture")
        data = self.d420_bin.read_bytes()
        patched, info = patch_binary(data, 6000)
        self.assertEqual(len(patched), len(data))
        self.assertNotEqual(info["original_sha256"], info["patched_sha256"])
        self.assertEqual(info["max_speed_raw"], 6000)
        self.assertEqual(info["max_speed_kmh"], 60.0)

        # Invalid bounds
        with self.assertRaises(ValueError):
            patch_binary(data, 1000)
        with self.assertRaises(ValueError):
            patch_binary(data, 70000)

    def test_flash_simulation(self):
        if not self.d420_bin.exists():
            self.skipTest("Requires local D4.2.0 fixture")
        data = self.d420_bin.read_bytes()
        patched, _ = patch_binary(data, 65535)
        flash_res = verify_flash_simulation(patched)
        self.assertEqual(flash_res["status"], "passed")
        self.assertEqual(flash_res["total_bytes"], len(data))
        self.assertEqual(flash_res["erase_code"], 77)

    def test_emulator_verification_arbitrary_large_int(self):
        if not self.d420_bin.exists() or not self.gateway_bin.exists():
            self.skipTest("Requires local EP800 and SC-EM800 fixtures")
        data = self.d420_bin.read_bytes()
        patched, _ = patch_binary(data, 65535)
        res = verify_in_emulator(patched, 65535, self.gateway_bin)
        self.assertEqual(res["status"], "verified")
        self.assertTrue(res["emulated_motor_connected"])
        self.assertTrue(res["authenticated"])
        # Check all speeds verified
        speeds = [item["speed_raw"] for item in res["tested_speed_limits"]]
        self.assertIn(65535, speeds)
        self.assertIn(5000, speeds)
        for item in res["tested_speed_limits"]:
            self.assertEqual(item["status"], "accepted")


if __name__ == "__main__":
    unittest.main()
