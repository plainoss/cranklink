"""Modeled GATT protocol checks; optional image matrix uses CRANKLINK_SIM_FIXTURES."""
import json
import os
import socket
import threading
import time
from collections import deque
from pathlib import Path
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
from fw_sim.gatt import (GattEndpoint, AUTH_CONTROL, AUTH_NONCE, FEATURE,
                         PCE_COMMAND, PCE_RESPONSE, FREE_COMMAND, FREE_RESPONSE,
                         NONCE, USER_PROOF, APP_PROOF, read_request_line)
from fw_sim.profiles import PROFILES


def write(endpoint, characteristic, value):
    return endpoint.request(dict(op='write', id=characteristic, bytes=list(value)))


def authenticate(endpoint):
    write(endpoint, AUTH_CONTROL, USER_PROOF)
    write(endpoint, AUTH_CONTROL, APP_PROOF)
    write(endpoint, FEATURE, b'\xff\0')
    for characteristic in (PCE_RESPONSE,FREE_RESPONSE):
        endpoint.request(dict(op='subscribe',id=characteristic))


class RequestFrameTests(unittest.TestCase):
    def test_complete_line_clean_eof_and_socket_timeout_restored(self):
        for data, expected in ((b'{"op":"close"}\n',b'{"op":"close"}\n'),(b'',b'')):
            client, server = socket.socketpair()
            with client, server:
                server.settimeout(0.2)
                client.sendall(data)
                client.shutdown(socket.SHUT_WR)
                self.assertEqual(read_request_line(server),expected)
                self.assertEqual(server.gettimeout(),0.2)

    def test_partial_eof_and_oversized_line_are_rejected(self):
        for data, message in ((b'{"op":','disconnected'),(b'x'*4096,'exceeds limit')):
            client, server = socket.socketpair()
            with self.subTest(message=message),client,server:
                client.sendall(data)
                client.shutdown(socket.SHUT_WR)
                with self.assertRaisesRegex(RuntimeError,message):
                    read_request_line(server)

    def test_partial_frame_deadline_is_total_not_per_byte(self):
        client, server = socket.socketpair()
        with client,server:
            def dribble():
                try:
                    for _ in range(15):
                        client.sendall(b'x')
                        time.sleep(0.01)
                except OSError:
                    pass
            writer = threading.Thread(target=dribble)
            writer.start()
            start = time.monotonic()
            try:
                with self.assertRaises((RuntimeError,TimeoutError)):
                    read_request_line(server,timeout=0.04,idle_timeout=0.2)
                self.assertLess(time.monotonic()-start,0.12)
            finally:
                client.shutdown(socket.SHUT_RDWR)
                writer.join()

    def test_idle_client_has_a_separate_bound(self):
        client, server = socket.socketpair()
        with client,server,self.assertRaises((RuntimeError,TimeoutError)):
            read_request_line(server,timeout=0.2,idle_timeout=0.01)


class GattProtocolTests(unittest.TestCase):
    def setUp(self):
        gateway = SimpleNamespace(profile={'pce_dispatcher':0x10000},restore=Mock(),slot=13)
        with patch('fw_sim.gatt.setup', return_value=(gateway,SimpleNamespace())), \
                patch('fw_sim.gatt.Path.read_bytes', return_value=bytes(64)):
            self.endpoint = GattEndpoint('gateway','motor')

    def test_authentication_order_and_pin_rejection(self):
        e = self.endpoint
        self.assertEqual(e.request(dict(op='read',id=AUTH_NONCE))['bytes'],list(NONCE))
        for proof in (APP_PROOF,b'\x01'+b'\0'*16):
            with self.assertRaisesRegex(RuntimeError,'challenge rejected'):
                write(e,AUTH_CONTROL,proof)
        with self.assertRaisesRegex(RuntimeError,'authenticate'):
            write(e,PCE_COMMAND,b'\0\4')
        with self.assertRaisesRegex(RuntimeError,'feature write rejected'):
            write(e,FEATURE,b'\xff\0')
        authenticate(e)
        self.assertTrue(e.access)
        with self.assertRaisesRegex(RuntimeError,'challenge rejected'):
            write(e,AUTH_CONTROL,APP_PROOF)

    def test_read_only_boundaries_reject_without_firmware_calls(self):
        e = self.endpoint
        authenticate(e)
        e.master = True
        for characteristic,payload in ((FEATURE,b'\0\x80'),(PCE_COMMAND,b'\0\x10'),
                                       (PCE_COMMAND,b'\0\x13\x01\0\0'),
                                       (FREE_COMMAND,b'\x0d\x16\xd8\0'),
                                       (FREE_COMMAND,b'\x0d\x01\0\0')):
            with self.subTest(payload=payload.hex()),self.assertRaises(RuntimeError):
                write(e,characteristic,payload)

    def test_notifications_require_subscription_and_receive_is_bounded(self):
        e = self.endpoint
        e.notify(PCE_RESPONSE,b'\x24\x93')
        self.assertIsNone(e.request(dict(op='receive',timeout_ms=0))['notification'])
        e.request(dict(op='subscribe',id=PCE_RESPONSE))
        e.notify(PCE_RESPONSE,b'\x24\x93')
        self.assertEqual(e.request(dict(op='receive'))['notification'],
                         dict(characteristic=PCE_RESPONSE,value=[0x24,0x93]))
        for timeout in (-1,60001,'100'):
            with self.assertRaisesRegex(RuntimeError,'timeout'):
                e.request(dict(op='receive',timeout_ms=timeout))

    def test_malformed_writes_and_unmodeled_topology_fail_closed(self):
        e = self.endpoint
        authenticate(e)
        for value in ([True],[256],[-1],['x'],[0]*21):
            with self.assertRaisesRegex(RuntimeError,'write bytes'):
                e.request(dict(op='write',id=FREE_COMMAND,bytes=value))
        with self.assertRaisesRegex(RuntimeError,'bus-master-zero'):
            write(e,PCE_COMMAND,b'\0\x0d\0')
        with self.assertRaisesRegex(RuntimeError,'master'):
            write(e,FREE_COMMAND,b'\x0d\x16\xac\1')

    def test_radio_disconnect_never_sends_firmware_cleanup_commands(self):
        e = self.endpoint
        e.motor.connected = True
        e.close()
        self.assertTrue(e.motor.connected)
        self.assertIsNone(e.device.active_endpoint)
        self.assertEqual(e.device.lifecycle[-1]['cleanup_confirmed'], False)
        e.gateway.restore.assert_called_once()  # Initial device setup only.
        self.assertFalse(any(event['kind'] == 'coordinator-cleanup-after-client-disconnect'
                             for event in e.device.lifecycle))


class StockGattTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_SIM_FIXTURES')
        if not manifest: raise unittest.SkipTest('set CRANKLINK_SIM_FIXTURES for stock firmware GATT tests')
        path = Path(manifest)
        fixtures = {k:(path.parent/v).resolve() for k,v in json.loads(path.read_text()).items()}
        gateways = [p for p in PROFILES if p['id'] in fixtures and p['kind']=='gateway' and 'pce_dispatcher' in p]
        motors = [p for p in PROFILES if p['id'] in fixtures and p['kind']=='motor']
        if not gateways or not motors: raise unittest.SkipTest('fixtures require a PCE dispatcher gateway and motor')
        cls.pairs = [(fixtures[g['id']],fixtures[m['id']]) for g in gateways for m in motors]

    def test_stock_master_lifecycle_repeated_reads_and_cleanup(self):
        for gateway,motor in self.pairs:
            with self.subTest(gateway=gateway.name,motor=motor.name):
                endpoint = GattEndpoint(gateway,motor)
                try:
                    authenticate(endpoint)
                    for packet in (b'\0\3\0',b'\0\4',b'\0\x13\x01\x1c\0',
                                   b'\0\3\x20',b'\0\4',b'\0\x1d\0',b'\0\x1b',b'\0\x0c\1'):
                        write(endpoint,PCE_COMMAND,packet)
                    self.assertTrue(endpoint.motor.connected)
                    self.assertEqual(endpoint.motor.byte(endpoint.motor.profile['owner_ram']),0)
                    self.assertEqual(endpoint.motor.byte(endpoint.motor.profile['auth_ram']),0)
                    endpoint.queue.clear()
                    for _ in range(20):
                        write(endpoint,FREE_COMMAND,b'\x0d\x16\xac\1')
                        reply = endpoint.request(dict(op='receive'))['notification']
                        self.assertEqual(reply['characteristic'],FREE_RESPONSE)
                        self.assertEqual(reply['value'][:4],[13,0x16,0xae,1])
                    write(endpoint,PCE_COMMAND,b'\0\3\0')
                    write(endpoint,PCE_COMMAND,b'\0\4')
                    write(endpoint,PCE_COMMAND,b'\0\x0c\0')
                    self.assertFalse(endpoint.motor.connected)
                finally:
                    endpoint.close()
                self.assertTrue(endpoint.closed)

    def test_dropped_radio_retains_stock_pc_link_until_an_explicit_stop(self):
        for gateway, motor in self.pairs:
            with self.subTest(gateway=gateway.name, motor=motor.name):
                endpoint = GattEndpoint(gateway, motor)
                authenticate(endpoint)
                for packet in (b'\0\3\0', b'\0\4', b'\0\3\x20',
                               b'\0\x1d\0', b'\0\x1b', b'\0\x0c\1'):
                    write(endpoint, PCE_COMMAND, packet)
                firmware = endpoint.motor
                self.assertTrue(firmware.connected)
                endpoint.close()
                self.assertTrue(firmware.connected)
                self.assertTrue(endpoint.device.snapshot()['motor_connected'])
                replacement = endpoint.device.connect()
                try:
                    self.assertIs(replacement.motor, firmware)
                    self.assertTrue(firmware.connected)
                    authenticate(replacement)
                    for packet in (b'\0\3\0', b'\0\3\x20', b'\0\x0c\0'):
                        write(replacement, PCE_COMMAND, packet)
                    self.assertFalse(firmware.connected)
                finally:
                    replacement.close()

    def test_gateway_stop_ack_with_lost_bus_delivery_does_not_release_firmware(self):
        for gateway, motor in self.pairs:
            with self.subTest(gateway=gateway.name, motor=motor.name):
                endpoint = GattEndpoint(gateway, motor)
                authenticate(endpoint)
                for packet in (b'\0\3\x20', b'\0\x1d\0', b'\0\x1b', b'\0\x0c\1'):
                    write(endpoint, PCE_COMMAND, packet)
                self.assertTrue(endpoint.motor.connected)
                endpoint.queue.clear()
                # Drop only delivery at the modeled bus boundary. The stock
                # gateway still constructs its own acknowledgement normally.
                with patch.object(endpoint, '_pump'):
                    write(endpoint, PCE_COMMAND, b'\0\x0c\0')
                self.assertTrue(any(n['characteristic'] == PCE_RESPONSE
                                    and n['value'][:2] == [0x2c, 0]
                                    for n in endpoint.queue))
                self.assertTrue(endpoint.motor.connected)
                endpoint.close()
                self.assertTrue(endpoint.motor.connected)
