import struct
import unittest
import zlib

from prepare_photos import strip_metadata


class MetadataTests(unittest.TestCase):
    def test_jpeg_preserves_primary_scan_removes_descriptive_and_auxiliary_data(self):
        def segment(marker, payload):
            return bytes([255, marker]) + (len(payload) + 2).to_bytes(2, "big") + payload
        scan = b"\xff\xda\x00\x02coded\xff\x00pixels\xff\xd9"
        icc = segment(0xe2, b"ICC_PROFILE\0colour")
        source = (b"\xff\xd8" + segment(0xe1, b"Exif\0GPS") + icc +
                  segment(0xe2, b"MPF\0private") + segment(0xfe, b"comment") +
                  scan + b"\xff\xd8auxiliary EXIF location")
        self.assertEqual(strip_metadata(source), b"\xff\xd8" + icc + scan)

    def test_png_preserves_coded_pixels(self):
        def chunk(kind, payload):
            return (struct.pack(">I", len(payload)) + kind + payload +
                    struct.pack(">I", zlib.crc32(kind + payload)))
        signature = b"\x89PNG\r\n\x1a\n"
        image = chunk(b"IDAT", b"coded pixels")
        source = signature + chunk(b"iTXt", b"private XMP") + image + chunk(b"IEND", b"")
        self.assertEqual(strip_metadata(source), signature + image + chunk(b"IEND", b""))
