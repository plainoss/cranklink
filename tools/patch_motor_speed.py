#!/usr/bin/env python3
"""Shimano STEPS Firmware Speed Limit Patcher and Emulator Verification Tool.

Patches DCASX motor firmware binaries (.bin or encrypted .dat) for offline
speed-limit research in the tested 15.00..99.00 km/h range.

Supported drive unit families:
- DU-EP800 / DU-EP800-CRG (Series 0x23, versions D4.1.10, D4.1.11, D4.2.0)
- DU-E6100 family (Series 0x20, version D4.8.0)
- DU-E5100 family (Series 0x38, version D4.1.0)
- DU-E5000 family (Series 0x22, version D4.5.0)

The patch operates on 3 key locations in the firmware:
1. get_max_speed_limit: Replaces destination-based speed limit lookup with returning
   the configured maximum speed limit (default 6000 / 60.00 km/h)
   using position-independent 16-bit Thumb instructions.
2. check_speed_limit: Retains the stock conditional upper bound check.
3. boot_clamp: Retains the stock boot-time clamp to the configured ceiling.

Analyzes flash layout offline and verifies selected application routines in Unicorn.
"""

import argparse
import hashlib
from decimal import Decimal, InvalidOperation
from pathlib import Path
import struct
import sys
import tempfile

# Optional import of PyCryptodome for container handling
try:
    from Crypto.Cipher import AES
    HAVE_CRYPTO = True
except ImportError:
    HAVE_CRYPTO = False

MARKER = bytes([0x6f, 0x1b, 0x52, 0x9d, 0x84, 0x7c, 0x17, 0xdb, 0xd7, 0x2a, 0xf3, 0xd3, 0x37, 0xcb, 0x3c, 0xc3])
MAX_IMAGE_BYTES = 16 * 1024 * 1024
MAX_CONTAINER_BYTES = MAX_IMAGE_BYTES + 68

# Decoded, unmodified images only. Container IVs may differ; the payload cannot.
STOCK_IMAGES = {
    "ep800-4.2.0": ("f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551", 0x23, bytes.fromhex("420000")),
    "ep800-4.1.11": ("0afc6abc21ee462197f1f420e40b398cf86bf8230724ffc4cc48dc89b5fe2aa9", 0x23, bytes.fromhex("410b00")),
    "ep800-4.1.10": ("4ab93e74332427de4f035b32bee35a9fe4600d281f4434d39ddc5de8f4b379fb", 0x23, bytes.fromhex("410a00")),
    "e6100-4.8.0": ("47fa9c5d52d03987c919d623ee6cb9aae7182548084c51115cffbe7cb3dade7c", 0x20, bytes.fromhex("480000")),
    "e5100-4.1.0": ("f3bcba12ac9685123fac3bc4c5198b8ae7da64753b8f8804bbfdbee87e0207fe", 0x38, bytes.fromhex("410000")),
    "e5000-4.5.0": ("44806bd54aedff95a88bb73fafe0f0581297f2f67b2ea35545cea012899d90bb", 0x22, bytes.fromhex("450000")),
}
ORIGINAL_PREAMBLE = bytes.fromhex("00b501000800c0b2002805d0")

MOTOR_SERIES_NAMES = {
    0x23: "DU-EP800 / DU-EP800-CRG",
    0x20: "DU-E6100",
    0x38: "DU-E5100",
    0x22: "DU-E5000",
    0x30: "DU-EP600 / DU-EP600-CRG",
    0x37: "DU-EP500",
    0x35: "DU-EP801 / DU-EP801-CRG",
}

PATCHABLE_SERIES = {0x23, 0x20, 0x38, 0x22}
# Conservative tested envelope. The stock assist routine uses signed halfwords;
# accepting the full unsigned 16-bit range causes wrapping and zero thresholds.
MIN_SPEED_RAW, MAX_SPEED_RAW, DEFAULT_SPEED_RAW = 1500, 9900, 6000


def validate_speed_raw(value: int) -> None:
    if type(value) is not int or not MIN_SPEED_RAW <= value <= MAX_SPEED_RAW:
        raise ValueError("Speed limit must be 1500..9900 raw (15.00..99.00 km/h)")


