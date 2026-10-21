#!/usr/bin/env python3
"""Remove JPEG/PNG descriptive metadata without recompressing image pixels.

Usage: python3 prepare_photos.py SOURCE DESTINATION
Preserves JPEG coded scan bytes and PNG IDAT chunks exactly. The publication
manifest records original and publication-copy hashes separately.
"""
from pathlib import Path
import struct
import sys


def strip_metadata(data):
    if data.startswith(b"\xff\xd8"):
        result = bytearray(data[:2])
        offset = 2
        while offset < len(data):
            start = offset
            if data[offset] != 0xff:
                raise ValueError("unexpected JPEG marker")
            while data[offset] == 0xff:
                offset += 1
            marker = data[offset]
            offset += 1
            if marker == 0xda:  # SOS: retain primary coded data, no re-encoding.
                end = data.find(b"\xff\xd9", offset)
                if end < 0:
                    raise ValueError("missing primary JPEG end")
                return bytes(result) + data[start:end + 2]
            if marker in (0xd8, 0xd9) or 0xd0 <= marker <= 0xd7:
                result.extend(data[start:offset])
                continue
            length = int.from_bytes(data[offset:offset + 2], "big")
            if length < 2 or offset + length > len(data):
                raise ValueError("invalid JPEG segment")
            offset += length
            # Preserve only application markers needed for display: JFIF,
            # Adobe colour transform, and ICC. Remove EXIF/XMP/IPTC/MPF and
            # unknown application segments, plus trailing auxiliary images.
            is_application = 0xe0 <= marker <= 0xef
            keep_application = marker in (0xe0, 0xee) or (
                marker == 0xe2 and data[start + 4:offset].startswith(b"ICC_PROFILE\0")
            )
            if marker != 0xfe and (not is_application or keep_application):
                result.extend(data[start:offset])
        raise ValueError("JPEG has no scan")
    if data.startswith(b"\x89PNG\r\n\x1a\n"):
        result = bytearray(data[:8])
        offset = 8
        while offset < len(data):
            length = struct.unpack_from(">I", data, offset)[0]
            end = offset + length + 12
            if end > len(data):
                raise ValueError("truncated PNG chunk")
            kind = data[offset + 4:offset + 8]
            if kind not in (b"eXIf", b"tEXt", b"zTXt", b"iTXt", b"tIME"):
                result.extend(data[offset:end])
            offset = end
        return bytes(result)
    raise ValueError("expected JPEG or PNG")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepare_photos.py SOURCE DESTINATION")
    source, destination = map(Path, sys.argv[1:])
    if source.resolve() == destination.resolve():
        raise SystemExit("use a separate publication copy")
    with destination.open("xb") as output:
        output.write(strip_metadata(source.read_bytes()))
