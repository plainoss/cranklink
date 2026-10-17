"""Regression and verification tests for the EP800 firmware speed limit patcher."""

from pathlib import Path
import argparse
import json
import subprocess
import tempfile
from unittest.mock import patch
import unittest

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from patch_motor_speed import parse_speed_limit, MAX_SPEED_RAW
from patch_ep800_speed import (
    is_container,
    analyze_flash_layout,
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
        patched, _ = patch_binary(data, MAX_SPEED_RAW)
        flash_res = verify_flash_simulation(patched)
        self.assertEqual(flash_res["status"], "analyzed")
        self.assertFalse(flash_res["bootloader_executed"])
        self.assertEqual(flash_res["total_bytes"], len(data))
        self.assertEqual(flash_res["erase_code"], 77)

    def test_ep800_setting_persistence_and_assist_verification(self):
        if not self.d420_bin.exists() or not self.gateway_bin.exists():
            self.skipTest("Requires local EP800 and SC-EM800 fixtures")
        data = self.d420_bin.read_bytes()
        patched, info = patch_binary(data, MAX_SPEED_RAW)
        res = verify_in_emulator(patched, MAX_SPEED_RAW, self.gateway_bin, info)
        self.assertEqual(res["status"], "verified")
        self.assertTrue(res["emulated_motor_connected"])
        self.assertTrue(res["authenticated"])
        # Check all speeds verified
        speeds = [item["speed_raw"] for item in res["tested_speed_limits"]]
        self.assertIn(MAX_SPEED_RAW, speeds)
        self.assertIn(5000, speeds)
        for item in res["tested_speed_limits"]:
            self.assertEqual(item["status"], "accepted")
            self.assertTrue(item["persisted"])
            self.assertTrue(item["reloaded"])
            self.assertEqual(item["get_reply"], item["reboot_get_reply"])
            speed = item["speed_raw"]
            self.assertEqual(item["assist_curve_table"][4:], [speed + 125, speed - 300, speed - 200, speed])

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
                patched, info = patch_binary(plain, MAX_SPEED_RAW)
                res = verify_in_emulator(patched, MAX_SPEED_RAW, patch_info=info)
                self.assertEqual(res["status"], "partial")
                self.assertEqual(res["boot_clamp"]["status"], "verified")
                self.assertEqual(res["max_speed_limit_returned"], MAX_SPEED_RAW)
                self.assertTrue(res["get_speed_reply"].startswith("16b6"))


    def ep800_fixture(self):
        if not self.d420_bin.exists() or not self.gateway_bin.exists():
            self.skipTest("Requires local EP800 and SC-EM800 fixtures")
        return self.d420_bin.read_bytes()

    def test_default_and_range_boundaries_in_emulator(self):
        data = self.ep800_fixture()
        for ceiling in (1500, 2500, 6000):
            with self.subTest(ceiling=ceiling):
                patched, info = patch_binary(data, ceiling)
                result = verify_in_emulator(patched, ceiling, self.gateway_bin, info)
                self.assertEqual(result["status"], "verified")
                self.assertEqual(result["tested_speed_limits"][-1]["speed_raw"], ceiling)
                self.assertTrue(all(x["speed_raw"] <= ceiling for x in result["tested_speed_limits"]))

    def test_corrupt_boot_instruction_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        damaged = bytearray(patched)
        offset = find_signatures(data)["boot_clamp_bhs"]
        damaged[offset:offset + 2] = bytes.fromhex("00de")  # udf #0
        with self.assertRaises(RuntimeError):
            verify_in_emulator(bytes(damaged), 6000, self.gateway_bin, info)

    def test_boot_clamp_bypass_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        damaged = bytearray(patched)
        offset = find_signatures(data)["boot_clamp_bhs"]
        damaged[offset:offset + 2] = bytes.fromhex("00e0")
        with self.assertRaisesRegex(RuntimeError, "Boot clamp mismatch"):
            verify_in_emulator(bytes(damaged), 6000, self.gateway_bin, info)

    def test_set_ceiling_bypass_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        damaged = bytearray(patched)
        offset = find_signatures(data)["check_speed_bhs"]
        damaged[offset:offset + 2] = bytes.fromhex("01e0")
        with self.assertRaisesRegex(RuntimeError, "Above-ceiling speed .* accepted"):
            verify_in_emulator(bytes(damaged), 6000, self.gateway_bin, info)

    def test_zero_assist_thresholds_fail_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        from fw_sim.motor import Motor
        original = Motor.receive

        def zero_curve(machine, payload, source):
            replies = original(machine, payload, source)
            if payload[:2] == bytes.fromhex("16b0"):
                machine.u.mem_write(0x200024b0, bytes(16))
            return replies

        with patch.object(Motor, "receive", zero_curve):
            with self.assertRaisesRegex(RuntimeError, "Assist curve mismatch"):
                verify_in_emulator(patched, 6000, self.gateway_bin, info)

    def test_missing_disk_commit_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        from fw_sim.persistence import PersistentRecords
        with patch.object(PersistentRecords, "save", return_value=None):
            with self.assertRaises(FileNotFoundError):
                verify_in_emulator(patched, 6000, self.gateway_bin, info)

    def test_reload_of_stale_speed_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        from fw_sim.persistence import PersistentRecords
        original = PersistentRecords.__init__

        def stale_reload(storage, motor, path=None):
            reloading = path is not None and Path(path).exists()
            original(storage, motor, path)
            if reloading:
                motor.u.mem_write(0x2000047c, (2500).to_bytes(2, "little"))

        with patch.object(PersistentRecords, "__init__", stale_reload):
            with self.assertRaisesRegex(RuntimeError, "did not survive reload"):
                verify_in_emulator(patched, 6000, self.gateway_bin, info)

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



class PatcherInputTests(unittest.TestCase):
    """Run without private firmware, PyCryptodome, or Unicorn fixtures."""

    @staticmethod
    def image():
        data = bytearray(1024)
        data[:16] = bytes([255]) * 16
        data[16:19] = bytes.fromhex("420000")
        data[24:28] = len(data).to_bytes(4, "little")
        data[40:44] = bytes.fromhex("23000401")
        data[47:50] = bytes.fromhex("410b00")
        data[-2:] = b"et"
        for offset, signature in (
            (140, "022807d004d30428"),
            (200, "3800210080b289b2884201d2002621e0"),
            (300, "02000b8892b29a4200d2088001bd"),
        ):
            code = bytes.fromhex(signature)
            data[offset:offset + len(code)] = code
        return bytes(data)

    def test_unsigned_max_and_out_of_range_raw_limits_rejected(self):
        for speed in (0, 1499, 9901, 32767, 60000, 65535, 70000):
            with self.subTest(speed=speed), self.assertRaises(ValueError):
                patch_binary(self.image(), speed)

    def test_flash_analysis_rejects_destroyed_header(self):
        data = bytearray(157996)
        data[40:43] = bytes.fromhex("230004")
        with self.assertRaises(ValueError):
            analyze_flash_layout(bytes(data))
        for offset in (0, 24, 42, 1023):
            damaged = bytearray(self.image())
            damaged[offset] ^= 1
            with self.subTest(offset=offset), self.assertRaises(ValueError):
                analyze_flash_layout(bytes(damaged))

    def test_flash_analysis_explicitly_does_not_execute_bootloader(self):
        for analyze in (analyze_flash_layout, verify_flash_simulation):
            result = analyze(self.image())
            self.assertEqual(result["status"], "analyzed")
            self.assertFalse(result["bootloader_executed"])

    def test_kmh_parser_never_guesses_raw_units(self):
        for value, raw in (("15", 1500), ("45.01", 4501), ("60", 6000), ("99.00", 9900)):
            self.assertEqual(parse_speed_limit(value), raw)
        for value in ("1500", "65535", "655.35", "99.01", "14.99", "NaN", "inf", "-inf", "45.001", "invalid"):
            with self.subTest(value=value), self.assertRaises(argparse.ArgumentTypeError):
                parse_speed_limit(value)

    def test_cli_default_boundaries_and_invalid_units(self):
        tool = Path(__file__).resolve().parent.parent / "patch_motor_speed.py"
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.bin"
            output = Path(directory) / "patched.bin"
            source.write_bytes(self.image())
            command = [sys.executable, str(tool), "-i", str(source), "-o", str(output), "--json"]
            for options, expected in (([], 6000), (["-s", "15"], 1500), (["-s", "45.01"], 4501), (["-s", "99"], 9900)):
                completed = subprocess.run(command + options, capture_output=True, text=True, check=True)
                self.assertEqual(json.loads(completed.stdout)["patch_info"]["max_speed_raw"], expected)
            for value in ("1500", "65535", "NaN", "inf", "45.001"):
                output.unlink(missing_ok=True)
                completed = subprocess.run(command + ["-s", value], capture_output=True, text=True)
                self.assertEqual(completed.returncode, 2)
                self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
