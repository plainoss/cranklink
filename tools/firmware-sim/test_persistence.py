"""Real stock setting/auth/storage regression tests using private local fixtures."""
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from fw_sim.motor import Motor
from fw_sim.profiles import identify
from fw_sim.scenarios import setup
from fw_sim.bus import Bus


def keys_for_serial(serial):
    """Independent host algorithm already used by src/ble/unit_auth.rs."""
    a,b,c = [int.from_bytes(serial[i:i+2],'little') for i in (0,2,4)]
    state = [123456789^((a<<16)|b),362436069,521288629,88675123^((b<<16)|c)]
    def step():
        x,y,z,w = state
        t = (x^(x<<11))&0xffffffff
        state[:] = [y,z,w,(w^(w>>19)^t^(t>>8))&0xffffffff]
        return state[3].to_bytes(4,'little')
    for _ in range(a>>8): step()
    auth = (step()+step())[:7]
    return auth,b''.join(step() for _ in range(4))


def authenticate(motor, bad_proof=False):
    from Crypto.Cipher import AES
    serial = bytes(motor.u.mem_read(0x20002934,6))
    auth,key = keys_for_serial(serial)
    replies = motor.receive(b'\x16\xd8'+auth,0)
    bodies = [bytes.fromhex(r['payload']) for r in replies]
    if len(bodies) != 3 or [b[:3] for b in bodies] != [b'\x16\xda\x16',b'\x16\xda\x26',b'\x16\xda\x34']:
        raise AssertionError('stock authentication did not emit three challenge fragments')
    nonce = bodies[0][3:9]+bodies[1][3:9]+bodies[2][3:7]
    proof = AES.new(key,AES.MODE_ECB).encrypt(nonce)
    if bad_proof: proof = bytes([proof[0]^1])+proof[1:]
    result = []
    for selector,offset,length in ((0x16,0,6),(0x26,6,6),(0x34,12,4)):
        result += motor.receive(bytes([0x16,0xe0,selector])+proof[offset:offset+length]+b'\xff'*(6-length),0)
    return result


class PersistentFirmwareTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest = os.environ.get('CRANKLINK_SIM_FIXTURES')
        if not manifest: raise unittest.SkipTest('set CRANKLINK_SIM_FIXTURES for stock persistent-write tests')
        p = Path(manifest)
        cls.fixtures = {k:(p.parent/v).resolve() for k,v in json.loads(p.read_text()).items()}
        if not {'sc-em800-4.6.0','ep800-4.2.0'} <= cls.fixtures.keys():
            raise unittest.SkipTest('persistent tests require SC-EM8004.6.0 and EP8004.2.0')

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.state = Path(self.directory.name)/'state.json'
        self.gateway,self.motor = self.new_motor()

    def new_motor(self):
        g,m = setup(self.fixtures['sc-em800-4.6.0'],self.fixtures['ep800-4.2.0'])
        m.install_storage(self.state)
        bus = Bus(g,m)
        for packet in g.start(): bus.enqueue(packet)
        bus.drain()
        self.assertTrue(m.connected)
        return g,m

    def test_lamp_actual_setter_commit_and_reboot_readback(self):
        m = self.motor
        self.assertEqual(m.receive(bytes.fromhex('101400'),0)[0]['payload'],'101600')
        self.assertEqual(m.receive(bytes.fromhex('101002'),0)[0]['payload'],'101202')
        self.assertEqual(m.receive(bytes.fromhex('101400'),0)[0]['payload'],'101602')
        self.assertEqual(m.storage.commits,1)
        _,rebooted = self.new_motor()
        self.assertEqual(rebooted.receive(bytes.fromhex('101400'),0)[0]['payload'],'101602')
        self.assertEqual(rebooted.byte(rebooted.profile['auth_ram']),0)
        self.assertNotIn('auth_key',self.state.read_text())

    def test_tire_rejects_unauthorized_then_real_crypto_proof_allows_persistent_save(self):
        m = self.motor
        self.assertEqual(m.receive(bytes.fromhex('350400'),0)[0]['payload'],'35066508ffff')
        rejected = m.receive(bytes.fromhex('350067080000'),0)
        self.assertTrue(rejected[0]['payload'].startswith('3503'))
        self.assertEqual(m.storage.snapshot()['tire_circumference'],2149)
        self.assertEqual(authenticate(m)[0]['payload'],'16e2ffffffff')
        self.assertEqual(m.byte(m.profile['auth_ram']),2)
        for value in (2151,2150,2151):
            request = b'\x35\0'+value.to_bytes(2,'little')+b'\0\0'
            self.assertEqual(m.receive(request,0)[0]['payload'][:4],'3502')
            self.assertEqual(m.storage.snapshot()['tire_circumference'],value)
        _,rebooted = self.new_motor()
        self.assertEqual(rebooted.storage.snapshot()['tire_circumference'],2151)
        self.assertEqual(rebooted.byte(rebooted.profile['auth_ram']),0)

    def test_redundant_pc_handshake_really_revokes_auth_and_rejects_next_save(self):
        m = self.motor
        self.assertEqual(authenticate(m)[0]['payload'],'16e2ffffffff')
        self.assertTrue(m.receive(bytes.fromhex('350066080000'),0)[0]['payload'].startswith('3502'))
        self.assertEqual(m.storage.snapshot()['tire_circumference'],2150)
        bus = Bus(self.gateway,m)
        for packet in self.gateway.start(): bus.enqueue(packet)
        bus.drain()
        self.assertEqual(m.byte(m.profile['auth_ram']),0)
        self.assertTrue(m.receive(bytes.fromhex('350067080000'),0)[0]['payload'].startswith('3503'))
        self.assertEqual(m.storage.snapshot()['tire_circumference'],2150)
        self.assertEqual(authenticate(m)[0]['payload'],'16e2ffffffff')
        self.assertTrue(m.receive(bytes.fromhex('350067080000'),0)[0]['payload'].startswith('3502'))
        self.assertEqual(m.storage.snapshot()['tire_circumference'],2151)

    def test_bad_proof_uses_stock_rejection_and_persists_failure_counter(self):
        m = self.motor
        replies = authenticate(m,bad_proof=True)
        self.assertTrue(replies[0]['payload'].startswith('16e3'))
        self.assertNotEqual(m.byte(m.profile['auth_ram']),2)
        self.assertGreater(m.storage.snapshot()['auth_failures'],0)
        _,rebooted = self.new_motor()
        self.assertEqual(rebooted.storage.snapshot()['auth_failures'],m.storage.snapshot()['auth_failures'])

    def test_stock_rejects_invalid_values_and_unprofiled_writes_leave_storage_unchanged(self):
        m = self.motor
        self.assertEqual(authenticate(m)[0]['payload'],'16e2ffffffff')
        records = dict(m.storage.records)
        snapshot = m.storage.snapshot()
        for payload,reply_prefix in (('101003','1013'),('1010ff','1013'),('350000000000','3503')):
            with self.subTest(payload=payload):
                result = m.receive(bytes.fromhex(payload),0)
                self.assertTrue(result[0]['payload'].startswith(reply_prefix))
                self.assertEqual(m.storage.records,records)
                self.assertEqual(m.storage.snapshot(),snapshot)
                self.assertFalse(self.state.exists())
        # Admission stays narrow even with storage installed: unrelated setting,
        # reset, update and alternate write encodings cannot reach firmware.
        for payload in ('16a80103ffff','01c000','32f000','101100'):
            with self.subTest(payload=payload):
                instructions = m.instructions
                with self.assertRaises(RuntimeError): m.receive(bytes.fromhex(payload),0)
                self.assertEqual(m.instructions,instructions)
                self.assertEqual(m.storage.records,records)
                self.assertEqual(m.storage.snapshot(),snapshot)
                self.assertFalse(self.state.exists())

    def test_storage_failure_cannot_acknowledge_or_update_live_setting(self):
        m = self.motor
        with patch.object(m.storage,'save',side_effect=OSError('modeled disk failure')):
            with self.assertRaisesRegex(OSError,'disk failure'):
                m.receive(bytes.fromhex('101002'),0)
        self.assertEqual(m.storage.snapshot()['lamp'],0)
        self.assertEqual(m.storage.commits,0)
        self.assertFalse(self.state.exists())

    def test_wrong_hash_or_tampered_state_fails_before_execution(self):
        m = self.motor
        m.receive(bytes.fromhex('101002'),0)
        state = json.loads(self.state.read_text())
        state['firmware_sha256'] = '0'*64
        self.state.write_text(json.dumps(state))
        with self.assertRaisesRegex(RuntimeError,'match firmware'): self.new_motor()
        state['firmware_sha256'] = m.profile['sha256']
        # Corrupt checksums must fail before any firmware protocol operation.
        state['records']['35'] = '01'+'00'*7
        self.state.write_text(json.dumps(state))
        with self.assertRaisesRegex(RuntimeError,'checksum'): self.new_motor()

    def test_writes_remain_disabled_by_default_and_other_hashes_fail(self):
        image,profile = identify(self.fixtures['ep800-4.2.0'],'motor')
        m = Motor(image,profile)
        with self.assertRaisesRegex(RuntimeError,'only normal'): m.receive(bytes.fromhex('101002'),0)
        if 'ep800-4.1.10' in self.fixtures:
            image,profile = identify(self.fixtures['ep800-4.1.10'],'motor')
            with self.assertRaisesRegex(RuntimeError,'exact EP800'): Motor(image,profile).install_storage()

    def test_journal_wear_leveling_and_history_retention(self):
        m = self.motor
        s = m.storage
        self.assertEqual(s.active_bank, 0)
        self.assertEqual(s.compactions, 0)

        # Update lamp state (record 0x23) to generate history
        m.receive(bytes.fromhex('101002'), 0)
        self.assertEqual(len(s.history.get(0x23, [])), 1)

        # Simulate wear-leveling page counter advancement past 32 to trigger bank rotation
        m.write_byte(0x20002E2E, 31)
        m.receive(bytes.fromhex('101001'), 0)
        self.assertEqual(s.active_bank, 1)
        self.assertEqual(s.compactions, 1)
        # Non-whitelisted record 0x23 history should be pruned on compaction
        self.assertNotIn(0x23, s.history)

        # Populate whitelisted record 0x05 history
        s.history[0x05] = [b'\x01'*8] * 10
        # Trigger another compaction (page counter at 63 -> 64)
        m.write_byte(0x20002E2E, 63)
        m.receive(bytes.fromhex('101002'), 0)
        self.assertEqual(s.active_bank, 2)
        self.assertEqual(s.compactions, 2)
        # Record 0x05 must retain exactly 7 historical versions
        self.assertEqual(len(s.history[0x05]), 7)


