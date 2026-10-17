"""Auto-pull and local caching of official Shimano STEPS firmware binaries.

Fetches hash-pinned firmware from official Shimano E-TUBE endpoints,
verifies integrity, decodes encrypted containers into plaintext binaries,
and caches them locally without committing to git.
"""
import hashlib
import json
import os
import struct
import sys
import urllib.request
from pathlib import Path

# Try pycryptodome, then cryptography, then pure-Python AES fallback
try:
    from Crypto.Cipher import AES as _PyCryptodomeAES

    def _decrypt_aes_cbc(key: bytes, iv: bytes, data: bytes) -> bytes:
        return _PyCryptodomeAES.new(key, _PyCryptodomeAES.MODE_CBC, iv).decrypt(data)

except ImportError:
    try:
        from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

        def _decrypt_aes_cbc(key: bytes, iv: bytes, data: bytes) -> bytes:
            cipher = Cipher(algorithms.AES(key), modes.CBC(iv))
            decryptor = cipher.decryptor()
            return decryptor.update(data) + decryptor.finalize()

    except ImportError:
        # Minimal pure-Python AES-128-CBC fallback
        def _decrypt_aes_cbc(key: bytes, iv: bytes, data: bytes) -> bytes:
            s_box = [
                0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
                0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
                0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
                0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
                0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
                0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
                0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
                0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
                0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
                0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
                0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
                0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
                0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
                0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
                0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
                0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
            ]
            inv_s_box = [0] * 256
            for idx, val in enumerate(s_box):
                inv_s_box[val] = idx
            rcon = [0x00, 0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36]

            # Key expansion for 128-bit key
            w = [list(key[i : i + 4]) for i in range(0, 16, 4)]
            for i in range(4, 44):
                temp = list(w[i - 1])
                if i % 4 == 0:
                    temp = temp[1:] + temp[:1]
                    temp = [s_box[b] for b in temp]
                    temp[0] ^= rcon[i // 4]
                w.append([a ^ b for a, b in zip(w[i - 4], temp)])

            def gmult(a, b):
                p = 0
                for _ in range(8):
                    if b & 1:
                        p ^= a
                    hi = a & 0x80
                    a = (a << 1) & 0xFF
                    if hi:
                        a ^= 0x1B
                    b >>= 1
                return p

            def inv_cipher_block(block):
                state = [list(block[i : i + 4]) for i in range(0, 16, 4)]
                # transpose to col-major: state[row][col]
                state = [[state[c][r] for c in range(4)] for r in range(4)]

                # AddRoundKey round 10
                for r in range(4):
                    for c in range(4):
                        state[r][c] ^= w[40 + c][r]

                for rnd in range(9, 0, -1):
                    # InvShiftRows
                    state[1] = state[1][-1:] + state[1][:-1]
                    state[2] = state[2][-2:] + state[2][:-2]
                    state[3] = state[3][-3:] + state[3][:-3]
                    # InvSubBytes
                    for r in range(4):
                        for c in range(4):
                            state[r][c] = inv_s_box[state[r][c]]
                    # AddRoundKey
                    for r in range(4):
                        for c in range(4):
                            state[r][c] ^= w[rnd * 4 + c][r]
                    # InvMixColumns
                    for c in range(4):
                        s0, s1, s2, s3 = state[0][c], state[1][c], state[2][c], state[3][c]
                        state[0][c] = gmult(s0, 0x0E) ^ gmult(s1, 0x0B) ^ gmult(s2, 0x0D) ^ gmult(s3, 0x09)
                        state[1][c] = gmult(s0, 0x09) ^ gmult(s1, 0x0E) ^ gmult(s2, 0x0B) ^ gmult(s3, 0x0D)
                        state[2][c] = gmult(s0, 0x0D) ^ gmult(s1, 0x09) ^ gmult(s2, 0x0E) ^ gmult(s3, 0x0B)
                        state[3][c] = gmult(s0, 0x0B) ^ gmult(s1, 0x0D) ^ gmult(s2, 0x09) ^ gmult(s3, 0x0E)

                # Round 0: InvShiftRows + InvSubBytes + AddRoundKey 0
                state[1] = state[1][-1:] + state[1][:-1]
                state[2] = state[2][-2:] + state[2][:-2]
                state[3] = state[3][-3:] + state[3][:-3]
                for r in range(4):
                    for c in range(4):
                        state[r][c] = inv_s_box[state[r][c]] ^ w[c][r]

                out = bytearray(16)
                for c in range(4):
                    for r in range(4):
                        out[c * 4 + r] = state[r][c]
                return bytes(out)

            out = bytearray(len(data))
            prev = iv
            for off in range(0, len(data), 16):
                blk = data[off : off + 16]
                dec = inv_cipher_block(blk)
                out[off : off + 16] = bytes(a ^ b for a, b in zip(dec, prev))
                prev = blk
            return bytes(out)


SHIMANO_OFFICIAL_URL_BASE = "https://api.shimano.com/etube/public/data/upload/published/"
REQUEST_HEADERS = {
    "User-Agent": "Ktor client",
    "Cache-Control": "no-cache",
}

CONTAINER_MARKER = bytes.fromhex("6f1b529d847c17dbd72af3d337cb3cc3")


def get_default_cache_dir() -> Path:
    env_dir = os.environ.get("CRANKLINK_FW_CACHE")
    if env_dir:
        return Path(env_dir)
    return Path(__file__).resolve().parent.parent / ".cache" / "firmware"


# Canonical mapping for all 18 hash-pinned simulator profiles
CATALOG = {
    # Gateways (Displays)
    "sc-em800-4.0.0": {
        "filename": "SCEM800.4.0.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.0.0",
        "sha256": "6239988c78ea734cee44c7c61477a9498dfa79f38e392d6c38db580687482bc8",
    },
    "sc-em800-4.1.0": {
        "filename": "SCEM800.4.1.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.1.0",
        "sha256": "9a5e8212034c17cb3ce56db59edacf99c69971f952ff13b03fb55ca59e9b3c82",
    },
    "sc-em800-4.3.0": {
        "filename": "SCEM800.4.3.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.3.0",
        "sha256": "0f2c469d62803df67df6bd09d46fdaa4201c71b8f8d9fec6c84ef964b6415e32",
    },
    "sc-em800-4.4.0": {
        "filename": "SCEM800.4.4.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.4.0",
        "sha256": "e473f3326615d83cd2e60124ebe7df73541b4e23bb417b3ac8615ca4dd970617",
    },
    "sc-em800-4.5.0": {
        "filename": "SCEM800.4.5.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.5.0",
        "sha256": "8aa8bbe626c44daeb9ecbe12d0985aee3fdb2c1f0b0ce688325983ee27e0ee11",
    },
    "sc-em800-4.6.0": {
        "filename": "SCEM800.4.6.0.dat",
        "kind": "gateway",
        "model": "SC-EM800",
        "version": "4.6.0",
        "sha256": "c943c90239efbcf36037cb8fa3399ab34d012913774e26728341750b96e30c6f",
    },
    "sc-e6100-4.1.0": {
        "filename": "SCE6100.4.1.0.dat",
        "kind": "gateway",
        "model": "SC-E6100",
        "version": "4.1.0",
        "sha256": "8149fe326ddf14e439c83ab09141cfb2b08bd745938b34fbcf603a3fb8e7f71a",
    },
    "sc-e7000-4.1.0": {
        "filename": "SCE7000.4.1.0.dat",
        "kind": "gateway",
        "model": "SC-E7000",
        "version": "4.1.0",
        "sha256": "1f3c42ad0cc3e46d2e9affd5f023245f645acbc1c8ee3d68569bfcb25c9c3e79",
    },
    # Motors (D images)
    "ep800-4.1.10": {
        "filename": "DUEP800-D.4.1.10.dat",
        "kind": "motor",
        "model": "DU-EP800",
        "version": "4.1.10",
        "sha256": "4ab93e74332427dec30f6dfcfa78693b749603f90558b0f43a0d5c412f8641e7",
    },
    "ep800-4.1.11": {
        "filename": "DUEP800-D.4.1.11.dat",
        "kind": "motor",
        "model": "DU-EP800",
        "version": "4.1.11",
        "sha256": "0afc6abc21ee462197f1f420e40b398cf86bf8230724ffc4cc48dc89b5fe2aa9",
    },
    "ep800-4.2.0": {
        "filename": "DUEP800-D.4.2.0.dat",
        "kind": "motor",
        "model": "DU-EP800",
        "version": "4.2.0",
        "sha256": "f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551",
    },
    "ep600-4.4.1": {
        "filename": "DUEP600-D.4.4.1.dat",
        "kind": "motor",
        "model": "DU-EP600",
        "version": "4.4.1",
        "sha256": "c2bdeb052b7f5d89fcf27ee31cc38827c134aa89e3ecddc983d5cfbe81f96ba5",
    },
    "ep600-4.6.0": {
        "filename": "DUEP600-D.4.6.0.dat",
        "kind": "motor",
        "model": "DU-EP600",
        "version": "4.6.0",
        "sha256": "ccea58cacf7588b0d1a9c6037cde81f25510b4b2fbf59fdaf99b8099d16f3b80",
    },
    "ep500-4.1.0": {
        "filename": "DUEP500-D.4.1.0.dat",
        "kind": "motor",
        "model": "DU-EP500",
        "version": "4.1.0",
        "sha256": "b273042b1f0e69aae28ffcb5fca6be24883f32c322bca6df9d1b6ec2ecb61214",
    },
    "ep500-4.1.1": {
        "filename": "DUEP500-D.4.1.1.dat",
        "kind": "motor",
        "model": "DU-EP500",
        "version": "4.1.1",
        "sha256": "6dd6ebf14f04a2d363cced8f77a8c8f86b64583c71e2abb9b2cc0fe64e2d1952",
    },
    "e6100-4.8.0": {
        "filename": "DUE6100-D.4.8.0.dat",
        "kind": "motor",
        "model": "DU-E6100",
        "version": "4.8.0",
        "sha256": "47fa9c5d52d03987bf594514a60ff96b1b51e4ceaa9960ff8666578aeb8516fb",
    },
    "e5000-4.5.0": {
        "filename": "DUE5000-D.4.5.0.dat",
        "kind": "motor",
        "model": "DU-E5000",
        "version": "4.5.0",
        "sha256": "44806bd54aedff95a88bb73fafe0f0581297f2f67b2ea35545cea012899d90bb",
    },
    "e5100-4.1.0": {
        "filename": "DUE5100-D.4.1.0.dat",
        "kind": "motor",
        "model": "DU-E5100",
        "version": "4.1.0",
        "sha256": "f3bcba12ac9685123fac3bc4c5198b8ae7da64753b8f8804bbfdbee87e0207fe",
    },
}

# Hardware presets (pairing a motor with default compatible gateway)
HARDWARE_PAIRS = {
    "ep800": ("sc-em800-4.6.0", "ep800-4.2.0"),
    "du-ep800": ("sc-em800-4.6.0", "ep800-4.2.0"),
    "ep600": ("sc-em800-4.6.0", "ep600-4.6.0"),
    "du-ep600": ("sc-em800-4.6.0", "ep600-4.6.0"),
    "ep500": ("sc-em800-4.6.0", "ep500-4.1.1"),
    "du-ep500": ("sc-em800-4.6.0", "ep500-4.1.1"),
    "e6100": ("sc-e6100-4.1.0", "e6100-4.8.0"),
    "du-e6100": ("sc-e6100-4.1.0", "e6100-4.8.0"),
    "e5000": ("sc-e6100-4.1.0", "e5000-4.5.0"),
    "du-e5000": ("sc-e6100-4.1.0", "e5000-4.5.0"),
    "e5100": ("sc-e6100-4.1.0", "e5100-4.1.0"),
    "du-e5100": ("sc-e6100-4.1.0", "e5100-4.1.0"),
    "e7000": ("sc-e7000-4.1.0", "ep800-4.2.0"),
    "sc-e7000": ("sc-e7000-4.1.0", "ep800-4.2.0"),
}

DEFAULT_VERSION_BY_MODEL = {
    "ep800": "ep800-4.2.0",
    "du-ep800": "ep800-4.2.0",
    "ep600": "ep600-4.6.0",
    "du-ep600": "ep600-4.6.0",
    "ep500": "ep500-4.1.1",
    "du-ep500": "ep500-4.1.1",
    "e6100": "e6100-4.8.0",
    "du-e6100": "e6100-4.8.0",
    "e5000": "e5000-4.5.0",
    "du-e5000": "e5000-4.5.0",
    "e5100": "e5100-4.1.0",
    "du-e5100": "e5100-4.1.0",
    "sc-em800": "sc-em800-4.6.0",
    "em800": "sc-em800-4.6.0",
    "sc-e6100": "sc-e6100-4.1.0",
    "sc-e7000": "sc-e7000-4.1.0",
}


def _common_key(md5: bytes) -> bytes:
    seed = int.from_bytes(md5[:6], "little")
    a = (seed >> 0) & 0xFFFF
    b = (seed >> 16) & 0xFFFF
    c = (seed >> 32) & 0xFFFF
    x = 123456789 ^ ((a << 16) | b)
    y = 362436069
    z = 521288629
    w = 88675123 ^ ((b << 16) | c)

    def step():
        nonlocal x, y, z, w
        t = (x ^ (x << 11)) & 0xFFFFFFFF
        x, y, z, w = y, z, w, (w ^ (w >> 19) ^ t ^ (t >> 8)) & 0xFFFFFFFF
        return w

    for _ in range(a >> 8):
        step()
    return b"".join(struct.pack("<I", step()) for _ in range(4))


def decode_container(raw: bytes) -> bytes:
    """Decode raw Shimano firmware container into plaintext executable bytes."""
    if not raw.endswith(CONTAINER_MARKER):
        return raw

    if len(raw) < 68 or (len(raw) - 52) % 16 != 0:
        raise ValueError("Invalid encrypted container length")

    declared = struct.unpack("<I", raw[-20:-16])[0]
    md5 = raw[-36:-20]
    key = _common_key(md5)
    iv = raw[:16]
    ciphertext = raw[16:-36]

    padded = _decrypt_aes_cbc(key, iv, ciphertext)
    pad = padded[-1]
    if not 1 <= pad <= 16 or padded[-pad:] != bytes([pad]) * pad:
        raise ValueError("Invalid PKCS7 padding")

    plain = padded[:-pad]
    if len(plain) != declared:
        raise ValueError(f"Declared plaintext size mismatch: declared {declared}, got {len(plain)}")
    if hashlib.md5(plain).digest() != md5:
        raise ValueError("Plaintext MD5 checksum mismatch")

    return plain


def resolve_profile_id(name: str, kind: str = None) -> str:
    """Resolve user input (profile id, filename, model, or alias) to canonical profile id."""
    lower = name.lower().strip()

    # Exact profile id match
    if lower in CATALOG:
        if kind and CATALOG[lower]["kind"] != kind:
            raise ValueError(f"Profile '{lower}' is kind '{CATALOG[lower]['kind']}', expected '{kind}'")
        return lower

    # Match by filename (e.g. DUEP800-D.4.2.0.dat or SCEM800.4.6.0.dat)
    for pid, meta in CATALOG.items():
        if meta["filename"].lower() == lower or meta["filename"].lower().replace(".dat", "") == lower:
            if kind and meta["kind"] != kind:
                continue
            return pid

    # Match by model alias (e.g. "ep800", "sc-em800")
    if lower in DEFAULT_VERSION_BY_MODEL:
        pid = DEFAULT_VERSION_BY_MODEL[lower]
        if kind and CATALOG[pid]["kind"] != kind:
            raise ValueError(f"Model alias '{lower}' resolved to '{pid}' ({CATALOG[pid]['kind']}), expected '{kind}'")
        return pid

    # Match prefix/version pattern e.g. "ep800:4.1.10" or "ep800@4.1.10" or "ep800 4.1.10"
    for sep in [":", "@", " ", "_"]:
        if sep in lower:
            parts = lower.split(sep, 1)
            candidate = f"{parts[0]}-{parts[1]}"
            if candidate in CATALOG:
                return candidate

    raise ValueError(f"Unknown firmware name, profile ID, or hardware model: '{name}'")


def pull_firmware(name: str, cache_dir: Path = None, kind: str = None, force: bool = False, verbose: bool = True) -> Path:
    """Pull firmware by profile ID, model name, or Shimano filename, decode and cache locally."""
    pid = resolve_profile_id(name, kind=kind)
    meta = CATALOG[pid]
    cache = Path(cache_dir) if cache_dir else get_default_cache_dir()
    cache.mkdir(parents=True, exist_ok=True)

    decoded_filename = f"{meta['filename']}.decoded.bin"
    decoded_path = cache / decoded_filename
    raw_path = cache / meta["filename"]

    # Check existing decoded file
    if not force and decoded_path.exists():
        data = decoded_path.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest == meta["sha256"]:
            if verbose:
                print(f"[cached] {pid:16} -> {decoded_path}")
            return decoded_path

    # Check raw container if already cached
    raw_bytes = None
    if not force and raw_path.exists():
        raw_candidate = raw_path.read_bytes()
        try:
            plain_candidate = decode_container(raw_candidate)
            if hashlib.sha256(plain_candidate).hexdigest() == meta["sha256"]:
                raw_bytes = raw_candidate
                plain = plain_candidate
        except Exception:
            raw_bytes = None

    if raw_bytes is None:
        url = SHIMANO_OFFICIAL_URL_BASE + meta["filename"]
        if verbose:
            print(f"[download] Fetching {meta['filename']} from {url}...")
        req = urllib.request.Request(url, headers=REQUEST_HEADERS)
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                raw_bytes = resp.read()
        except Exception as err:
            raise RuntimeError(f"Failed to download {meta['filename']} from Shimano: {err}") from err

        raw_path.write_bytes(raw_bytes)
        plain = decode_container(raw_bytes)

    digest = hashlib.sha256(plain).hexdigest()
    if digest != meta["sha256"]:
        raise ValueError(
            f"Integrity failure for {meta['filename']}: expected SHA-256 {meta['sha256']}, got {digest}"
        )

    decoded_path.write_bytes(plain)
    if verbose:
        print(f"[ready]  {pid:16} ({meta['kind']}) -> {decoded_path} ({len(plain):,} bytes)")

    return decoded_path


def resolve_firmware_target(target: str, kind: str = None, cache_dir: Path = None, verbose: bool = True) -> Path:
    """Resolve a target: if already an existing file, use it (and decode if needed); otherwise auto-pull."""
    p = Path(target)
    if p.exists() and p.is_file():
        raw = p.read_bytes()
        # If already decoded plaintext matching profile hash
        digest = hashlib.sha256(raw).hexdigest()
        for pid, meta in CATALOG.items():
            if meta["sha256"] == digest:
                return p
        # If it's an encrypted .dat container, decode it into cache
        if raw.endswith(CONTAINER_MARKER):
            cache = Path(cache_dir) if cache_dir else get_default_cache_dir()
            cache.mkdir(parents=True, exist_ok=True)
            plain = decode_container(raw)
            plain_digest = hashlib.sha256(plain).hexdigest()
            out_path = cache / f"{p.name}.decoded.bin"
            out_path.write_bytes(plain)
            return out_path
        return p

    # Otherwise auto-pull by name
    return pull_firmware(target, cache_dir=cache_dir, kind=kind, verbose=verbose)


def resolve_hardware_pair(
    hardware: str,
    gateway_override: str = None,
    motor_override: str = None,
    cache_dir: Path = None,
    verbose: bool = True,
) -> tuple[Path, Path]:
    """Resolve and auto-pull a matching gateway/motor pair for given hardware."""
    hw_lower = hardware.lower().strip()
    if hw_lower not in HARDWARE_PAIRS:
        supported = ", ".join(sorted(HARDWARE_PAIRS.keys()))
        raise ValueError(f"Unknown hardware preset '{hardware}'. Supported presets: {supported}")

    default_gw, default_mot = HARDWARE_PAIRS[hw_lower]
    gw_name = gateway_override or default_gw
    mot_name = motor_override or default_mot

    gw_path = resolve_firmware_target(gw_name, kind="gateway", cache_dir=cache_dir, verbose=verbose)
    mot_path = resolve_firmware_target(mot_name, kind="motor", cache_dir=cache_dir, verbose=verbose)
    return gw_path, mot_path


def pull_all(cache_dir: Path = None, fixtures_output: Path = None, verbose: bool = True) -> dict[str, str]:
    """Pull and decode all 18 profiled firmware images and optionally generate a fixture manifest."""
    manifest = {}
    for pid in sorted(CATALOG.keys()):
        path = pull_firmware(pid, cache_dir=cache_dir, verbose=verbose)
        manifest[pid] = str(path.resolve())

    if fixtures_output:
        out_p = Path(fixtures_output)
        out_p.parent.mkdir(parents=True, exist_ok=True)
        out_p.write_text(json.dumps(manifest, indent=2) + "\n")
        if verbose:
            print(f"[manifest] Saved fixtures manifest to {out_p}")

    return manifest


def list_catalog(cache_dir: Path = None) -> list[dict]:
    """List all available firmware versions with local cache status."""
    cache = Path(cache_dir) if cache_dir else get_default_cache_dir()
    rows = []
    for pid, meta in sorted(CATALOG.items()):
        cached = (cache / f"{meta['filename']}.decoded.bin").exists()
        rows.append(
            {
                "id": pid,
                "kind": meta["kind"],
                "model": meta["model"],
                "version": meta["version"],
                "filename": meta["filename"],
                "cached": cached,
                "sha256": meta["sha256"],
            }
        )
    return rows
