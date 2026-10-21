"""Trace analysis must associate overlapping uploads with the correct receipts."""
import json
from pathlib import Path
import tempfile
import unittest

from analyze_d_transfer import analyze


class DTraceAnalysisTests(unittest.TestCase):
    def test_prefetched_upload_does_not_steal_current_receipts_even_after_wrap(self):
        records = []

        def log(direction, value):
            records.append(dict(time_ms=len(records), direction=direction,
                                characteristic='00002afa' if direction == 'tx' else '00002af9',
                                bytes=value.hex()))

        def upload(index):
            packet = (2 * (index - 1)) % 256
            data = bytes([index % 251]) * 64
            payload = bytes([packet, 2, 0]) + data + bytes([sum(data) % 256]) + index.to_bytes(2, 'little')
            for offset, header in zip(range(0, 70, 18), (0x80, 0xa0, 0xc0, 0x60)):
                log('tx', bytes([header, 0x0b]) + payload[offset:offset + 18])
            log('rx', bytes([0x2b, 0, 0x82, packet, 0]))

        upload(1)
        for index in range(1, 132):
            packet = (2 * (index - 1)) % 256
            log('tx', bytes([0, 0x0b, (packet + 1) % 256, 3, packet]))
            if index < 131:
                upload(index + 1)
            log('rx', bytes([0x2b, 0, 0x83, (packet + 1) % 256, 0]))
            log('rx', bytes([0x2b, 0, 0xc0, packet]))
            log('rx', bytes([0x88, 0x31]) + index.to_bytes(2, 'little'))

        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'wire.jsonl'
            path.write_text('\n'.join(json.dumps(record) for record in records))
            result = analyze(path)
        self.assertEqual(result['complete_indexed_blocks'], 131)
        self.assertEqual(result['observed_blocks'], 131)
        self.assertGreater(result['phases']['transfer_dispatch_to_all_receipts']['mean_ms'], 0)


if __name__ == '__main__':
    unittest.main()
