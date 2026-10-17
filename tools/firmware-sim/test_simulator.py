"""Hardware-free regression checks; optional stock images stay outside Git.

Run: python -m unittest discover -s tools/firmware-sim -p 'test_*.py'
Set CRANKLINK_SIM_FIXTURES to a JSON profile-id:image-path manifest to enable
the strict real-firmware tests. Relative image paths resolve beside the manifest.
"""
import hashlib
import json
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from fw_sim.bus import Bus
from fw_sim.gateway import Gateway, fragments
from fw_sim.machine import (Machine, INPUT, RETURN, UC_ARM_REG_R0, UC_ARM_REG_R1,
                            UC_ARM_REG_R4, UC_ARM_REG_SP, UC_ARM_REG_LR)
from fw_sim.motor import Motor
from fw_sim.profiles import PROFILES, identify
from fw_sim.scenarios import FAULTS, run


class VirtualGateway:
    slot = 13

    def __init__(self):
        self.received = []

    def receive(self, payload, mutation=None):
        self.received.append((payload, mutation))
        return {'slot': self.slot, 'payload': payload.hex()}


class VirtualMotor:
    def __init__(self, reply=False):
        self.received = []
        self.reply = reply

    def receive(self, payload, source):
        self.received.append((payload, source))
        return [{'target': source, 'payload': '16ae01'}] if self.reply else []


def packet(payload='16ac01'):
    return {'source': 0, 'target': 13, 'payload': payload}


class BusTests(unittest.TestCase):
    def test_logical_deadlines_and_stable_ties(self):
        motor = VirtualMotor()
        bus = Bus(VirtualGateway(), motor)
        for payload, delay in [('0101', 5), ('0202', 1), ('0303', 1)]:
            bus.enqueue(packet(payload), delay=delay)
        bus.drain()
        self.assertEqual([p.hex() for p, _ in motor.received], ['0202', '0303', '0101'])
        self.assertEqual([(e['time'], e['sequence']) for e in bus.trace], [(1, 2), (1, 3), (5, 1)])
        bus.enqueue(packet(), delay=2)
        bus.drain()
        self.assertEqual(bus.time, 7)

    def test_drop_and_duplicate_are_delivery_events(self):
        motor = VirtualMotor()
        bus = Bus(VirtualGateway(), motor)
        bus.enqueue(packet('0101'), fault='drop')
        bus.enqueue(packet('0202'))
        bus.enqueue(packet('0202'))
        bus.drain()
        self.assertEqual(motor.received, [(b'\x02\x02', 0)] * 2)
        self.assertEqual(len(bus.trace), 3)
        self.assertEqual(bus.trace[0]['fault'], 'drop')

    def test_reply_loss_does_not_reach_gateway(self):
        gateway, motor = VirtualGateway(), VirtualMotor(reply=True)
        bus = Bus(gateway, motor)
        bus.drop_reply = True
        bus.enqueue(packet())
        self.assertEqual(bus.drain(), [])
        self.assertEqual(gateway.received, [])
        self.assertEqual(bus.trace[-1]['direction'], 'motor->gateway')
        self.assertEqual(bus.trace[-1]['fault'], 'drop')

    def test_successful_reply_has_later_tick_and_correct_routing(self):
        gateway, motor = VirtualGateway(), VirtualMotor(reply=True)
        bus = Bus(gateway, motor)
        bus.enqueue(packet())
        self.assertEqual(bus.drain(), [{'slot': 13, 'payload': '16ae01'}])
        self.assertEqual([e['time'] for e in bus.trace], [1, 2])
        self.assertEqual(gateway.received, [(bytes.fromhex('16ae01'), None)])

    def test_enqueued_packet_is_copied(self):
        motor = VirtualMotor()
        bus = Bus(VirtualGateway(), motor)
        original = packet()
        bus.enqueue(original)
        original['payload'] = 'ffff'
        bus.drain()
        self.assertEqual(motor.received[0][0], bytes.fromhex('16ac01'))

    def test_negative_delay_and_unroutable_destination_fail(self):
        bus = Bus(VirtualGateway(), VirtualMotor())
        with self.assertRaisesRegex(RuntimeError, 'negative event delay'):
            bus.enqueue(packet(), delay=-1)
        bus.enqueue(dict(packet(), target=12))
        with self.assertRaisesRegex(RuntimeError, 'unroutable'):
            bus.drain()

    def test_unknown_direction_and_fault_do_not_silently_deliver(self):
        bus = Bus(VirtualGateway(), VirtualMotor())
        for options in ({'direction': 'typo'}, {'fault': 'typo'}):
            with self.subTest(options=options), self.assertRaises(RuntimeError):
                bus.enqueue(packet(), **options)
        self.assertEqual(bus.events, [])

    def test_self_replenishing_bus_is_bounded(self):
        bus = Bus(VirtualGateway(), VirtualMotor())
        def replenish(payload, source):
            bus.enqueue(packet())
            return []
        bus.motor.receive = replenish
        bus.enqueue(packet())
        with self.assertRaisesRegex(RuntimeError, 'event budget exhausted'):
            bus.drain()
        self.assertEqual(len(bus.trace), 1000)


