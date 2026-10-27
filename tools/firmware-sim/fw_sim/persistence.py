"""Modeled EEPROM for exact EP800 D4.2.0 stock or its bounded ceiling patch.

Stock record staging/checksum, setting validation, RAM setters, authentication
and reply builders execute unchanged. This replaces the unavailable physical
EEPROM commit/ready boundary; boot hydration of selected RAM caches is modeled.
"""
import json
import hashlib
import os
from pathlib import Path
import struct
import tempfile
from .machine import require, UC_ARM_REG_PC, UC_ARM_REG_LR
from .speed_patch import STOCK_SHA256, speed_patch_ceiling

SHA = STOCK_SHA256
RECORD_TABLE, RECORD_STATE, COMMIT = 0x10DFC, 0x20001220, 0x284CA
# Values are synthetic fixtures, not recovered customer or device settings.
HYDRATE = {0x23:(0x20002EA7,1), 0x20:(0x20002CDE,2), 0x0D:(0x20002D59,1),
           0x29:(0x2000047C,2)}

# Wear-leveling flash journal configuration for D4.2.0
JOURNAL_BANKS = 3
JOURNAL_BANK_SIZE = 6144  # 6 KiB per bank
# Retention limits per record across compaction: records 0x05, 0x06, 0x0A retain history
JOURNAL_HISTORY_LIMITS = {0x05: 7, 0x06: 3, 0x0A: 7}
JOURNAL_PAGE_COUNTER_ADDR = 0x20002E2E


def record_bytes(payload):
    body = payload.ljust(7,b'\xff')
    return body + bytes([-sum(body) & 255])


class PersistentRecords:
    def __init__(self, motor, path=None):
        self.firmware_sha256 = hashlib.sha256(motor._image).hexdigest()
        require(self.firmware_sha256 == SHA or speed_patch_ceiling(motor._image) is not None,
                'persistent writes require exact EP800 D4.2.0 profile or supported eight-byte patch')
        self.motor, self.path, self.commits = motor, Path(path) if path is not None else None, 0
        self.history = {}
        self.active_bank = 0
        self.compactions = 0
        self.profile_mirrors = 0
        self.metadata = {}
        for index in range(63):
            length = motor.byte(RECORD_TABLE+16*index+4)
            address = struct.unpack('<I',motor.u.mem_read(RECORD_TABLE+16*index+8,4))[0]
            require(1 <= length <= 32 and 0x20000000 <= address <= 0x2000FFFF-length,
                    'invalid stock EEPROM record metadata')
            self.metadata[index] = (address,length)
        self.records = {0x23:record_bytes(b'\x00'),0x20:record_bytes((2149).to_bytes(2,'little')),
                        0x0D:record_bytes(b'\x00'),0x29:record_bytes((2500).to_bytes(2,'little'))}
        if self.path is not None and self.path.exists():
            require(self.path.stat().st_size <= 16384, 'persistent state exceeds limit')
            state = json.loads(self.path.read_text())
            require(state.get('schema_version') == 1 and state.get('firmware_sha256') == self.firmware_sha256,
                    'persistent state does not match firmware')
            records = state.get('records')
            require(isinstance(records,dict) and len(records) <= 63, 'invalid persistent records')
            for key,value in records.items():
                require(key.isdecimal() and int(key) in self.metadata and isinstance(value,str),
                        'invalid persistent record')
                data = bytes.fromhex(value)
                require(len(data) == self.metadata[int(key)][1] and sum(data)&255 == 0,
                        'invalid persistent record length/checksum')
                self.records[int(key)] = data
        for index,data in self.records.items():
            motor.u.mem_write(self.metadata[index][0],data)
        for index,(address,length) in HYDRATE.items():
            motor.u.mem_write(address,self.records[index][:length])
        # Model boot initialization, executing the stock generator and AES setup
        # with the synthetic DCAS serial already exposed by the firmware cache.
        serial = bytes(motor.u.mem_read(0x20002934,6))
        motor.call(0x2F4E4,int.from_bytes(serial[:4],'little'),int.from_bytes(serial[4:],'little'))
        motor.call(0x129DA,0x2000272C)

    def snapshot(self):
        return dict(profile=self.motor.profile["id"],modeled_commit=True,commits=self.commits,
                    lamp=self.motor.byte(0x20002EA7),
                    tire_circumference=int.from_bytes(self.motor.u.mem_read(0x20002CDE,2),"little"),
                    auth_failures=self.motor.byte(0x20002D59),
                    persisted_record_ids=sorted(self.records),
                    journal_bank=self.active_bank,
                    journal_history={str(k):len(v) for k,v in sorted(self.history.items()) if v},
                    compactions=self.compactions,
                    profile_mirrors=self.profile_mirrors)

    def save(self, records):
        if self.path is None: return
        payload = json.dumps(dict(schema_version=1,firmware_sha256=self.firmware_sha256,
                                  records={str(k):v.hex() for k,v in sorted(records.items())},
                                  journal_history={str(k):[item.hex() for item in v] for k,v in sorted(self.history.items()) if v},
                                  active_bank=self.active_bank,
                                  compactions=self.compactions,
                                  profile_mirrors=self.profile_mirrors),indent=2)+'\n'
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode='w',dir=self.path.parent,prefix='.sim-state-',delete=False) as stream:
                temporary = Path(stream.name)
                stream.write(payload);stream.flush();os.fsync(stream.fileno())
            os.replace(temporary,self.path)
        finally:
            if temporary is not None: temporary.unlink(missing_ok=True)

    def observe(self, address):
        if address != COMMIT: return False
        motor = self.motor
        pending = []
        records = dict(self.records)
        for index,(pointer,length) in self.metadata.items():
            if motor.byte(RECORD_STATE+index*10+6) == 1:
                data = bytes(motor.u.mem_read(pointer,length))
                require(sum(data)&255 == 0,'stock EEPROM checksum invalid')
                pending.append(index)
                if index in self.records and self.records[index] != data:
                    if index not in self.history:
                        self.history[index] = []
                    self.history[index].insert(0, self.records[index])
                    if len(self.history[index]) > 64:
                        self.history[index] = self.history[index][:64]
                    if index in (0x16, 0x17, 0x18):
                        self.profile_mirrors += 1
                records[index] = data

        cur_page = (motor.byte(JOURNAL_PAGE_COUNTER_ADDR) + 1) & 0xFF
        motor.write_byte(JOURNAL_PAGE_COUNTER_ADDR, cur_page)
        if cur_page > 0 and (cur_page % 32 == 0):
            self.active_bank = (self.active_bank + 1) % JOURNAL_BANKS
            self.compactions += 1
            for rec_id in list(self.history.keys()):
                limit = JOURNAL_HISTORY_LIMITS.get(rec_id, 0)
                if limit == 0:
                    del self.history[rec_id]
                else:
                    self.history[rec_id] = self.history[rec_id][:limit]

        # A failed modeled commit cannot acknowledge a setting as persisted.
        self.save(records)
        self.records = records
        self.commits += len(pending)
        for index in pending:
            motor.write_byte(RECORD_STATE+index*10+6,0)
            motor.write_byte(RECORD_STATE+index*10+8,1)
        motor.u.reg_write(UC_ARM_REG_PC,motor.u.reg_read(UC_ARM_REG_LR))
        return True
