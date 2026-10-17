"""Retained powered-device and radio-policy checks using private stock fixtures."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from browser_peer import BrowserPeer
from fw_sim.gatt import (AUTH_CONTROL, FEATURE, PCE_COMMAND, PCE_RESPONSE,
                         FREE_COMMAND, FREE_RESPONSE, USER_PROOF, APP_PROOF)
from fw_sim.scenarios import setup


class RetainedDeviceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_SIM_FIXTURES')
        if not manifest: raise unittest.SkipTest('set CRANKLINK_SIM_FIXTURES for persistent stock-device tests')
        path = Path(manifest)
        fixtures = {key:(path.parent/value).resolve() for key,value in json.loads(path.read_text()).items()}
        if not {'sc-em800-4.6.0','ep800-4.2.0'} <= fixtures.keys():
            raise unittest.SkipTest('persistent tests require SC-EM8004.6.0 and EP8004.2.0')
        cls.gateway,cls.motor = fixtures['sc-em800-4.6.0'],fixtures['ep800-4.2.0']

    def call(self, op, **fields):
        return self.peer.request(dict(id=1,op=op,**fields))

    def write(self, characteristic, payload):
        return self.call('request',request=dict(op='write',id=characteristic,bytes=list(payload)))

    def connect(self):
        self.call('connect')
        for characteristic in (PCE_RESPONSE,FREE_RESPONSE):
            self.call('request',request=dict(op='subscribe',id=characteristic))
        for characteristic,payload in ((AUTH_CONTROL,USER_PROOF),(AUTH_CONTROL,APP_PROOF),(FEATURE,b'\xff\0')):
            self.write(characteristic,payload)
        for payload in ('000300','0004','0013011c00','000320','0004','001d00','001b','000c01'):
            self.write(PCE_COMMAND,bytes.fromhex(payload))

    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.peer = BrowserPeer(self.gateway,self.motor,Path(self.folder.name)/'state.json',True)

    def tearDown(self):
        self.peer.close()
        self.folder.cleanup()

    def release_motor(self):
        self.write(FREE_COMMAND,bytes.fromhex('0d321000000000'))

    def restart_motor(self):
        # Ask a separate stock gateway machine to generate the handshake. Keep
        # firmware-generated key material out of committed test fixtures.
        gateway, _ = setup(self.gateway, self.motor)
        self.write(FREE_COMMAND, bytes([13,0x32,0x10,1,0,0,0]))
        for packet in gateway.start():
            payload = bytes.fromhex(packet['payload'])
            if payload[:2] == b'\x32\x30':
                # FREE commands use zero padding, whereas the stock gateway's
                # broadcast key packets carry its transport padding.
                self.write(FREE_COMMAND, bytes([13])+payload[:4]+b'\0\0')

    def test_three_saves_reuse_firmware_and_survive_disconnect_and_power_cycle(self):
        self.connect()
        motor = self.peer.device.motor
        for index,value in enumerate((2,1,2)):
            if index: self.restart_motor()
            self.write(FREE_COMMAND,bytes([13,0x10,0x10,value]))
            read = self.write(FREE_COMMAND,bytes.fromhex('0d101400'))
            self.assertEqual(read['notifications'][0]['value'],[13,0x10,0x16,value])
            self.release_motor()
            self.assertIs(self.peer.device.motor,motor)
            self.assertFalse(motor.connected)
        snapshot = self.call('snapshot')['result']
        self.assertEqual(snapshot['connections'],1)
        self.assertEqual(snapshot['storage']['commits'],3)
        self.assertEqual(snapshot['lifecycle_counts']['pce-command:0x1d'],1)
        self.assertTrue(snapshot['radio_available'])
        self.call('disconnect')
        self.call('power_cycle')
        self.assertIsNot(self.peer.device.motor,motor)
        self.assertEqual(self.call('snapshot')['result']['storage']['lamp'],2)
        self.connect()
        read = self.write(FREE_COMMAND,bytes.fromhex('0d101400'))
        self.assertEqual(read['notifications'][0]['value'][3],2)

    def test_actual_gateway_stop_ends_opt_in_connection_mode_until_reenabled(self):
        self.connect()
        self.release_motor()
        for payload in ('000300','0004','000c00'):
            self.write(PCE_COMMAND,bytes.fromhex(payload))
        self.call('disconnect')
        state = self.call('snapshot')['result']
        self.assertFalse(state['radio_available'])
        self.assertFalse(state['motor_connected'])
        with self.assertRaisesRegex(RuntimeError,'Gateway connection mode ended'):
            self.call('connect')
        self.call('reenable')
        self.call('connect')
        self.assertEqual(self.peer.endpoint.auth_stage,0)
        self.assertFalse(self.peer.endpoint.access)


if __name__=='__main__': unittest.main()