class MachineTests(unittest.TestCase):
    def image(self, code=b'\x70\x47'):
        image = code.ljust(64, b'\x00')
        return image, {'id': 'synthetic-test', 'sha256': hashlib.sha256(image).hexdigest()}

    def test_hash_mismatch_fails_before_mapping_or_execution(self):
        image, profile = self.image()
        with patch('fw_sim.machine.Uc') as emulator:
            with self.assertRaisesRegex(RuntimeError, 'hash mismatch'):
                Machine(image + b'\x00', profile)
            emulator.assert_not_called()

    def test_correct_hash_cannot_allow_invalid_mapping_size(self):
        for size in (0, 63, 0x3F001):
            with self.subTest(size=size):
                image = b'\x00' * size
                profile = {'id': 'oversize-test', 'sha256': hashlib.sha256(image).hexdigest()}
                with patch('fw_sim.machine.Uc') as emulator:
                    with self.assertRaisesRegex(RuntimeError, 'invalid mapped image size'):
                        Machine(image, profile)
                    emulator.assert_not_called()

    def test_unknown_image_is_not_identified_by_filename(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'SCEM800.4.0.0.dat'
            path.write_bytes(b'untrusted image')
            with self.assertRaisesRegex(RuntimeError, 'unsupported gateway SHA-256'):
                identify(path, 'gateway')

    def test_instruction_budget_stops_infinite_thumb_loop(self):
        machine = Machine(*self.image(b'\xfe\xe7'), budget=23)
        with self.assertRaisesRegex(RuntimeError, 'instruction budget exhausted'):
            machine.call(0x10000)
        self.assertEqual(machine.instructions, 23)

    def test_normal_thumb_return_succeeds(self):
        machine = Machine(*self.image())
        machine.call(0x10000)
        self.assertEqual(machine.instructions, 1)

    def test_transport_length_is_bounded(self):
        machine = Machine(*self.image())
        machine.u.reg_write(UC_ARM_REG_R1, INPUT)
        for length in (0, 1, 13, 255):
            with self.subTest(length=length):
                machine.u.mem_write(INPUT, bytes([length]) + b'\x00' * 16)
                with self.assertRaisesRegex(RuntimeError, 'packet length'):
                    machine.packet()
        machine.u.reg_write(UC_ARM_REG_R0, 13)
        machine.u.mem_write(INPUT, b'\x02\xff\x16\xae')
        self.assertEqual(machine.packet(), {'target': 13, 'payload': '16ae'})


class PeripheralServiceTests(unittest.TestCase):
    def machine(self, service=None, budget=32):
        # Main: movs r0,#42; poll: adds r0,#1; bx lr.
        # Service: clear busy via literal RAM pointer; clobber r4; bx lr.
        image = bytearray(64)
        image[:6] = bytes.fromhex('2a2001307047')
        image[16:26] = (service or bytes.fromhex('00200349087063247047')).ljust(10, b'\x00')
        image[32:36] = (0x20000010).to_bytes(4, 'little')
        image = bytes(image)
        profile = dict(id='synthetic-peripheral', sha256=hashlib.sha256(image).hexdigest(),
                       peripheral_init=0x10000, peripheral_poll=0x10002,
                       peripheral_busy_ram=0x20000010, peripheral_service=0x10010,
                       peripheral_channel=3)
        machine = Machine(image, profile, budget=budget)
        machine.write_byte(profile['peripheral_busy_ram'], 1)
        return machine

    def test_service_changes_ram_and_restores_interrupted_cpu_context(self):
        machine = self.machine()
        machine.call(0x10000, registers=((UC_ARM_REG_R4, 77),))
        self.assertEqual(machine.byte(0x20000010), 0)
        self.assertEqual(machine.peripheral_services, 1)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R0), 43)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R4), 77)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_SP), 0x2000F000)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_LR), RETURN | 1)
        self.assertIsNone(machine._call_deadline)
        self.assertFalse(machine._servicing_peripheral)

    def test_service_and_main_share_one_instruction_budget(self):
        machine = self.machine(service=bytes.fromhex('fee7'), budget=9)
        with self.assertRaisesRegex(RuntimeError, 'instruction budget exhausted'):
            machine.call(0x10000)
        self.assertEqual(machine.instructions, 9)
        self.assertEqual(machine.peripheral_services, 0)
        self.assertIsNone(machine._call_deadline)
        self.assertFalse(machine._servicing_peripheral)
        # Exception cleanup restores the interrupted context and allows a new
        # independently bounded call, rather than retaining a spent deadline.
        machine.write_byte(0x20000010, 0)
        machine.call(0x10000)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R0), 43)
        self.assertEqual(machine.instructions, 12)