def parse_speed_limit(value: str) -> int:
    """Parse km/h only, rejecting non-finite values and sub-cent precision."""
    try:
        speed = Decimal(value)
        if not speed.is_finite() or not Decimal("15") <= speed <= Decimal("99"):
            raise ValueError("Speed limit must be 15.00..99.00 km/h")
        raw = int(speed * 100)
        if speed != Decimal(raw) / 100:
            raise ValueError("Speed limit must use at most two decimal places")
        return raw
    except (InvalidOperation, ValueError) as error:
        raise argparse.ArgumentTypeError(str(error)) from error


def common_key(md5: bytes) -> bytes:
    """PRNG-based AES key derivation matching Cranklink and Shimano firmware containers."""
    a = int.from_bytes(md5[0:2], "little")
    b = int.from_bytes(md5[2:4], "little")
    c = int.from_bytes(md5[4:6], "little")
    state = [123456789 ^ ((a << 16) | b), 362436069, 521288629, 88675123 ^ ((b << 16) | c)]

    def step():
        t = (state[0] ^ (state[0] << 11)) & 0xffffffff
        state[0] = state[1]
        state[1] = state[2]
        state[2] = state[3]
        state[3] = (state[3] ^ (state[3] >> 19) ^ t ^ (t >> 8)) & 0xffffffff
        return state[3].to_bytes(4, "little")

    for _ in range(a >> 8):
        step()
    return b"".join(step() for _ in range(4))


def is_container(raw: bytes) -> bool:
    """Check if raw data is an encrypted Shimano .dat container."""
    return raw.endswith(MARKER)


def read_bounded(path: Path, limit: int = MAX_CONTAINER_BYTES) -> bytes:
    with path.open("rb") as stream:
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError(f"Input exceeds {limit} byte size limit")
    return data


def decode_container(raw: bytes) -> bytes:
    """Decrypt a Shimano .dat container to raw binary payload."""
    if not HAVE_CRYPTO:
        raise RuntimeError("PyCryptodome required to decrypt .dat container files")
    if not is_container(raw) or len(raw) < 68 or len(raw) > MAX_CONTAINER_BYTES or (len(raw) - 52) % 16 != 0:
        raise ValueError("Invalid encrypted container length")
    footer = len(raw) - 36
    md5 = raw[footer:footer + 16]
    declared = int.from_bytes(raw[footer + 16:footer + 20], "little")
    if declared > MAX_IMAGE_BYTES:
        raise ValueError("Declared container size exceeds limit")
    key = common_key(md5)
    iv = raw[:16]
    cipher = AES.new(key, AES.MODE_CBC, iv=iv)
    plain = cipher.decrypt(raw[16:footer])
    pad = plain[-1]
    if not (1 <= pad <= 16) or not all(b == pad for b in plain[-pad:]):
        raise ValueError("Invalid PKCS7 padding")
    plain = plain[:-pad]
    if len(plain) != declared:
        raise ValueError(f"Declared size mismatch: declared {declared}, got {len(plain)}")
    if hashlib.md5(plain).digest() != md5:
        raise ValueError("Plaintext MD5 mismatch")
    return plain


def encode_container(plain: bytes) -> bytes:
    """Encrypt a raw binary payload into a Shimano .dat container."""
    if not HAVE_CRYPTO:
        raise RuntimeError("PyCryptodome required to encrypt .dat container files")
    if len(plain) > MAX_IMAGE_BYTES:
        raise ValueError("Plaintext image exceeds size limit")
    md5 = hashlib.md5(plain).digest()
    key = common_key(md5)
    iv = b"\x12" * 16
    pad = 16 - (len(plain) % 16)
    padded = plain + bytes([pad] * pad)
    cipher = AES.new(key, AES.MODE_CBC, iv=iv)
    encrypted = cipher.encrypt(padded)
    return iv + encrypted + md5 + len(plain).to_bytes(4, "little") + MARKER


