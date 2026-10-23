#!/usr/bin/env python3
"""Unit tests for firmware auto-pull, container decoding, and local caching."""
import hashlib
import json
import struct
import tempfile
import unittest
from pathlib import Path

from fw_sim.pull import (
    CATALOG,
    HARDWARE_PAIRS,
    CONTAINER_MARKER,
    _common_key,
    _decrypt_aes_cbc,
    decode_container,
    resolve_profile_id,
    resolve_hardware_pair,
    resolve_firmware_target,
    pull_firmware,
    list_catalog,
)


class TestFirmwarePull(unittest.TestCase):
    def test_catalog_completeness(self):
        """Catalog must contain all 18 hash-pinned profiles."""
        self.assertEqual(len(CATALOG), 18)
        gateways = [k for k, v in CATALOG.items() if v["kind"] == "gateway"]
        motors = [k for k, v in CATALOG.items() if v["kind"] == "motor"]
        self.assertEqual(len(gateways), 8)
        self.assertEqual(len(motors), 10)

    def test_catalog_hashes_match_simulator_profiles(self):
        """Pull only fixtures the simulator will accept: one source of truth per image."""
        profiles = {p["id"]: p for p in json.loads((Path(__file__).parent / "profiles.json").read_text())}
        for profile_id, meta in CATALOG.items():
            with self.subTest(profile_id=profile_id):
                self.assertIn(profile_id, profiles)
                self.assertEqual(meta["sha256"], profiles[profile_id]["sha256"])
                self.assertEqual(meta["kind"], profiles[profile_id]["kind"])

    def test_resolve_profile_id_exact(self):
        self.assertEqual(resolve_profile_id("ep800-4.2.0"), "ep800-4.2.0")
        self.assertEqual(resolve_profile_id("sc-em800-4.6.0"), "sc-em800-4.6.0")
        self.assertEqual(resolve_profile_id("sc-e6100-4.1.0"), "sc-e6100-4.1.0")

    def test_resolve_profile_id_aliases(self):
        self.assertEqual(resolve_profile_id("ep800"), "ep800-4.2.0")
        self.assertEqual(resolve_profile_id("DU-EP800"), "ep800-4.2.0")
        self.assertEqual(resolve_profile_id("ep600"), "ep600-4.6.0")
        self.assertEqual(resolve_profile_id("sc-em800"), "sc-em800-4.6.0")
        self.assertEqual(resolve_profile_id("em800"), "sc-em800-4.6.0")
        self.assertEqual(resolve_profile_id("e6100"), "e6100-4.8.0")
        self.assertEqual(resolve_profile_id("e5000"), "e5000-4.5.0")
        self.assertEqual(resolve_profile_id("e5100"), "e5100-4.1.0")
        self.assertEqual(resolve_profile_id("sc-e7000"), "sc-e7000-4.1.0")

    def test_resolve_profile_id_filenames(self):
        self.assertEqual(resolve_profile_id("DUEP800-D.4.2.0.dat"), "ep800-4.2.0")
        self.assertEqual(resolve_profile_id("SCEM800.4.6.0.dat"), "sc-em800-4.6.0")
        self.assertEqual(resolve_profile_id("SCE6100.4.1.0.dat"), "sc-e6100-4.1.0")

    def test_resolve_profile_id_version_delimiters(self):
        self.assertEqual(resolve_profile_id("ep800:4.1.10"), "ep800-4.1.10")
        self.assertEqual(resolve_profile_id("ep800@4.1.11"), "ep800-4.1.11")
        self.assertEqual(resolve_profile_id("ep600 4.4.1"), "ep600-4.4.1")

    def test_resolve_profile_id_kind_enforcement(self):
        self.assertEqual(resolve_profile_id("ep800-4.2.0", kind="motor"), "ep800-4.2.0")
        with self.assertRaises(ValueError):
            resolve_profile_id("ep800-4.2.0", kind="gateway")

    def test_resolve_profile_id_unknown(self):
        with self.assertRaises(ValueError):
            resolve_profile_id("unknown-device-1.0.0")

    def test_resolve_hardware_pairs(self):
        for hw in ["ep800", "ep600", "ep500", "e6100", "e5000", "e5100"]:
            self.assertIn(hw, HARDWARE_PAIRS)
            gw_id, mot_id = HARDWARE_PAIRS[hw]
            self.assertEqual(CATALOG[gw_id]["kind"], "gateway")
            self.assertEqual(CATALOG[mot_id]["kind"], "motor")

    def test_resolve_hardware_unknown(self):
        with self.assertRaises(ValueError):
            resolve_hardware_pair("nonexistent-motor")

    def test_decode_plaintext_pass_through(self):
        plain = b"arbitrary-plaintext-firmware-bytes"
        self.assertEqual(decode_container(plain), plain)

    def test_decode_encrypted_container_roundtrip(self):
        # Construct synthetic valid Shimano encrypted container
        payload = b"Hello, Shimano firmware simulation payload!"
        md5_digest = hashlib.md5(payload).digest()
        key = _common_key(md5_digest)
        iv = b"\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10"

        # PKCS7 padding
        pad_len = 16 - (len(payload) % 16)
        padded_payload = payload + bytes([pad_len]) * pad_len

        # Encrypt with AES-CBC
        try:
            from Crypto.Cipher import AES
            cipher = AES.new(key, AES.MODE_CBC, iv)
            ciphertext = cipher.encrypt(padded_payload)
        except ImportError:
            # Skip test if crypto cannot encrypt
            return

        container = bytearray()
        container.extend(iv)
        container.extend(ciphertext)
        container.extend(md5_digest)
        container.extend(struct.pack("<I", len(payload)))
        container.extend(CONTAINER_MARKER)

        decoded = decode_container(bytes(container))
        self.assertEqual(decoded, payload)

    def test_decode_container_corrupt_rejections(self):
        # Truncated container
        with self.assertRaises(ValueError):
            decode_container(b"short" + CONTAINER_MARKER)

        # Invalid declared length
        iv = bytes(16)
        dummy_ct = bytes(16)
        md5 = hashlib.md5(b"").digest()
        container = iv + dummy_ct + md5 + struct.pack("<I", 99999) + CONTAINER_MARKER
        with self.assertRaises(ValueError):
            decode_container(container)

    def test_list_catalog_and_caching(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_cache = Path(tmp_dir)
            catalog_rows = list_catalog(cache_dir=tmp_cache)
            self.assertEqual(len(catalog_rows), 18)
            self.assertTrue(all(not r["cached"] for r in catalog_rows))

            # Simulate caching an image
            test_prof = CATALOG["ep800-4.2.0"]
            decoded_file = tmp_cache / f"{test_prof['filename']}.decoded.bin"
            decoded_file.write_bytes(bytes.fromhex("00") * 100)

            catalog_after = list_catalog(cache_dir=tmp_cache)
            cached_item = [r for r in catalog_after if r["id"] == "ep800-4.2.0"][0]
            self.assertTrue(cached_item["cached"])

    def test_cached_firmware_retrieval_without_network(self):
        with tempfile.TemporaryDirectory() as tmp_dir:
            tmp_cache = Path(tmp_dir)
            test_prof = CATALOG["ep800-4.2.0"]

            # Pre-populate cache with exact hash-matching content
            dummy_content = b"ep800-4.2.0-mock-data"
            # Override profile hash for testing
            orig_hash = test_prof["sha256"]
            test_prof["sha256"] = hashlib.sha256(dummy_content).hexdigest()
            try:
                decoded_file = tmp_cache / f"{test_prof['filename']}.decoded.bin"
                decoded_file.write_bytes(dummy_content)

                path = pull_firmware("ep800-4.2.0", cache_dir=tmp_cache, verbose=False)
                self.assertEqual(path, decoded_file)
                self.assertEqual(path.read_bytes(), dummy_content)
            finally:
                test_prof["sha256"] = orig_hash


if __name__ == "__main__":
    unittest.main()