class ProfileInventoryTests(unittest.TestCase):
    def test_ids_hashes_and_executable_roles_are_unambiguous(self):
        self.assertTrue(PROFILES)
        self.assertEqual(len({p['id'] for p in PROFILES}), len(PROFILES))
        self.assertEqual(len({p['sha256'] for p in PROFILES}), len(PROFILES))
        for profile in PROFILES:
            with self.subTest(profile=profile['id']):
                self.assertIn(profile['kind'], ('gateway', 'motor'))
                self.assertEqual(len(bytes.fromhex(profile['sha256'])), 32)
                self.assertTrue(profile['model'])


class FramingAndAdmissionTests(unittest.TestCase):
    def test_fragment_headers_sequence_and_padding(self):
        self.assertEqual(fragments(bytes.fromhex('16ae')), [bytes.fromhex('c116aeff')])
        self.assertEqual(fragments(bytes.fromhex('16ae0102030405')),
                         [bytes.fromhex('8116ae01'), bytes.fromhex('02020304'), bytes.fromhex('4305ffff')])
        for size in (0, 1, 10, 1000):
            with self.subTest(size=size), self.assertRaisesRegex(RuntimeError, '2..9'):
                fragments(b'\x00' * size)

    def test_fifo_rejects_excess_or_malformed_frames_before_cpu(self):
        gateway = Gateway.__new__(Gateway)
        gateway.slot, gateway.profile = 13, {}
        gateway.call = Mock()
        for frames in ([b'\x00' * 4] * 13, [b'\x00' * 3], [b'\x00' * 5]):
            with self.subTest(frames=len(frames)), self.assertRaisesRegex(RuntimeError, 'invalid FIFO'):
                gateway.receive_fragments(frames)
        gateway.call.assert_not_called()

    def test_write_reset_and_auth_commands_do_not_execute(self):
        motor = Motor.__new__(Motor)
        motor.u, motor.call, motor.output, motor.profile = Mock(), Mock(), [], {}
        for payload in ('16ad01', '16ab00', '16a800', '101000', '321101'):
            with self.subTest(payload=payload), self.assertRaisesRegex(RuntimeError, 'only normal read'):
                motor.receive(bytes.fromhex(payload), 0)
        motor.call.assert_not_called()
        for size in (0, 1, 10):
            with self.subTest(size=size), self.assertRaisesRegex(RuntimeError, '2..9'):
                motor.receive(b'\x00' * size, 0)


class NativeWireTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        from fw_sim import wire
        cls.wire = wire

    def test_matches_rust_protocol_golden_frame(self):
        encoded = self.wire.encode(0x48, bytes.fromhex('0d167c00'))
        self.assertEqual(encoded[:8], bytes.fromhex('bb480d167c0019bb'))
        self.assertEqual(len(encoded), 128)
        self.assertEqual(encoded[8:], b'\xff' * 120)
        self.assertEqual(self.wire.Decoder().feed(encoded), [(0x48, bytes.fromhex('0d167c00'))])

    def test_every_split_preserves_escaped_bytes_and_partial_frames(self):
        payload = bytes(range(256))
        encoded = self.wire.encode(0x48, payload)
        for split in range(len(encoded) + 1):
            with self.subTest(split=split):
                decoder = self.wire.Decoder()
                self.assertEqual(decoder.feed(encoded[:split]) + decoder.feed(encoded[split:]),
                                 [(0x48, payload)])
        decoder = self.wire.Decoder()
        frames = []
        for byte in encoded:
            frames.extend(decoder.feed(bytes([byte])))
        self.assertEqual(frames, [(0x48, payload)])

    def test_coalesced_frames_preserve_controls_and_payloads(self):
        frames = [(0x48, b'\x0d\x16\xae\xff'), (0x24, b''), (0xa8, b'\xbb\xbd')]
        data = b''.join(self.wire.encode(control, payload) for control, payload in frames)
        self.assertEqual(self.wire.Decoder().feed(data), frames)

    def test_checksum_and_malformed_escapes_raise_without_dispatch(self):
        corrupt = bytearray(self.wire.encode(3, b'\x04'))
        corrupt[2] = 5
        for data in (bytes(corrupt), bytes.fromhex('bb48bdbb'), bytes.fromhex('bb48bd00bb')):
            with self.subTest(data=data.hex()), self.assertRaises(ValueError):
                self.wire.Decoder().feed(data)

    def test_unterminated_input_is_bounded(self):
        with self.assertRaises(ValueError):
            self.wire.Decoder().feed(b'\xbb' + b'\x01' * 65537)


