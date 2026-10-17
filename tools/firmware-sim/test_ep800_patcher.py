"""Regression and verification tests for the EP800 firmware speed limit patcher."""

from pathlib import Path
import tempfile
import unittest

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from patch_ep800_speed import (
    is_container,
    decode_container,
    encode_container,
    validate_dcas_header,
    encode_mov32,
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
        cls.d4111_dat = Path("/Users/manu/development/esteps-recover/evidence/ble-firmware-20261016/DUEP800-D.4.1.11.dat")
        cls.d4110_bin = Path("/Users/manu/development/esteps-recover/evidence/ble-privileged-auth-20261015/DUEP800-D.4.1.10.decoded.bin")
        cls.d4110_dat = Path("/Users/manu/development/esteps-recover/evidence/ble-privileged-auth-20261015/DUEP800-D.4.1.10.dat")
        cls.e6100_dat = Path("/private/var/folders/wg/l3w4n48j2qv1b1x5tv21kp2m0000gn/T/esteps-current-firmware-audit/DUE6100-D.4.8.0.dat")
        cls.e5100_dat = Path("/private/var/folders/wg/l3w4n48j2qv1b1x5tv21kp2m0000gn/T/esteps-current-firmware-audit/DUE5100-D.4.1.0.dat")
        cls.e5000_dat = Path("/private/var/folders/wg/l3w4n48j2qv1b1x5tv21kp2m0000gn/T/esteps-current-firmware-audit/DUE5000-D.4.5.0.dat")
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

    def test_signature_detection_across_motor_families(self):
        all_candidates = [
            self.d420_bin, self.d4110_bin, self.d4111_dat,
            self.e6100_dat, self.e5100_dat, self.e5000_dat
        ]
        fixtures = [p for p in all_candidates if p.exists()]
        if not fixtures:
            self.skipTest("Requires motor firmware fixtures")
        for path in fixtures:
            with self.subTest(path=path.name):
                raw = path.read_bytes()
                plain = decode_container(raw) if is_container(raw) else raw
                sigs = find_signatures(plain)
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
        patched, info = patch_binary(data, 65535)
        res = verify_in_emulator(patched, 65535, self.gateway_bin, info)
        self.assertEqual(res["status"], "verified")
        self.assertTrue(res["emulated_motor_connected"])
        self.assertTrue(res["authenticated"])
        # Check all speeds verified
        speeds = [item["speed_raw"] for item in res["tested_speed_limits"]]
        self.assertIn(65535, speeds)
        self.assertIn(5000, speeds)
        for item in res["tested_speed_limits"]:
            self.assertEqual(item["status"], "accepted")

    def test_multi_motor_patching_and_emulation(self):
        motor_fixtures = [
            ("EP800-4.1.11", self.d4111_dat),
            ("EP800-4.1.10", self.d4110_dat),
            ("E6100-4.8.0", self.e6100_dat),
            ("E5100-4.1.0", self.e5100_dat),
            ("E5000-4.5.0", self.e5000_dat),
        ]
        available = [(name, path) for name, path in motor_fixtures if path.exists()]
        if not available:
            self.skipTest("Requires legacy DCASX motor fixtures")

        for name, path in available:
            with self.subTest(motor=name):
                raw = path.read_bytes()
                plain = decode_container(raw) if is_container(raw) else raw
                patched, info = patch_binary(plain, 65535)
                res = verify_in_emulator(patched, 65535, patch_info=info)
                self.assertEqual(res["status"], "verified")
                self.assertEqual(res["max_speed_limit_returned"], 65535)
                self.assertTrue(res["get_speed_reply"].startswith("16b6"))

    def test_encode_mov32(self):
        # 16-bit constant (<= 0xffff): 8 bytes
        code_16 = encode_mov32(65535)
        self.assertEqual(len(code_16), 8)
        self.assertTrue(code_16.endswith(bytes([0x70, 0x47, 0x00, 0xbf])))  # bx lr; nop

        code_2500 = encode_mov32(2500)
        self.assertEqual(len(code_2500), 8)
        self.assertTrue(code_2500.endswith(bytes([0x70, 0x47, 0x00, 0xbf])))

        # 32-bit constant (> 0xffff): 12 bytes
        code_32 = encode_mov32(100000)
        self.assertEqual(len(code_32), 12)
        self.assertTrue(code_32.endswith(bytes([0x70, 0x47, 0x00, 0xbf])))


if __name__ == "__main__":
    unittest.main()
