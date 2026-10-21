"""Recognize one bounded patch to the exact EP800 D4.2.0 stock image.

Profile names and caller-provided hashes do not authorize persistent writes.
Restoring the original eight bytes must reproduce the complete stock hash.
"""
import hashlib

STOCK_SHA256 = 'f2f2d18441b7651620a0aacfd4f9f31b2a97b6397bf0f03b2171ff9652cf2551'
PATCH_OFFSET = 0x6B8E
ORIGINAL_CODE = bytes.fromhex('00b501000800c0b2')
MIN_CEILING, MAX_CEILING = 1500, 9900


def speed_patch_ceiling(image):
    """Return the exact supported patch's ceiling, or None for other images."""
    code = image[PATCH_OFFSET:PATCH_OFFSET + 8]
    if len(code) != 8 or code[1:4] != bytes.fromhex('200002'):
        return None
    if code[5:] == bytes.fromhex('307047'):
        ceiling = (code[0] << 8) | code[4]
        # The patcher's canonical zero-low-byte form uses BX/NOP instead.
        if code[4] == 0:
            return None
    elif code[4:] == bytes.fromhex('704700bf'):
        ceiling = code[0] << 8
    else:
        return None
    if not MIN_CEILING <= ceiling <= MAX_CEILING:
        return None
    restored = image[:PATCH_OFFSET] + ORIGINAL_CODE + image[PATCH_OFFSET + 8:]
    if hashlib.sha256(restored).hexdigest() != STOCK_SHA256:
        return None
    return ceiling
