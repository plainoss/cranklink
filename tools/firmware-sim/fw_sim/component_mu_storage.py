"""Exact-image MU gear-count record staging with a modeled NVM commit boundary.

The stock setter, range checks, record checksum, cache changes, and ACK execute
unchanged. Physical EEPROM programming is replaced at its synchronous service
entry. Reload hydrates synthetic application caches; reset boot is unvalidated.
"""
import json
import os
from pathlib import Path
import struct
import tempfile
from unicorn.arm_const import UC_ARM_REG_LR, UC_ARM_REG_PC
from .machine import require

SHA = '29f137dae1ab9c335ccd313e70f70ee04058c52c08b00236d5872b1beb93bc59'
TABLE, STATE, COMMIT, GEAR_RECORD = 0x102B0, 0x200006C0, 0x1298A, 10


def gear_record(gears):
    body = bytes([gears]) + b'\xff'*6
    return body + bytes([-sum(body) & 255])


class MuGearRecords:
    def __init__(self, component, gears, path=None):
        require(component.profile['sha256'] == SHA, 'MU storage requires the exact profiled image')
        self.component, self.path = component, Path(path) if path is not None else None
        self.commits = 0
        self.metadata = {}
        for index in range(13):
            length = component.byte(TABLE+16*index+4)
            pointer = struct.unpack('<I',component.u.mem_read(TABLE+16*index+8,4))[0]
            require(1 <= length <= 32 and 0x20000000 <= pointer <= 0x2000FFFF-length,
                    'invalid MU EEPROM record metadata')
            self.metadata[index] = pointer,length
        self.record = gear_record(gears)
        if self.path is not None and self.path.exists():
            require(self.path.stat().st_size <= 4096, 'MU persistent state exceeds limit')
            state = json.loads(self.path.read_text())
            require(state.get('schema_version') == 1 and state.get('firmware_sha256') == SHA,
                    'MU persistent state does not match firmware')
            value = state.get('gear_record')
            require(isinstance(value,str), 'missing MU gear record')
            self.record = bytes.fromhex(value)
        require(len(self.record) == 8 and self.record[0] in (5,8,11)
                and self.record[1:7] == b'\xff'*6 and sum(self.record)&255 == 0,
                'invalid MU gear record length/checksum/value')
        pointer,length = self.metadata[GEAR_RECORD]
        require(length == len(self.record), 'unexpected MU gear record layout')
        component.u.mem_write(pointer,self.record)
        # Boot-cached valid-record marker, with synthetic storage contents.
        component.u.mem_write(STATE+10*GEAR_RECORD,b'\x00\x00')
        variant = (5,8,11).index(self.record[0])
        component.write_byte(component.profile['gear_variant_ram'],variant)
        component.write_byte(component.profile['adjustment_variant_ram'],variant)

    def snapshot(self):
        return dict(profile=self.component.profile['id'],modeled_commit=True,
                    commits=self.commits,gear_count=self.record[0])

    def save(self, data):
        if self.path is None:
            return
        content = json.dumps(dict(schema_version=1,firmware_sha256=SHA,gear_record=data.hex()),indent=2)+'\n'
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode='w',dir=self.path.parent,prefix='.mu-sim-',delete=False) as stream:
                temporary = Path(stream.name)
                stream.write(content)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary,self.path)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)

    def observe(self, address):
        if address != COMMIT:
            return False
        component = self.component
        pending = [index for index in self.metadata if component.byte(STATE+10*index+6) == 1]
        require(set(pending) <= {GEAR_RECORD}, 'MU fixture refuses unrelated EEPROM writes')
        if pending:
            pointer,length = self.metadata[GEAR_RECORD]
            data = bytes(component.u.mem_read(pointer,length))
            require(sum(data)&255 == 0 and data[0] in (5,8,11), 'invalid stock MU gear record')
            self.save(data)
            self.record = data
            self.commits += 1
            component.u.mem_write(STATE+10*GEAR_RECORD,b'\x00\x00')
            component.write_byte(STATE+10*GEAR_RECORD+6,0)
            component.write_byte(STATE+10*GEAR_RECORD+8,1)
        component.u.reg_write(UC_ARM_REG_PC,component.u.reg_read(UC_ARM_REG_LR))
        return True