def validate_dcas_header(data: bytes) -> dict:
    """Validate DCASX header parameters for Shimano drive unit firmware."""
    if len(data) < 64:
        raise ValueError("Firmware header truncated (minimum 64 bytes)")
    if len(data) > MAX_IMAGE_BYTES:
        raise ValueError("Firmware image exceeds size limit")
    if not data.endswith(b"et"):
        raise ValueError("Firmware missing expected 'et' trailer")

    if not all(b == 0xff for b in data[:16]):
        raise ValueError("Expected DCASX header with 16 FF prefix bytes")

    major = data[16] >> 4
    minor = data[16] & 0x0f
    patch = data[17]
    build = data[18]
    version_str = f"{major}.{minor}.{patch}.{build}"

    compat_major = data[47] >> 4
    compat_minor = data[47] & 0x0f
    compat_patch = data[48]
    compat_build = data[49]
    compat_str = f"{compat_major}.{compat_minor}.{compat_patch}.{compat_build}"

    series = data[40]
    unit = data[41]
    board = data[43]
    declared = int.from_bytes(data[24:28], "little")

    model_name = MOTOR_SERIES_NAMES.get(series, f"Unknown (Series {series:#02x})")

    if unit != 0x00:
        raise ValueError(f"Not a drive unit firmware: unit {unit:#02x} (expected 0x00)")
    if data[42] != 4:
        raise ValueError("Unsupported DCASX image architecture")
    if series not in PATCHABLE_SERIES:
        if series in (0x30, 0x37, 0x35):
            raise ValueError(
                f"{model_name} is a modern basic-mode architecture. In modern firmware, speed limits "
                "are governed directly by market destination (e.g. US Class 3 / 45 km/h via destination 5) "
                "rather than legacy 0x16b0 opcode checks."
            )
        raise ValueError(f"Unsupported drive unit series {series:#02x} ({model_name})")

    if declared != len(data):
        raise ValueError(f"Header declared size mismatch: declared {declared}, actual {len(data)}")

    return {
        "model": model_name,
        "version": version_str,
        "compatibility_version": compat_str,
        "series": series,
        "unit": unit,
        "board": board,
        "size": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def validate_stock_image(data: bytes) -> dict:
    """Require a complete, hash-pinned stock image before changing any byte."""
    metadata = validate_dcas_header(data)
    matches = [profile_id for profile_id, (digest, series, version) in STOCK_IMAGES.items()
               if digest == metadata["sha256"] and series == data[40] and version == data[16:19]]
    if len(matches) != 1:
        raise ValueError("Unknown or modified firmware; an exact supported stock image is required")
    return dict(metadata, profile_id=matches[0])


def encode_mov32(val: int) -> bytes:
    """Encode position-independent Thumb (ARMv6-M compliant) instructions to load a constant and return.

    Emits 16-bit Thumb-1 instructions (movs, lsls, adds, bx lr) compatible with Cortex-M0/M0+
    as well as Cortex-M3/M4. Avoids ARMv7-M Thumb-2 movw/movt which raise HardFault on ARMv6-M.
    For 16-bit values (<= 0xffff), returns exactly 8 bytes.
    """
    if type(val) is not int or not 0 <= val <= 0xffffffff:
        raise ValueError("Constant must be an unsigned 32-bit integer")
    if (val >> 16) != 0:
        # Full 32-bit constant on ARMv6-M (16 bytes):
        b3 = (val >> 24) & 0xff
        b2 = (val >> 16) & 0xff
        b1 = (val >> 8) & 0xff
        b0 = val & 0xff
        return bytes([
            b3, 0x20,        # movs r0, #b3
            0x00, 0x02,        # lsls r0, r0, #8
            b2, 0x30,        # adds r0, #b2
            0x00, 0x02,        # lsls r0, r0, #8
            b1, 0x30,        # adds r0, #b1
            0x00, 0x02,        # lsls r0, r0, #8
            b0, 0x30,        # adds r0, #b0
            0x70, 0x47,        # bx lr
        ])

    hi = (val >> 8) & 0xff
    lo = val & 0xff

    if hi == 0:
        # movs r0, #lo; bx lr; nop; nop (8 bytes)
        return bytes([lo, 0x20, 0x70, 0x47, 0x00, 0xbf, 0x00, 0xbf])
    elif lo == 0:
        # movs r0, #hi; lsls r0, r0, #8; bx lr; nop (8 bytes)
        return bytes([hi, 0x20, 0x00, 0x02, 0x70, 0x47, 0x00, 0xbf])
    else:
        # movs r0, #hi; lsls r0, r0, #8; adds r0, #lo; bx lr (8 bytes)
        return bytes([hi, 0x20, 0x00, 0x02, lo, 0x30, 0x70, 0x47])


def find_signatures(data: bytes) -> dict:
    """Locate unique aligned code sites in stock or canonically patched images."""
    def unique(pattern: bytes, label: str) -> int:
        offset = data.find(pattern)
        if offset < 0 or data.find(pattern, offset + 1) >= 0:
            raise ValueError(f"Expected one {label} signature")
        if offset < 64 or offset % 2:
            raise ValueError(f"Invalid {label} signature alignment or bounds")
        return offset

    # Target 1: get_max_speed_limit table dispatch
    # Signature: cmp r0, #2; beq ...; blo ...; cmp r0, #4
    sig1 = bytes.fromhex("022807d004d30428")
    fn_max_speed = unique(sig1, "get_max_speed_limit") - 12
    if fn_max_speed < 64 or data[fn_max_speed + 8:fn_max_speed + 12] != ORIGINAL_PREAMBLE[8:]:
        raise ValueError("Unrecognized get_max_speed_limit function bounds or preamble")

    # Target 2: check_speed_limit in SET speed limit handler
    prefix2 = bytes.fromhex("3800210080b289b2884201d2002621e0")
    idx2 = unique(prefix2, "check_speed_limit")
    bhs_offset_check = idx2 + 10

    # Target 3: boot_clamp (clamps RAM speed limit cache to max speed on boot)
    prefix3 = bytes.fromhex("02000b8892b29a4200d2088001bd")
    idx3 = unique(prefix3, "boot_clamp")
    bhs_offset_clamp = idx3 + 8

    return {
        "max_speed_fn": fn_max_speed,
        "check_speed_bhs": bhs_offset_check,
        "boot_clamp_bhs": bhs_offset_clamp,
    }


def patch_binary(data: bytes, max_speed_raw: int = DEFAULT_SPEED_RAW) -> tuple[bytes, dict]:
    """Patch the ceiling while retaining runtime and startup enforcement.

    Args:
        data: Plaintext DCASX binary bytes.
        max_speed_raw: Target ceiling in 0.01 km/h units (1500..9900).

    Returns:
        (patched_data, patch_info)
    """
    validate_speed_raw(max_speed_raw)
    validate_stock_image(data)
    sigs = find_signatures(data)

    fn_offset = sigs["max_speed_fn"]
    if data[fn_offset:fn_offset + 12] != ORIGINAL_PREAMBLE:
        raise ValueError("Unrecognized stock get_max_speed_limit preamble")
    patch1 = encode_mov32(max_speed_raw)
    if len(patch1) != 8:
        raise ValueError("The ceiling patch must fit exactly eight bytes")
    patched = bytearray(data)
    patched[fn_offset:fn_offset + len(patch1)] = patch1

    # Both stock bounds remain unchanged; find_signatures validates them.
    check_offset = sigs["check_speed_bhs"]
    clamp_offset = sigs["boot_clamp_bhs"]

    info = {
        "max_speed_raw": max_speed_raw,
        "max_speed_kmh": max_speed_raw / 100.0,
        "patch1_max_speed_fn_offset": hex(fn_offset + 0x10000),
        "patch2_check_speed_offset": hex(check_offset + 0x10000),
        "patch3_boot_clamp_offset": hex(clamp_offset + 0x10000),
        "original_sha256": hashlib.sha256(data).hexdigest(),
        "patched_sha256": hashlib.sha256(patched).hexdigest(),
    }
    return bytes(patched), info


def analyze_flash_layout(data: bytes) -> dict:
    """Validate the image header and calculate offline transfer dimensions.

    No bootloader, transport, erase, or transfer state machine is executed here.
    These calculations cannot establish flashability or executable integrity.
    """
    validate_dcas_header(data)
    if len(data) > 256 * 2048:
        raise ValueError("Invalid DCASX image length")
    if data[40] not in PATCHABLE_SERIES or data[41] != 0x00 or data[42] != 4:
        raise ValueError("Invalid DCAS image compatibility header")

    erase_code = (len(data) - 1) // 2048
    return {
        "status": "analyzed",
        "bootloader_executed": False,
        "total_bytes": len(data),
        "total_blocks": (len(data) + 63) // 64,
        "erase_code": erase_code,
        "erase_sectors": erase_code + 1,
        "image_checksum": sum(data) & 0xff,
    }


def verify_flash_simulation(data: bytes) -> dict:
    """Compatibility alias; returns offline analysis, never a flash verdict."""
    return analyze_flash_layout(data)


def find_matching_profile(data: bytes, profiles: list[dict]) -> dict:
    """Match a stock image or exact eight-byte patch to its pinned base profile."""
    metadata, _ = validate_patched_image(data)
    matches = [profile for profile in profiles
               if profile["id"] == metadata["profile_id"]
               and profile["sha256"] == metadata["sha256"] and profile["kind"] == "motor"]
    if len(matches) != 1:
        raise ValueError("No exact simulation profile for this firmware image")
    return matches[0]


def validate_patched_image(data: bytes, max_speed_raw: int | None = None) -> tuple[dict, dict]:
    """Reconstruct and hash the stock source, accepting only a canonical patch."""
    validate_dcas_header(data)
    signatures = find_signatures(data)
    offset = signatures["max_speed_fn"]
    code = data[offset:offset + 8]
    original = data[:offset] + ORIGINAL_PREAMBLE[:8] + data[offset + 8:]
    metadata = validate_stock_image(original)
    if original[offset:offset + 12] != ORIGINAL_PREAMBLE:
        raise ValueError("Unrecognized stock ceiling function")
    if max_speed_raw is not None:
        validate_speed_raw(max_speed_raw)
        if code != encode_mov32(max_speed_raw):
            raise ValueError("Image does not contain the requested canonical eight-byte ceiling patch")
    elif code != ORIGINAL_PREAMBLE[:8]:
        ceiling = (code[0] << 8) | (0 if code[4:] == bytes.fromhex("704700bf") else code[4])
        validate_speed_raw(ceiling)
        if code != encode_mov32(ceiling):
            raise ValueError("Image contains an unrecognized ceiling patch")
    return metadata, signatures


def boot_clamp_site(data: bytes, sigs: dict) -> tuple[int, int]:
    """Resolve the shared clamp function entry and its literal RAM pointer."""
    entry = sigs["boot_clamp_bhs"] - 22
    if data[entry:entry + 2] != bytes.fromhex("80b5"):
        raise ValueError("Unrecognized boot clamp preamble")
    load = int.from_bytes(data[entry + 12:entry + 14], "little")
    if load & 0xff00 != 0x4900:  # ldr r1, [pc, #imm]
        raise ValueError("Unrecognized boot clamp cache load")
    literal = ((entry + 12 + 4) & ~3) + (load & 0xff) * 4
    cache = struct.unpack_from("<I", data, literal)[0]
    if not 0x20000000 <= cache <= 0x2000fffe:
        raise ValueError("Boot clamp cache outside RAM")
    return entry + 0x10000, cache


def verify_boot_clamp(motor, data: bytes, sigs: dict, ceiling: int) -> dict:
    entry, cache = boot_clamp_site(data, sigs)
    saved = bytes(motor.u.mem_read(cache, 2))
    try:
        for value in (MIN_SPEED_RAW, ceiling, 65535):
            motor.u.mem_write(cache, value.to_bytes(2, "little"))
            motor.call(entry)
            actual = int.from_bytes(motor.u.mem_read(cache, 2), "little")
            if actual != min(value, ceiling):
                raise RuntimeError(f"Boot clamp mismatch: {value} became {actual}")
    finally:
        motor.u.mem_write(cache, saved)
    return {"status": "verified", "entry": hex(entry)}


def verify_assist_curve(motor, speed: int) -> list[int]:
    """Check the EP8004.2 synthetic EU fixture's actual SET-generated table."""
    data = motor.u.mem_read(0x200024b0, 16)
    words = [int.from_bytes(data[i:i + 2], "little") for i in range(0, 16, 2)]
    expected = [speed + 125, speed - 300, speed - 200, speed]
    if words[4:] != expected:
        raise RuntimeError(f"Assist curve mismatch at {speed}: {words[4:]} != {expected}")
    return words


def verify_in_emulator(
    patched_bin: bytes,
    max_speed_raw: int,
    gateway_path: Path | None = None,
    patch_info: dict | None = None,
) -> dict:
    """Execute selected application paths; full boot and flashing are unmodeled.

    EP8004.2 checks real SET/GET, modeled EEPROM commits/reloads, the actual
    startup clamp, and expected assist thresholds. Other profiles check only
    the ceiling function, startup clamp, and GET handler, reported as partial.
    """
    validate_speed_raw(max_speed_raw)
    # Reject any other changed byte before importing or executing the emulator.
    metadata, sigs = validate_patched_image(patched_bin, max_speed_raw)
    if metadata["profile_id"] == "ep800-4.2.0" and gateway_path is None:
        raise ValueError("Supply --gateway with a profiled, decoded SC-EM800 gateway binary")
    tools_dir = Path(__file__).resolve().parent / "firmware-sim"
    if str(tools_dir) not in sys.path:
        sys.path.insert(0, str(tools_dir))

    from fw_sim.motor import Motor
    from fw_sim.gateway import Gateway
    from fw_sim.bus import Bus
    from unicorn.arm_const import UC_ARM_REG_R0
    import json

    flash_analysis = analyze_flash_layout(patched_bin)
    profiles = json.loads((tools_dir / "profiles.json").read_text())
    base_profile = find_matching_profile(patched_bin, profiles)
    patched_profile = dict(base_profile)
    patched_profile["sha256"] = hashlib.sha256(patched_bin).hexdigest()
    patched_profile["id"] = f"{base_profile['id']}-patched"
    patched_profile["base_profile"] = base_profile["id"]

    motor = Motor(patched_bin, patched_profile)
    # Discover from the image, rather than trusting caller-supplied offsets.
    motor.call(sigs["max_speed_fn"] + 0x10000)
    max_speed_val = motor.u.reg_read(UC_ARM_REG_R0)
    if max_speed_val != max_speed_raw:
        raise RuntimeError(f"get_max_speed_limit returned {max_speed_val}, expected {max_speed_raw}")
    boot_result = verify_boot_clamp(motor, patched_bin, sigs, max_speed_raw)
    limitations = ["bootloader flashing", "full boot", "physical EEPROM and hardware"]

    if base_profile["id"] != "ep800-4.2.0":
        motor.write_byte(motor.profile["mode_ram"], 1)
        motor.write_byte(motor.profile["owner_ram"], 0)
        read = motor.receive(bytes.fromhex("16b401"), 0)
        if not read or not read[0]["payload"].startswith("16b6"):
            raise RuntimeError(f"Reading speed failed: {read}")
        return {
            "status": "partial",
            "scope": "ceiling_function_boot_clamp_and_get_handler",
            "flash_analysis": flash_analysis,
            "max_speed_limit_returned": max_speed_val,
            "boot_clamp": boot_result,
            "get_speed_reply": read[0]["payload"],
            "unverified": limitations + ["SET authentication", "persistence", "assist curve"],
        }

    gw_data = read_bounded(Path(gateway_path), MAX_IMAGE_BYTES)
    gw_sha = hashlib.sha256(gw_data).hexdigest()
    gateways = [profile for profile in profiles
                if profile["sha256"] == gw_sha and profile["kind"] == "gateway"
                and profile["model"] == "SC-EM800"]
    if len(gateways) != 1:
        raise ValueError("Unsupported gateway image; a profiled, decoded SC-EM800 image is required")
    gw_profile = gateways[0]
    from test_persistence import authenticate

    clamp_entry, cache = boot_clamp_site(patched_bin, sigs)

    def connect(state_path):
        # Each reload uses fresh CPU/RAM, reloads the on-disk record, and executes
        # the real clamp function that the simulator's synthetic boot omits.
        gateway = Gateway(gw_data, gw_profile, slot=13)
        fresh = Motor(patched_bin, patched_profile)
        reloading = state_path.exists()
        fresh.install_storage(state_path)
        if reloading:
            fresh.call(clamp_entry)
        # The first connection starts from the simulator's synthetic 2500 cache.
        # Thus even a 1500 ceiling requires a real SET/commit before reload.
        bus = Bus(gateway, fresh)
        for packet in gateway.start():
            bus.enqueue(packet)
        bus.drain()
        if not fresh.connected:
            raise RuntimeError("Motor failed to connect in PC-link mode")
        return fresh

    def read_speed(machine):
        replies = machine.receive(bytes.fromhex("16b401"), 0)
        if not replies or not replies[0]["payload"].startswith("16b6"):
            raise RuntimeError(f"Reading speed failed: {replies}")
        return int.from_bytes(bytes.fromhex(replies[0]["payload"])[2:4], "little"), replies[0]["payload"]

    def set_speed(machine, speed):
        return machine.receive(b"\x16\xb0" + speed.to_bytes(2, "little") + b"\x01\xff", 0)

    test_speeds = sorted({MIN_SPEED_RAW, max_speed_raw} | {
        speed for speed in (2500, 3200, 4500, 5000, 6000) if speed <= max_speed_raw
    })
    speed_results = []
    with tempfile.TemporaryDirectory(prefix="cranklink-speed-verify-") as directory:
        state_path = Path(directory) / "eeprom.json"
        motor = connect(state_path)
        for speed in test_speeds:
            auth_replies = authenticate(motor)
            if not auth_replies or not auth_replies[0]["payload"].startswith("16e2"):
                raise RuntimeError("Motor authentication failed")
            before_commits = motor.storage.commits
            replies = set_speed(motor, speed)
            if not replies or not replies[0]["payload"].startswith("16b2"):
                raise RuntimeError(f"Setting speed {speed} failed: {replies}")
            value, get_reply = read_speed(motor)
            if value != speed:
                raise RuntimeError(f"Speed readback mismatch: expected {speed}, got {value}")
            curve_words = verify_assist_curve(motor, speed)
            if motor.storage.commits <= before_commits:
                raise RuntimeError(f"No EEPROM commit for speed {speed}")
            # Read from disk independently of the in-memory storage model.
            persisted = json.loads(state_path.read_text())
            record = bytes.fromhex(persisted["records"][str(0x29)])
            if int.from_bytes(record[:2], "little") != speed or sum(record) & 255:
                raise RuntimeError(f"Persisted speed mismatch at {speed}")

            # Above-ceiling writes must not be acknowledged or change RAM/EEPROM.
            for rejected_speed in {max_speed_raw + 1, 65535}:
                rejected = set_speed(motor, rejected_speed)
                if not rejected or not rejected[0]["payload"].startswith("16b3"):
                    raise RuntimeError(f"Above-ceiling speed {rejected_speed} accepted")
                if read_speed(motor)[0] != speed or json.loads(state_path.read_text()) != persisted:
                    raise RuntimeError("Rejected speed changed RAM or EEPROM")

            motor = connect(state_path)
            reloaded, reboot_reply = read_speed(motor)
            cached = int.from_bytes(motor.u.mem_read(cache, 2), "little")
            if reloaded != speed or cached != speed:
                raise RuntimeError(f"Speed did not survive reload/clamp: {speed} -> {reloaded}/{cached}")
            if motor.byte(motor.profile["auth_ram"]) != 0:
                raise RuntimeError("Authentication survived fresh-machine reload")
            speed_results.append({
                "speed_raw": speed,
                "speed_kmh": speed / 100.0,
                "set_reply": replies[0]["payload"],
                "get_reply": get_reply,
                "reboot_get_reply": reboot_reply,
                "persisted": True,
                "reloaded": True,
                "assist_curve_table": curve_words,
                "status": "accepted",
            })

    return {
        "status": "verified",
        "scope": "modeled_settings_persistence_and_assist",
        "flash_analysis": flash_analysis,
        "emulated_motor_connected": motor.connected,
        "authenticated": True,
        "tested_speed_limits": speed_results,
        "assist_curve_table": curve_words,
        "boot_clamp": boot_result,
        "unverified": limitations,
    }


def main():
    parser = argparse.ArgumentParser(
        description="Shimano STEPS Firmware Speed Limit Patcher and Emulator Verification Tool"
    )
    parser.add_argument("-i", "--input", required=True, type=Path, help="Input firmware file (.bin or .dat)")
    parser.add_argument("-o", "--output", required=True, type=Path, help="Output patched firmware file (.bin or .dat)")
    parser.add_argument(
        "-s", "--speed-limit",
        type=parse_speed_limit,
        default=DEFAULT_SPEED_RAW,
        help="Target ceiling in km/h, 15.00..99.00 with at most two decimal places (default: 60.00)",
    )
    parser.add_argument("--verify", action="store_true", help="Check selected application routines in the emulator; flashing is not emulated")
    parser.add_argument("--gateway", type=Path, help="Profiled, decoded SC-EM800 gateway binary; required for EP800 D4.2.0 --verify")
    parser.add_argument("--json", action="store_true", help="Output results in JSON format")

    args = parser.parse_args()

    raw_speed = args.speed_limit

    if not args.input.exists():
        sys.exit(f"Error: input file '{args.input}' does not exist")
    if args.input.resolve() == args.output.resolve() or (args.output.exists() and args.input.samefile(args.output)):
        raise ValueError("Input and output must be different files")

    raw_input = read_bounded(args.input)
    is_dat = is_container(raw_input)

    if is_dat:
        plain_input = decode_container(raw_input)
    else:
        plain_input = raw_input

    meta = validate_stock_image(plain_input)
    patched_plain, patch_info = patch_binary(plain_input, raw_speed)

    out_is_dat = args.output.suffix.lower() == ".dat"
    if out_is_dat:
        output_bytes = encode_container(patched_plain)
    else:
        output_bytes = patched_plain

    result = {
        "input_file": str(args.input),
        "output_file": str(args.output),
        "format": "encrypted_dat" if out_is_dat else "raw_bin",
        "firmware_metadata": meta,
        "patch_info": patch_info,
    }

    if args.verify:
        verify_res = verify_in_emulator(patched_plain, raw_speed, args.gateway, patch_info)
        result["verification"] = verify_res

    # A failed verification must not leave behind a newly generated artifact.
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_bytes(output_bytes)

    if args.json:
        import json
        print(json.dumps(result, indent=2))
    else:
        print(f"Successfully patched {meta['model']} v{meta['version']} (Series {meta['series']:#02x}, Board {meta['board']}):")
        print(f"  Input:          {args.input} ({len(raw_input)} bytes)")
        print(f"  Output:         {args.output} ({len(output_bytes)} bytes)")
        print(f"  Max Speed:      {raw_speed} raw ({patch_info['max_speed_kmh']:.2f} km/h)")
        print(f"  Patched SHA256: {patch_info['patched_sha256']}")
        print("  Patches applied:")
        print(f"    - get_max_speed_limit return:  {patch_info['patch1_max_speed_fn_offset']}")
        print(f"    - SET ceiling check retained:  {patch_info['patch2_check_speed_offset']}")
        print(f"    - boot ceiling clamp retained: {patch_info['patch3_boot_clamp_offset']}")

        if args.verify:
            print("\nEmulator Verification Results:")
            v = result["verification"]
            fs = v["flash_analysis"]
            print(f"  Offline Flash Layout:          {fs['total_blocks']} blocks, erase code {fs['erase_code']}; bootloader NOT EXECUTED")
            if "tested_speed_limits" in v:
                print(f"  Motor Execution:               CONNECTED & AUTHENTICATED")
                for r in v["tested_speed_limits"]:
                    print(f"    Speed {r['speed_kmh']:6.2f} km/h ({r['speed_raw']:5d}): SET -> {r['set_reply']}  GET -> {r['get_reply']} ({r['status'].upper()})")
                print(f"  Assist Ramp Thresholds:        {v['assist_curve_table'][4:]}")
                print("  EEPROM Persistence:            MODELED COMMIT, DISK RELOAD & STARTUP CLAMP CHECKED")
            else:
                print("  Motor Execution:               DIRECT FUNCTION/GET CHECKS ONLY")
                print(f"    get_max_speed_limit return:  {v['max_speed_limit_returned']}")
                print(f"    GET 16b4 readback:           {v['get_speed_reply']}")
            print(f"  Status:                        {v['status'].upper()} ({v['scope']})")
            print(f"  Unverified:                    {', '.join(v['unverified'])}")


if __name__ == "__main__":
    main()
