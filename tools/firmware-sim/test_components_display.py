"""Display settings executed in locally supplied, exact-hash stock firmware."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fw_sim.components_display import Display, DISPLAY_PROFILES
from fw_sim.profiles import identify


KEYS = ('32100100ffff', '3230a22bffff', '3230300effff',
        '32307a4dffff', '3230622bffff', '323085b4ffff')


def link(display):
    output = []
    for packet in KEYS:
        output.extend(display.receive(bytes.fromhex(packet), 0))
    if output != [{'target': 0, 'payload': '32120100ffff'}] or not display.connected:
        raise AssertionError('stock display PC-link handshake failed')


class DisplayAdmissionTests(unittest.TestCase):
    def test_unknown_profile_never_maps_or_executes(self):
        with patch('fw_sim.components_display.Machine.__init__') as constructor:
            with self.assertRaisesRegex(RuntimeError, 'unsupported display'):
                Display(b'', {'id': 'unknown'})
            constructor.assert_not_called()

    def test_recognized_name_with_wrong_hash_never_maps_or_executes(self):
        with patch('fw_sim.components_display.Machine.__init__') as constructor:
            with self.assertRaisesRegex(RuntimeError, 'profile hash mismatch'):
                Display(b'', {'id': 'sc-em800-4.6.0', 'sha256': '00' * 32})
            constructor.assert_not_called()


@unittest.skipUnless(os.environ.get('CRANKLINK_SIM_FIXTURES'),
                     'set CRANKLINK_SIM_FIXTURES for stock display instruction checks')
class StockDisplayTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = Path(os.environ['CRANKLINK_SIM_FIXTURES']).resolve()
        paths = json.loads(manifest.read_text())
        cls.fixtures = {}
        for name, path in paths.items():
            if name in DISPLAY_PROFILES:
                resolved = (manifest.parent / path).resolve()
                image, profile = identify(resolved, 'gateway')
                if profile['id'] != name:
                    raise ValueError('display fixture identity mismatch')
                cls.fixtures[name] = (image, profile)
        if not cls.fixtures:
            raise ValueError('fixture manifest contains no executable display component profile')

    def display(self, fixture, **kwargs):
        return Display(*self.fixtures[fixture], **kwargs)

    def reply(self, display, request):
        packets = display.receive(request, 0)
        self.assertEqual(len(packets), 1, request.hex())
        self.assertEqual(packets[0]['target'], 0)
        return bytes.fromhex(packets[0]['payload'])

    def test_stock_handshake_checks_keys_and_stop_releases_owner(self):
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture):
                display = self.display(fixture)
                self.assertFalse(display.connected)
                self.assertEqual(display.receive(bytes.fromhex('230400'), 0), [])
                for packet in KEYS[:-1]:
                    self.assertEqual(display.receive(bytes.fromhex(packet), 0), [])
                self.assertEqual(display.receive(bytes.fromhex('323085b5ffff'), 0), [])
                self.assertFalse(display.connected)
                # An independently started handshake must execute all five keys.
                link(display)
                stop = self.reply(display, bytes.fromhex('32100000ffff'))
                self.assertEqual(stop, bytes.fromhex('32120000ffff'))
                self.assertFalse(display.connected)
                self.assertEqual(display.receive(bytes.fromhex('230400'), 0), [])

    def test_stock_identity_version_serial_and_boot_readiness_guard(self):
        for fixture, (image, _) in self.fixtures.items():
            with self.subTest(fixture=fixture):
                display = self.display(fixture)
                link(display)
                self.assertEqual(self.reply(display, bytes.fromhex('011c00')),
                                 bytes([1, 0x1E, image[40], 2, 255, 255]))
                version = self.reply(display, bytes.fromhex('012c00'))
                self.assertEqual(version[:5], b'\x01\x2e' + image[16:19])
                serial = self.reply(display, bytes.fromhex('013c00'))
                self.assertEqual(serial, b'\x01\x3e' + bytes(6))
                display.write_byte(display.display['ready_ram'], 0)
                self.assertEqual(self.reply(display, bytes.fromhex('012c00'))[:4],
                                 bytes.fromhex('012f3900'))

    def test_all_schema_choices_stock_roundtrip_preserves_siblings_and_restores(self):
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture):
                display = self.display(fixture, allow_writes=True)
                link(display)
                for group, set_code, initial, values, width in (
                    (0x23, 0x00, 1, (0, 1), 1),
                    (5, 0x00, 1, (0, 2, 1), 1),
                    (5, 0x18, 0, (1, 0), 1),
                    (5, 0x20, 0, (1, 2, 3, 4, 5, 0), 1),
                    (5, 0x50, 60, (20, 40, 60, 80, 100, 60), 4),
                ):
                    if (group, set_code) not in display.allowed_commands:
                        continue
                    get = bytes([group, set_code + 4, 0])
                    self.assertEqual(self.reply(display, get), bytes([group, set_code + 6, initial]))
                    for value in values:
                        before = dict(display.records)
                        setter = bytes([group, set_code]) + value.to_bytes(width, 'little')
                        self.assertEqual(self.reply(display, setter), bytes([group, set_code + 2, value]))
                        self.assertEqual(self.reply(display, get), bytes([group, set_code + 6, value]))
                        record = display.records[display.display['settings_record']]
                        offset = {(0x23, 0): 2, (5, 0): 4, (5, 0x18): 0, (5, 0x20): 1, (5, 0x50): 5}[(group, set_code)]
                        self.assertEqual(record[offset], value)
                        old = before[display.display['settings_record']]
                        self.assertEqual(record[:offset] + record[offset + 1:7], old[:offset] + old[offset + 1:7])
                        self.assertEqual(sum(record) & 255, 0)
                self.assertGreaterEqual(display.commits, 8)
                self.assertGreater(display.instructions, 10000)
                self.reply(display, bytes.fromhex('32100000ffff'))
                self.assertFalse(display.connected)

    def test_modeled_disk_commit_rehydrates_new_stock_instance(self):
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture), tempfile.TemporaryDirectory() as folder:
                path = Path(folder) / 'display.json'
                display = self.display(fixture, allow_writes=True, storage_path=path)
                link(display)
                packets = ['230000', '051801', '052005']
                if (5, 0x50) in display.allowed_commands:
                    packets.append('055064000000')
                if (5, 0) in display.allowed_commands:
                    packets.append('050002')
                for packet in packets:
                    self.reply(display, bytes.fromhex(packet))
                self.assertTrue(path.exists())
                self.reply(display, bytes.fromhex('32100000ffff'))
                restarted = self.display(fixture, allow_writes=True, storage_path=path)
                link(restarted)
                cases = [('230400', '230600'), ('051c00', '051e01'), ('052400', '052605')]
                if (5, 0x50) in restarted.allowed_commands:
                    cases.append(('055400', '055664'))
                if (5, 0) in restarted.allowed_commands:
                    cases.append(('050400', '050602'))
                for request, expected in cases:
                    self.assertEqual(self.reply(restarted, bytes.fromhex(request)), bytes.fromhex(expected))
                self.assertEqual(restarted.commits, 0)
                self.assertEqual(restarted.records, display.records)

    def test_every_byte_value_is_normalized_by_stock_settings_code(self):
        # These exercise the firmware's permissive raw protocol. The CLI has a
        # narrower schema and rejects these unsupported values before SET.
        for fixture in self.fixtures:
            display = self.display(fixture, allow_writes=True)
            link(display)
            for group, set_code, width, normalize in (
                (0x23, 0, 1, lambda value: int(value != 0)),
                (5, 0, 1, lambda value: value if value in (0, 2) else 1),
                (5, 0x18, 1, lambda value: value if value < 2 else 0),
                (5, 0x20, 1, lambda value: value if value < 6 else 0),
                (5, 0x50, 4, lambda value: max(20, ((value + 19) // 20) * 20) if value <= 100 else 60),
            ):
                if (group, set_code) not in display.allowed_commands:
                    continue
                for value in range(256):
                    with self.subTest(fixture=fixture, group=group, set=set_code, value=value):
                        request = bytes([group, set_code]) + value.to_bytes(width, 'little')
                        expected = normalize(value)
                        self.assertEqual(self.reply(display, request), bytes([group, set_code + 2, expected]))
                        self.assertEqual(self.reply(display, bytes([group, set_code + 4, 0])),
                                         bytes([group, set_code + 6, expected]))
                        self.assertEqual(sum(display.records[display.display['settings_record']]) & 255, 0)

    def test_mixed_saves_identity_queries_reboots_and_sibling_restore(self):
        # Match the native client ordering: identity before a setting operation,
        # shared live machine within a batch, fresh CPU for the next invocation.
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture), tempfile.TemporaryDirectory() as folder:
                path = Path(folder) / 'display.json'

                def start():
                    display = self.display(fixture, allow_writes=True, storage_path=path)
                    link(display)
                    for query in ('011c00', '012c00', '013c00'):
                        self.reply(display, bytes.fromhex(query))
                    return display

                fields = [(0x23, 0, 1, 0, 1), (5, 0x18, 0, 1, 1), (5, 0x20, 0, 2, 1)]
                display = start()
                if (5, 0x50) in display.allowed_commands:
                    fields.append((5, 0x50, 60, 100, 4))
                if (5, 0) in display.allowed_commands:
                    fields.append((5, 0, 1, 2, 1))
                for group, code, initial, value, width in fields:
                    self.reply(display, bytes.fromhex('013c00'))
                    self.assertEqual(self.reply(display, bytes([group, code + 4, 0]))[-1], initial)
                    self.assertEqual(self.reply(display, bytes([group, code]) + value.to_bytes(width, 'little'))[-1], value)
                self.reply(display, bytes.fromhex('32100000ffff'))
                for group, code, _, value, _ in fields:
                    display = start()
                    self.assertEqual(self.reply(display, bytes([group, code + 4, 0]))[-1], value)
                    self.reply(display, bytes.fromhex('32100000ffff'))
                display = start()
                self.reply(display, bytes.fromhex('052003'))
                self.reply(display, bytes.fromhex('32100000ffff'))
                display = start()
                self.reply(display, bytes.fromhex('230001'))
                self.assertEqual(self.reply(display, bytes.fromhex('052400'))[-1], 3)
                for group, code, initial, _, width in fields:
                    self.reply(display, bytes.fromhex('013c00'))
                    self.reply(display, bytes([group, code]) + initial.to_bytes(width, 'little'))
                self.reply(display, bytes.fromhex('32100000ffff'))
                display = start()
                for group, code, initial, _, _ in fields:
                    self.assertEqual(self.reply(display, bytes([group, code + 4, 0]))[-1], initial)

    def test_failed_modeled_commit_cannot_emit_ack_or_change_disk_records(self):
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture), tempfile.TemporaryDirectory() as folder:
                path = Path(folder) / 'display.json'
                display = self.display(fixture, allow_writes=True, storage_path=path)
                link(display)
                self.reply(display, bytes.fromhex('052002'))
                saved, records, commits, output = path.read_bytes(), dict(display.records), display.commits, len(display.output)
                with patch.object(display, '_save', side_effect=OSError('synthetic disk failure')):
                    with self.assertRaisesRegex(OSError, 'synthetic disk failure'):
                        display.receive(bytes.fromhex('052004'), 0)
                self.assertEqual(path.read_bytes(), saved)
                self.assertEqual(display.records, records)
                self.assertEqual(display.commits, commits)
                self.assertEqual(len(display.output), output)
                restarted = self.display(fixture, storage_path=path)
                link(restarted)
                self.assertEqual(self.reply(restarted, bytes.fromhex('052400')), bytes.fromhex('052602'))

    def test_corrupt_staged_record_cannot_be_acknowledged_or_persisted(self):
        for fixture in self.fixtures:
            with self.subTest(fixture=fixture):
                display = self.display(fixture, allow_writes=True)
                record = display.display['settings_record']
                pointer, length = display.metadata[record]
                display.u.mem_write(pointer, bytes([1]) * length)
                display.write_byte(display.display['record_state'] + record * 10 + 6, 1)
                before = dict(display.records)
                with self.assertRaisesRegex(RuntimeError, 'checksum invalid'):
                    display.call(display.display['commit'])
                self.assertEqual(display.records, before)
                self.assertEqual(display.commits, 0)
                self.assertEqual(display.output, [])

    def test_read_only_and_wrong_width_commands_fail_before_instructions(self):
        for fixture in self.fixtures:
            display = self.display(fixture)
            for packet in ('230000', '051801', '052001', '055064000000', '010000', '101000'):
                before = display.instructions
                with self.subTest(fixture=fixture, packet=packet), self.assertRaises(RuntimeError):
                    display.receive(bytes.fromhex(packet), 0)
                self.assertEqual(display.instructions, before)
            display = self.display(fixture, allow_writes=True)
            for packet in ('055064', '05200100', '051c', '32100100'):
                before = display.instructions
                with self.subTest(fixture=fixture, packet=packet), self.assertRaises(RuntimeError):
                    display.receive(bytes.fromhex(packet), 0)
                self.assertEqual(display.instructions, before)


if __name__ == '__main__':
    unittest.main()
