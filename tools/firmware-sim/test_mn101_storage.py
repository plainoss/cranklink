"""Synthetic storage checks and optional exact-image application regressions."""
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from fw_sim.mn101 import CPU_PROFILES, STOP, pack_pc
from fw_sim.mn101_storage import ByteStore, RegionDevice, verify_region


class ByteStoreTests(unittest.TestCase):
    def test_program_is_file_backed_and_rejects_zero_to_one_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'nvm.bin'
            store=ByteStore(path)
            self.assertFalse(path.exists())
            self.assertTrue(store.program(0x2d24,b'\x04\xfc'))
            saved=path.read_bytes()
            self.assertEqual(saved[0x2d24:0x2d26],b'\x04\xfc')
            self.assertEqual(ByteStore(path).data,store.data)
            self.assertFalse(store.program(0x2d24,b'\x01\xff'))
            self.assertEqual(path.read_bytes(),saved)
            self.assertEqual(bytes(store.data),saved)
            self.assertEqual(len(store.commits),1)

    def test_save_faults_do_not_publish_memory_or_disk_changes(self):
        for fault in ('replace','fsync'):
            with self.subTest(fault=fault),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                store=ByteStore(path)
                self.assertTrue(store.program(0x2d20,b'\0\0'))
                before=path.read_bytes()
                target=Path if fault=='replace' else os
                with patch.object(target,fault,side_effect=OSError('injected save failure')):
                    self.assertFalse(store.program(0x2d24,b'\x04\xfc'))
                self.assertEqual(bytes(store.data),before)
                self.assertEqual(path.read_bytes(),before)
                self.assertEqual(bytes(ByteStore(path).data),before)
                self.assertEqual(len(store.commits),1)
                self.assertEqual(list(Path(temporary).iterdir()),[path])

    def test_missing_parent_does_not_create_or_acknowledge_a_store(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'missing'/'nvm.bin'
            store=ByteStore(path)
            before=bytes(store.data)
            self.assertFalse(store.program(0x2d24,b'\x04\xfc'))
            self.assertEqual(bytes(store.data),before)
            self.assertEqual(store.commits,[])
            self.assertFalse(path.exists())

    def test_invalid_storage_sizes_ranges_and_empty_writes_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'nvm.bin'
            for length in (0,1,0x3fff,0x4001,0x4002):
                with self.subTest(length=length):
                    path.write_bytes(b'\xff'*length)
                    with self.assertRaises(ValueError): ByteStore(path)
            path.unlink()
            store=ByteStore(path)
            for address,data in ((0x1fff,b'\0'),(0x3fff,b'\0\0'),
                                 (0x4000,b'\0'),(0x2000,b'')):
                with self.subTest(address=address,data=data):
                    with self.assertRaises(RuntimeError): store.program(address,data)
            self.assertFalse(path.exists())

    def test_unknown_image_is_rejected_before_cpu_or_storage_execution(self):
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'nvm.bin'
            store=ByteStore(path)
            with self.assertRaises(RuntimeError): RegionDevice(b'not a supported image',store)
            self.assertFalse(path.exists())
            self.assertEqual(store.commits,[])


@unittest.skipUnless(os.environ.get('CRANKLINK_MN101_FIXTURES'),
                     'set CRANKLINK_MN101_FIXTURES for stock storage-routine checks')
class StockRegionStorageTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        manifest=Path(os.environ['CRANKLINK_MN101_FIXTURES']).resolve()
        entries=json.loads(manifest.read_text())
        if not isinstance(entries,dict) or not entries:
            raise ValueError('fixture manifest must be a nonempty profile-id:path object')
        known={profile[0]:profile[1] for profile in CPU_PROFILES}
        cls.fixtures={}
        for profile,path in entries.items():
            if profile not in known: raise ValueError('unknown fixture profile '+profile)
            if not isinstance(path,str) or not path: raise ValueError('invalid fixture path')
            if not profile.startswith('e6002-'): continue
            source=(manifest.parent/path).resolve()
            image=source.read_bytes()
            if hashlib.sha256(image).hexdigest()!=known[profile]:
                raise ValueError('fixture hash mismatch for '+profile)
            cls.fixtures[profile]=(source,image)
        if not cls.fixtures: raise unittest.SkipTest('manifest contains no region-storage CPU profiles')

    def cold(self,image,path,service_status=None):
        cpu=RegionDevice(image,ByteStore(path),service_status=service_status)
        self.assertEqual(bytes(cpu.ram),bytes(0x2800))
        counts=cpu.initialize()
        self.assertEqual(len(counts),3)
        self.assertTrue(all(count>0 for count in counts))
        self.assertEqual(cpu.sp,0x1f00)
        self.assertEqual(cpu.read_mode,0)
        return cpu

    def test_inventory_separates_modeled_storage_from_packet_support(self):
        from fw_sim.inventory import inventory
        from fw_sim.profiles import identify
        for profile,(source,_) in self.fixtures.items():
            with self.subTest(profile=profile):
                report,fixtures=inventory([source])
                row=report['images'][0]
                self.assertEqual(fixtures,{})
                self.assertFalse(row['execution']['supported'])
                self.assertEqual(row['region_storage'],dict(
                    supported=True,profile=profile,resident_rom='modeled',
                    transport=False,authentication=False,erase_rollover=False))
                with self.assertRaisesRegex(RuntimeError,'region-storage .*modeled resident ROM'):
                    identify(source,'motor')

    def record(self,cpu):
        descriptor=cpu.layout['descriptors']+11*8
        return (cpu.read(descriptor,2),cpu.read(descriptor+2,2),
                cpu.read(descriptor+4),cpu.read(descriptor+5,2))

    def assert_region(self,cpu,value):
        self.assertEqual(cpu.region(),value)
        result,reply=cpu.get_region()
        self.assertTrue(result['returned'])
        self.assertEqual(reply,bytes([6,0,0x16,0xae,1,value,0,0]))

    def test_four_file_appends_survive_fresh_cpu_disk_reload_and_restore(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path)
                self.assert_region(cpu,255)
                start,end,length,cache=self.record(cpu)
                self.assertEqual(length,2)
                self.assertEqual(end-start+1,8)
                for index,region in enumerate((0,1,4,0)):
                    old=bytes(cpu.store.data)
                    result=cpu.set_region(region)
                    self.assertTrue(result['returned'])
                    self.assert_region(cpu,region)
                    saved=path.read_bytes()
                    address=0x2000|start+index*length
                    data=bytes([region,(-region)&255])
                    self.assertEqual(saved[address:address+2],data)
                    self.assertEqual(saved[:address],old[:address])
                    self.assertEqual(saved[address+2:],old[address+2:])
                    self.assertEqual(len(cpu.store.commits),1)
                    cpu=self.cold(image,path)
                    self.assert_region(cpu,region)
                    self.assertEqual(bytes(cpu.ram[cache:cache+2]),data)
                    self.assertEqual(cpu.store.commits,[])

    def test_noncompletion_rom_status_does_not_commit_or_acknowledge_set(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path,service_status=1)
                # Stock code treats status1 as noncompletion, advances its
                # candidate slot, and declines the SET; it does not poll it.
                with self.assertRaises(RuntimeError): cpu.set_region(4)
                self.assertEqual([service['status'] for service in cpu.services],[1])
                self.assertEqual(cpu.store.commits,[])
                self.assertFalse(path.exists())
                self.assertEqual(cpu.region(),255)
                self.assert_region(self.cold(image,path),255)

    def test_explicit_rom_failure_status_is_bounded_without_acknowledgement(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path,service_status=0x80)
                with self.assertRaises(RuntimeError): cpu.set_region(4)
                self.assertTrue(cpu.services)
                self.assertTrue(all(service['status']==0x80 for service in cpu.services))
                self.assertEqual(cpu.store.commits,[])
                self.assertFalse(path.exists())
                self.assertEqual(cpu.region(),255)
                self.assert_region(self.cold(image,path),255)

    def test_unchanged_set_does_not_invoke_service_or_rewrite_file(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path)
                cpu.set_region(0)
                before=path.read_bytes()
                cpu=self.cold(image,path)
                cpu.set_region(0)
                self.assert_region(cpu,0)
                self.assertEqual(cpu.services,[])
                self.assertEqual(cpu.store.commits,[])
                self.assertEqual(path.read_bytes(),before)

    def test_slot_exhaustion_does_not_overwrite_or_claim_success(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path)
                for region in (0,1,4,0):
                    cpu.set_region(region)
                    cpu=self.cold(image,path)
                before=path.read_bytes()
                with self.assertRaises(RuntimeError): cpu.set_region(1)
                self.assertEqual(path.read_bytes(),before)
                self.assertEqual(bytes(cpu.store.data),before)
                self.assertEqual(cpu.region(),0)
                self.assert_region(self.cold(image,path),0)

    def test_file_failure_never_acknowledges_or_changes_live_or_durable_region(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path)
                cpu.set_region(0)
                cpu=self.cold(image,path)
                before=path.read_bytes()
                with patch.object(Path,'replace',side_effect=OSError('injected save failure')):
                    with self.assertRaises(RuntimeError): cpu.set_region(4)
                self.assertEqual(cpu.region(),0)
                self.assertEqual(path.read_bytes(),before)
                self.assertEqual(bytes(cpu.store.data),before)
                self.assertTrue(cpu.services)
                self.assertTrue(all(service['status']==0x80 for service in cpu.services))
                self.assert_region(self.cold(image,path),0)

    def test_stock_scanner_checksum_and_erased_gap_behavior(self):
        cases=[(b'\xff\xff',255),(b'\x04\x00',255),(b'\x04\xfc',4),
               (b'\x01\xff',1),(b'\0\0',0),(b'\xff\x01',255),
               (b'\x04\x00\x01\xff',1),(b'\xff\xff\x04\xfc',255),
               (b'\x04\xfc\x01\xff\x04\0',1)]
        for profile,(_,image) in self.fixtures.items():
            for records,expected in cases:
                with self.subTest(profile=profile,records=records.hex()),tempfile.TemporaryDirectory() as temporary:
                    path=Path(temporary)/'nvm.bin'
                    cpu=self.cold(image,path)
                    start,_,_,_=self.record(cpu)
                    data=bytearray(b'\xff'*0x4000)
                    data[0x2000|start:(0x2000|start)+len(records)]=records
                    path.write_bytes(data)
                    cpu=self.cold(image,path)
                    self.assert_region(cpu,expected)
                    self.assertEqual(cpu.store.commits,[])

    def test_storage_window_overrides_ram_only_in_explicit_read_mode(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                cpu=RegionDevice(image,ByteStore(Path(temporary)/'nvm.bin'))
                cpu.write(0x2000,0x12)
                cpu.store.data[0x2000]=0xab
                self.assertEqual(cpu.read(0x2000),0x12)
                cpu.write(0x3f0a,2)
                self.assertEqual(cpu.read(0x2000),0xab)
                cpu.write(0x3f0a,0)
                self.assertEqual(cpu.read(0x2000),0x12)
                with self.assertRaises(RuntimeError): cpu.read(0x2d24)
                with self.assertRaises(RuntimeError): cpu.write(0x3f0a,1)

    def prepared_service(self,image,path):
        cpu=RegionDevice(image,ByteStore(path))
        layout=cpu.layout
        start,_,length,cache=self.record(cpu)
        shared=layout['shared']
        cpu.a[0]=shared
        cpu.a[1]=layout['record_state']+11*9
        cpu.d[2]=length
        cpu.write(layout['bank'],5)
        cpu.write(layout['active'],11)
        cpu.write(cpu.a[1]+2,start,2)
        cpu.write(shared+2,0x2000|start,2)
        cpu.write(shared+4,2)
        cpu.write(shared+5,length-1)
        cpu.write(shared+6,cache,2)
        cpu.write(cache,0xfc04,2)
        for address,value in ((0x3e50,0xaa),(0x3f4d,0x55),(0x3e50,0),(0x3fbd,0x4b)):
            cpu.write(address,value)
        cpu.pc=0x6f899*2
        cpu.push(pack_pc(STOP),3)
        return cpu

    def test_prepared_service_requires_exact_descriptor_and_fresh_completed_key(self):
        for profile,(_,image) in self.fixtures.items():
            for mutation in ('missing-key','partial-key','state','record','count','bank','active',
                             'mode','length','source','destination','unaligned','checksum'):
                with self.subTest(profile=profile,mutation=mutation),tempfile.TemporaryDirectory() as temporary:
                    path=Path(temporary)/'nvm.bin'
                    cpu=self.prepared_service(image,path)
                    shared=cpu.layout['shared']
                    _,_,_,cache=self.record(cpu)
                    if mutation=='missing-key': cpu.key_generation=0
                    elif mutation=='partial-key': cpu.key_state=1
                    elif mutation=='state': cpu.a[0]=0
                    elif mutation=='record': cpu.a[1]=0
                    elif mutation=='count': cpu.d[2]=3
                    elif mutation=='bank': cpu.write(cpu.layout['bank'],6)
                    elif mutation=='active': cpu.write(cpu.layout['active'],10)
                    elif mutation=='mode': cpu.write(0x3f0a,2)
                    elif mutation=='length': cpu.write(shared+5,2)
                    elif mutation=='source': cpu.write(shared+6,0x1100,2)
                    elif mutation=='destination': cpu.write(shared+2,0x2000,2)
                    elif mutation=='unaligned': cpu.write(shared+2,cpu.read(shared+2,2)+1,2)
                    elif mutation=='checksum': cpu.write(cache+1,0)
                    before=bytes(cpu.store.data)
                    with self.assertRaises(RuntimeError): cpu.step()
                    self.assertEqual(bytes(cpu.store.data),before)
                    self.assertEqual(cpu.store.commits,[])
                    self.assertFalse(path.exists())
            with self.subTest(profile=profile,mutation='valid-then-replay'),tempfile.TemporaryDirectory() as temporary:
                cpu=self.prepared_service(image,Path(temporary)/'nvm.bin')
                cpu.step()
                self.assertEqual(cpu.d[0],0)
                self.assertEqual(len(cpu.store.commits),1)
                self.assertEqual(cpu.pc,STOP)
                cpu.write(0x3fbd,0x4b)
                cpu.pc=0x6f899*2
                with self.assertRaises(RuntimeError): cpu.step()
                self.assertEqual(len(cpu.store.commits),1)

    def test_invalid_regions_and_service_statuses_fail_before_writing(self):
        for profile,(_,image) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                cpu=self.cold(image,path)
                for value in (-1,2,3,5,255,True,'1'):
                    with self.assertRaises(ValueError): cpu.set_region(value)
                for status in (-1,0,2,11,1.5,'1',True):
                    with self.assertRaises(ValueError): RegionDevice(image,cpu.store,service_status=status)
                self.assertEqual(cpu.services,[])
                self.assertFalse(path.exists())

    def test_public_verifier_reopens_disk_and_discloses_modeled_boundaries(self):
        for profile,(source,_) in self.fixtures.items():
            with self.subTest(profile=profile),tempfile.TemporaryDirectory() as temporary:
                path=Path(temporary)/'nvm.bin'
                report=verify_region(source,path,4)
                self.assertTrue(report['passed'])
                self.assertEqual(report['profile'],profile)
                self.assertEqual(report['after'],4)
                self.assertEqual(report['reply'],'060016ae01040000')
                self.assertFalse(report['hardware_access'])
                self.assertEqual(len(report['commits']),1)
                self.assertIn('resident ROM instructions',report['not_validated'])
                self.assertIn('protected-session authentication',report['not_validated'])
                self.assertIn('erase/rollover',report['not_validated'])


if __name__=='__main__': unittest.main()