class EndpointAdmissionTests(unittest.TestCase):
    def endpoint(self):
        from fw_sim.endpoint import Endpoint
        endpoint = Endpoint.__new__(Endpoint)
        endpoint.gateway = SimpleNamespace(slot=13, profile={})
        endpoint.unit = Mock(return_value=[{'slot': 13, 'payload': '16ae0102030405'}])
        endpoint.drop_replies = False
        return endpoint

    def test_write_reset_auth_and_wrong_target_fail_before_unit(self):
        endpoint = self.endpoint()
        for payload in ('0d16ad01', '0d16a800', '0d101000', '0d32100200', '0d32100101', '0c16ac01'):
            with self.subTest(payload=payload), self.assertRaises(RuntimeError):
                endpoint.request(0x48, bytes.fromhex(payload))
        endpoint.unit.assert_not_called()

    def test_normal_host_reply_has_native_master_route(self):
        endpoint = self.endpoint()
        self.assertEqual(endpoint.request(0x48, bytes.fromhex('0d16ac01')),
                         [(0x48, bytes.fromhex('0016ae0102030405'))])
        endpoint.unit.assert_called_once_with(bytes.fromhex('16ac01'))


@unittest.skipUnless(os.environ.get('CRANKLINK_SIM_FIXTURES'),
                     'set CRANKLINK_SIM_FIXTURES for licensed/local stock image checks')
class StockFirmwareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = Path(os.environ['CRANKLINK_SIM_FIXTURES']).resolve()
        entries = json.loads(manifest.read_text())
        if not isinstance(entries, dict) or not entries:
            raise ValueError('fixture manifest must be a nonempty profile-id:path object')
        known = {p['id']: p for p in PROFILES}
        cls.fixtures = {}
        for name, path in entries.items():
            if name not in known:
                raise ValueError(f'unknown fixture profile {name}')
            resolved = (manifest.parent / path).resolve()
            _, actual = identify(resolved, known[name]['kind'])
            if actual['id'] != name:
                raise ValueError(f'fixture {name} identifies as {actual["id"]}')
            cls.fixtures[name] = resolved
        gateways = sorted(name for name in entries if known[name]['kind'] == 'gateway')
        motors = sorted(name for name in entries if known[name]['kind'] == 'motor')
        if not gateways or not motors:
            raise ValueError('fixtures must include at least one gateway and one motor')
        cls.gateway = cls.fixtures['sc-em800-4.0.0' if 'sc-em800-4.0.0' in gateways else gateways[0]]
        cls.motor = cls.fixtures['ep800-4.1.10' if 'ep800-4.1.10' in motors else motors[0]]

    def test_native_endpoint_consumes_more_than_one_host_ring_of_replies(self):
        from fw_sim.endpoint import Endpoint
        endpoint = Endpoint(self.gateway, self.motor)
        try:
            for index in range(40):
                with self.subTest(read=index):
                    before = len(endpoint.gateway.responses)
                    replies = endpoint.request(0x48, bytes.fromhex('0d16ac01'))
                    self.assertEqual(len(replies), 1)
                    self.assertEqual(replies[0][0], 0x48)
                    self.assertEqual(replies[0][1][:3], bytes.fromhex('0016ae'))
                    self.assertEqual(len(endpoint.gateway.responses), before + 1)
        finally:
            endpoint.close()
        self.assertFalse(endpoint.motor.connected)

    def test_stock_pair_all_faults_are_repeatable_and_cleanup(self):
        for fault in FAULTS:
            with self.subTest(fault=fault):
                first = run(self.gateway, self.motor, fault)
                self.assertEqual(first, run(self.gateway, self.motor, fault))
                self.assertTrue(first['cleanup'])
                self.assertEqual(first['connected'], fault not in ('drop-key', 'reorder-key'))
                self.assertGreater(first['instructions']['gateway'], 0)
                self.assertGreater(first['instructions']['motor'], 0)
                for read in first['reads']:
                    lost = fault in ('drop-reply', 'truncate', 'corrupt-sequence') or (
                        fault == 'reorder' and read['request'] != '101400')
                    self.assertEqual(len(read['replies']), 0 if lost else 1)
                    if not lost:
                        request = bytes.fromhex(read['request'])
                        response = bytes.fromhex(read['replies'][0]['payload'])
                        self.assertEqual(response[:2], bytes([request[0], request[1] + 2]))


if __name__ == '__main__':
    unittest.main()
