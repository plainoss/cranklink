"""Patcher regressions; stock images are optional and never bundled.

CRANKLINK_SIM_FIXTURES uses the simulator's JSON profile-id:path manifest. Paths
resolve beside the manifest; this suite accepts decoded .bin or encrypted .dat
files. PyCryptodome is required only for .dat files. Firmware-free checks below
also run without Unicorn.
"""

from pathlib import Path
import argparse
from contextlib import redirect_stderr, redirect_stdout
import hashlib
from io import StringIO
import json
import os
import tempfile
from unittest.mock import patch
import unittest

import sys
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import patch_motor_speed as patcher
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
        cls.fixture_directory = tempfile.TemporaryDirectory(prefix="patcher-fixtures-")
        cls.addClassCleanup(cls.fixture_directory.cleanup)
        empty = Path(cls.fixture_directory.name) / "not-supplied.bin"
        cls.d420_bin = cls.d420_dat = cls.d4111_dat = cls.d4110_bin = empty
        cls.d4110_dat = cls.e6100_dat = cls.e5100_dat = cls.e5000_dat = empty
        cls.gateway_bin = empty
        fixture_manifest = os.environ.get("CRANKLINK_SIM_FIXTURES")
        if not fixture_manifest:
            return
        manifest = Path(fixture_manifest).resolve()
        entries = json.loads(manifest.read_text())
        if not isinstance(entries, dict) or not entries:
            raise ValueError("fixture manifest must be a nonempty profile-id:path object")
        profiles = json.loads((Path(__file__).parent / "profiles.json").read_text())
        known = {profile["id"]: profile for profile in profiles}
        fixture_attributes = {
            "ep800-4.2.0": ("d420_bin", "d420_dat"),
            "ep800-4.1.11": ("d4111_dat",),
            "ep800-4.1.10": ("d4110_bin", "d4110_dat"),
            "e6100-4.8.0": ("e6100_dat",),
            "e5100-4.1.0": ("e5100_dat",),
            "e5000-4.5.0": ("e5000_dat",),
            "sc-em800-4.6.0": ("gateway_bin",),
        }
        for name, value in entries.items():
            if name not in known or not isinstance(value, str):
                raise ValueError(f"unknown fixture profile or invalid path: {name}")
            source = (manifest.parent / value).resolve()
            raw = source.read_bytes()
            plain = decode_container(raw) if is_container(raw) else raw
            if hashlib.sha256(plain).hexdigest() != known[name]["sha256"]:
                raise ValueError(f"fixture {name} does not match its profile SHA-256")
            decoded = Path(cls.fixture_directory.name) / f"{name}.bin"
            decoded.write_bytes(plain)
            for attribute in fixture_attributes.get(name, ()):
                setattr(cls, attribute, decoded)

    def test_container_roundtrip(self):
        if not self.d420_dat.exists() or not self.d420_bin.exists():
            self.skipTest("Requires local D4.2.0 fixtures")
        if not patcher.HAVE_CRYPTO:
            self.skipTest("PyCryptodome required for container roundtrip")
        raw_dat = encode_container(self.d420_dat.read_bytes())
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
        with self.assertRaises((ValueError, RuntimeError)):
            verify_in_emulator(bytes(damaged), 6000, self.gateway_bin, info)

    def test_boot_clamp_bypass_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        damaged = bytearray(patched)
        offset = find_signatures(data)["boot_clamp_bhs"]
        damaged[offset:offset + 2] = bytes.fromhex("00e0")
        with self.assertRaises((ValueError, RuntimeError)):
            verify_in_emulator(bytes(damaged), 6000, self.gateway_bin, info)

    def test_set_ceiling_bypass_fails_verification(self):
        data = self.ep800_fixture()
        patched, info = patch_binary(data)
        damaged = bytearray(patched)
        offset = find_signatures(data)["check_speed_bhs"]
        damaged[offset:offset + 2] = bytes.fromhex("01e0")
        with self.assertRaises((ValueError, RuntimeError)):
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

class PatcherInputTests(unittest.TestCase):
    """Run without private firmware, PyCryptodome, or Unicorn fixtures."""

    def setUp(self):
        # Production accepts only hash-pinned stock. Test data is admitted by a
        # mock of that allowlist, never by a production command-line override.
        stock = self.admit_image(self.image())
        stock.start()
        self.addCleanup(stock.stop)

    @staticmethod
    def admit_image(image):
        return patch.dict(patcher.STOCK_IMAGES, {
            "synthetic-test": (hashlib.sha256(image).hexdigest(), image[40], bytes(image[16:19])),
        }, clear=True)

    @staticmethod
    def image():
        data = bytearray(1024)
        data[:16] = bytes([255]) * 16
        data[16:19] = bytes.fromhex("420000")
        data[24:28] = len(data).to_bytes(4, "little")
        data[40:44] = bytes.fromhex("23000401")
        data[47:50] = bytes.fromhex("410b00")
        data[128:140] = bytes.fromhex("00b501000800c0b2002805d0")
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

    def test_patch_changes_only_the_eight_byte_constant_return(self):
        data = self.image()
        sites = find_signatures(data)
        for speed in (1500, 2500, 6000, 6500, 9900):
            with self.subTest(speed=speed):
                patched, info = patch_binary(data, speed)
                self.assertEqual(len(patched), len(data))
                self.assertEqual(patched[:128], data[:128])
                self.assertEqual(patched[136:], data[136:])
                self.assertEqual(patched[128:136], encode_mov32(speed))
                self.assertEqual(set(i for i, pair in enumerate(zip(data, patched))
                                     if pair[0] != pair[1]), set(range(128, 136)))
                for site in ("check_speed_bhs", "boot_clamp_bhs"):
                    offset = sites[site]
                    self.assertEqual(patched[offset:offset + 2], data[offset:offset + 2])
                self.assertEqual(info["max_speed_raw"], speed)
                self.assertEqual(info["original_sha256"], hashlib.sha256(data).hexdigest())
                self.assertEqual(info["patched_sha256"], hashlib.sha256(patched).hexdigest())

    def test_unknown_and_previously_modified_images_are_rejected(self):
        patched, _ = patch_binary(self.image(), 6500)
        changed = bytearray(self.image())
        changed[700] ^= 1
        forged_version = bytearray(self.image())
        forged_version[16:19] = bytes.fromhex("430000")
        bypassed = bytearray(self.image())
        bypassed[210:212] = bytes.fromhex("01e0")
        for image in (patched, bytes(changed), bytes(forged_version), bytes(bypassed)):
            with self.subTest(digest=hashlib.sha256(image).hexdigest()):
                with self.assertRaises(ValueError):
                    patch_binary(image, 6500)

    def test_missing_duplicate_and_truncated_signatures_are_rejected(self):
        signatures = (
            (140, bytes.fromhex("022807d004d30428")),
            (200, bytes.fromhex("3800210080b289b2884201d2002621e0")),
            (300, bytes.fromhex("02000b8892b29a4200d2088001bd")),
        )
        for offset, signature in signatures:
            for mutation in ("missing", "duplicate", "truncated"):
                with self.subTest(offset=offset, mutation=mutation):
                    data = bytearray(self.image())
                    if mutation == "duplicate":
                        data[500:500 + len(signature)] = signature
                    else:
                        data[offset:offset + len(signature)] = bytes(len(signature))
                        if mutation == "truncated":
                            data[-2 - len(signature) + 1:-2] = signature[:-1]
                    with self.admit_image(data), self.assertRaises(ValueError):
                        patch_binary(bytes(data), 6500)

    def test_signature_before_function_entry_or_on_odd_address_is_rejected(self):
        signature = bytes.fromhex("022807d004d30428")
        for target in (4, 141):
            with self.subTest(target=target):
                data = bytearray(self.image())
                data[140:148] = bytes(8)
                data[target:target + 8] = signature
                with self.assertRaises(ValueError):
                    find_signatures(bytes(data))

    def test_patch_rejects_malformed_metadata_before_mutation(self):
        bad_images = [self.image()[:63]]
        for offset in (0, 24, 40, 41, 42, 1023):
            image = bytearray(self.image())
            image[offset] ^= 0xff
            bad_images.append(bytes(image))
        for image in bad_images:
            with self.subTest(length=len(image), prefix=image[:28].hex()):
                with self.assertRaises(ValueError):
                    patch_binary(image, 6500)

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
            command = [str(tool), "-i", str(source), "-o", str(output), "--json"]
            for options, expected in (([], 6000), (["-s", "15"], 1500), (["-s", "45.01"], 4501), (["-s", "65"], 6500), (["-s", "99"], 9900)):
                stdout = StringIO()
                with patch.object(sys, "argv", command + options), redirect_stdout(stdout):
                    patcher.main()
                self.assertEqual(json.loads(stdout.getvalue())["patch_info"]["max_speed_raw"], expected)
            for value in ("1500", "65535", "NaN", "inf", "45.001"):
                output.unlink(missing_ok=True)
                with patch.object(sys, "argv", command + ["-s", value]), redirect_stderr(StringIO()):
                    with self.assertRaises(SystemExit) as failure:
                        patcher.main()
                self.assertEqual(failure.exception.code, 2)
                self.assertFalse(output.exists())

    def test_cli_unknown_firmware_fails_without_output(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.bin"
            output = Path(directory) / "patched.bin"
            damaged = bytearray(self.image())
            damaged[700] ^= 1
            source.write_bytes(damaged)
            with patch.object(sys, "argv", ["patch_motor_speed.py", "-i", str(source), "-o", str(output)]):
                with self.assertRaises(ValueError):
                    patcher.main()
            self.assertFalse(output.exists())

    def test_verify_failure_does_not_create_or_overwrite_output(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.bin"
            output = Path(directory) / "new-directory" / "patched.bin"
            source.write_bytes(self.image())
            for existing in (False, True):
                with self.subTest(existing=existing):
                    if existing:
                        output.parent.mkdir()
                        output.write_bytes(b"existing artifact")
                    argv = ["patch_motor_speed.py", "-i", str(source), "-o", str(output), "--verify"]
                    with patch.object(sys, "argv", argv), patch.object(
                        patcher, "verify_in_emulator", side_effect=RuntimeError("forced verification failure")
                    ):
                        with self.assertRaisesRegex(RuntimeError, "forced verification failure"):
                            patcher.main()
                    if existing:
                        self.assertEqual(output.read_bytes(), b"existing artifact")
                    else:
                        self.assertFalse(output.exists())
                        self.assertFalse(output.parent.exists())

    def test_legacy_alias_uses_the_same_patcher_and_cli(self):
        import patch_ep800_speed as alias
        for name in ("encode_mov32", "patch_binary", "verify_in_emulator", "main"):
            with self.subTest(name=name):
                self.assertIs(getattr(alias, name), getattr(patcher, name))
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "source.bin"
            output = Path(directory) / "patched.bin"
            source.write_bytes(self.image())
            alias_path = Path(__file__).resolve().parent.parent / "patch_ep800_speed.py"
            stdout = StringIO()
            with patch.object(sys, "argv", [str(alias_path), "-i", str(source), "-o", str(output), "-s", "65", "--json"]), redirect_stdout(stdout):
                alias.main()
            self.assertEqual(json.loads(stdout.getvalue())["patch_info"]["max_speed_raw"], 6500)
            self.assertEqual(output.read_bytes(), patch_binary(self.image(), 6500)[0])


def execute_constant_thumb1(code):
    """Strictly decode the permitted 16-bit ISA subset, independently of Unicorn.

    Reject all other halfwords, including Thumb-2 prefixes. Instruction bytes can
    contain f2/f6 as legal immediate values; those bytes are not opcode prefixes.
    """
    if len(code) % 2:
        raise ValueError("truncated Thumb instruction")
    r0, returned = None, False
    for offset in range(0, len(code), 2):
        instruction = int.from_bytes(code[offset:offset + 2], "little")
        if instruction == 0xbf00:
            continue
        if returned:
            raise ValueError("non-padding code after return")
        if instruction & 0xff00 == 0x2000:  # MOVS R0, #imm8
            r0 = instruction & 0xff
        elif instruction & 0xf83f == 0 and r0 is not None:  # LSLS R0, R0, #imm5
            r0 = (r0 << ((instruction >> 6) & 31)) & 0xffffffff
        elif instruction & 0xff00 == 0x3000 and r0 is not None:  # ADDS R0, #imm8
            r0 = (r0 + (instruction & 0xff)) & 0xffffffff
        elif instruction == 0x4770 and r0 is not None:  # BX LR
            returned = True
        else:
            raise ValueError(f"unsupported Thumb-1 instruction {instruction:04x}")
    if not returned:
        raise ValueError("missing return")
    return r0


class Thumb1ConstantTests(unittest.TestCase):
    def test_all_16_bit_values_have_strict_thumb1_semantics(self):
        for value in range(0x10000):
            code = encode_mov32(value)
            self.assertEqual(len(code), 8, value)
            self.assertEqual(execute_constant_thumb1(code), value)

    def test_fixed_patch_constants_and_32_bit_boundaries(self):
        for value in (6500, 6000, 2500, 65535, 100, 4608, 65536, 100000, 0x7fffffff, 0x80000000, 0xffffffff):
            with self.subTest(value=value):
                code = encode_mov32(value)
                self.assertEqual(len(code), 8 if value <= 0xffff else 16)
                self.assertEqual(execute_constant_thumb1(code), value)

    def test_f2_and_f6_immediates_are_valid_thumb1(self):
        for value in (0xf2, 0xf6, 0xf200, 0xf600, 0xf2f6, 0xf6f2, 0xf2f6f2f6):
            with self.subTest(value=value):
                self.assertEqual(execute_constant_thumb1(encode_mov32(value)), value)

    def test_original_movw_regression_and_invalid_code_are_rejected(self):
        for code in (
            bytes.fromhex("41f27070704700bf"),  # MOVW R0,#6000; BX LR; NOP
            bytes.fromhex("01217047"),  # MOVS R1,#1 rather than R0
            bytes.fromhex("7047"),  # no initialized return value
            bytes.fromhex("012000"),  # truncated instruction
            bytes.fromhex("012070470130"),  # instructions after return
            bytes.fromhex("0120"),  # no return
        ):
            with self.subTest(code=code.hex()), self.assertRaises(ValueError):
                execute_constant_thumb1(code)

    def test_constant_encoder_rejects_non_uint32_input(self):
        for value in (-1, 0x100000000, 1.5, "6500", None, True):
            with self.subTest(value=value), self.assertRaises(ValueError):
                encode_mov32(value)


if __name__ == "__main__":
    unittest.main()
