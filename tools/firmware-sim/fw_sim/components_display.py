"""Exact stock SC-EM800/E6100/E7000 execution and an explicit EEPROM boundary.

The receive parser, PC-link proof, settings setters/getters, scheduler, record
staging/checksums and reply construction execute unchanged Thumb instructions.
Synthetic RAM replaces boot and factory NVM. Physical EEPROM commit/ready and
boot hydration of its selected cache are modeled; board reset, display hardware
and EEPROM electrical behavior are not represented.
"""
import json
import os
from pathlib import Path
import struct
import tempfile

from .machine import Machine, INPUT, require, UC_ARM_REG_PC, UC_ARM_REG_LR
from .profiles import identify


# Entries and literals were recovered by normalized Thumb comparison, followed
# by independent execution of every schema choice and all 256 raw byte inputs.
# The older versions have different cache layouts; no address is inherited from
# a filename/version guess. The image digest is checked again by Machine.
DISPLAY_PROFILES = {
    'sc-e6100-4.1.0': dict(
        sha256='8149fe326ddf14e439c83ab09141cfb2b08bd745938b34fbcf603a3fb8e7f71a',
        receive=0x216BC, scheduler=0x21858, group1_table=0x1D864,
        link=0x2A78C, key=0x2A838, transport=0x36068,
        node_ram=0x200002A0, ready_ram=0x2000030B,
        record_table=0x10424, record_state=0x20002A58, record_count=15,
        commit=0x2B4B8, commit_busy=0x2000010B,
        settings_record=10, caches=(0x200001D2, 0x200001D3, 0x2000023A),
        backlight_cache=0x200001D1,
    ),
    'sc-e7000-4.1.0': dict(
        sha256='1f3c42ad0cc3e46d2e9affd5f023245f645acbc1c8ee3d68569bfcb25c9c3e79',
        receive=0x19184, scheduler=0x19328, group1_table=0x15D7C,
        link=0x21C80, key=0x21D2C, transport=0x2BF74,
        node_ram=0x200001F8, ready_ram=0x20000263,
        record_table=0x10410, record_state=0x20002868, record_count=13,
        commit=0x229C4, commit_busy=0x200000F7,
        settings_record=8, caches=(0x20000176, 0x20000177, 0x20000178),
    ),
    'sc-em800-4.0.0': dict(
        sha256='6239988c78ea734cee44c7c61477a9498dfa79f38e392d6c38db580687482bc8',
        receive=0x20B90, scheduler=0x20D2C, group1_table=0x1E5EC,
        link=0x1E9E8, key=0x1EA98, transport=0x1A8C8,
        node_ram=0x20000228, ready_ram=0x200003A2,
        record_table=0x10504, record_state=0x200029F0, record_count=13,
        commit=0x2748C, commit_busy=0x200001CF,
        settings_record=8, caches=(0x20000131, 0x20000132, 0x2000010D, 0x200007D2),
    ),
    'sc-em800-4.1.0': dict(
        sha256='9a5e8212034c17cb3ce56db59edacf99c69971f952ff13b03fb55ca59e9b3c82',
        receive=0x20BAC, scheduler=0x20D4C, group1_table=0x1E608,
        link=0x1EA04, key=0x1EAB4, transport=0x1A8E4,
        node_ram=0x20000254, ready_ram=0x200002BF,
        record_table=0x10520, record_state=0x20002A68, record_count=13,
        commit=0x27444, commit_busy=0x200001FB,
        settings_record=8, caches=(0x2000015D, 0x2000015E, 0x20000139, 0x2000072A),
    ),
    'sc-em800-4.3.0': dict(
        sha256='0f2c469d62803df67df6bd09d46fdaa4201c71b8f8d9fec6c84ef964b6415e32',
        receive=0x20088, scheduler=0x20254, group1_table=0x2DBA0,
        link=0x2DF9C, key=0x2E04C, transport=0x32498,
        node_ram=0x200002C8, ready_ram=0x20000333,
        record_table=0x1059C, record_state=0x20002B5C, record_count=13,
        commit=0x2A32C, commit_busy=0x20000207,
        settings_record=8, caches=(0x2000012A, 0x2000012B, 0x2000012C, 0x2000012D),
    ),
    'sc-em800-4.4.0': dict(
        sha256='e473f3326615d83cd2e60124ebe7df73541b4e23bb417b3ac8615ca4dd970617',
        receive=0x20340, scheduler=0x2050C, group1_table=0x2DE58,
        link=0x2E254, key=0x2E304, transport=0x32750,
        node_ram=0x200002CC, ready_ram=0x20000337,
        record_table=0x1059C, record_state=0x20002B5C, record_count=13,
        commit=0x2A5E4, commit_busy=0x2000020B,
        settings_record=8, caches=(0x20000116, 0x20000117, 0x20000118, 0x20000119),
    ),
    'sc-em800-4.5.0': dict(
        sha256='8aa8bbe626c44daefb56b386f616d68146001dfd60fd284c0c2ba6770c17877d',
        receive=0x2038E, scheduler=0x20564, group1_table=0x2DFCC,
        link=0x2E3C8, key=0x2E478, transport=0x328B8,
        node_ram=0x200002F0, ready_ram=0x2000035B,
        record_table=0x1059C, record_state=0x20002B5C, record_count=13,
        commit=0x2A714, commit_busy=0x2000022F,
        settings_record=8, caches=(0x2000013A, 0x2000013B, 0x2000013C, 0x2000013D),
    ),
    'sc-em800-4.6.0': dict(
        sha256='c943c90239efbcf36037cb8fa3399ab34d012913774e26728341750b96e30c6f',
        receive=0x206D2, scheduler=0x208B0, group1_table=0x2D4C0,
        link=0x2D8BC, key=0x2D96C, transport=0x31E94,
        node_ram=0x20000318, ready_ram=0x20000383,
        record_table=0x105C8, record_state=0x20002B5C, record_count=14,
        commit=0x29C24, commit_busy=0x20000257,
        settings_record=8, caches=(0x20000162, 0x20000163, 0x20000164, 0x20000165),
    ),
}

READS = {(1, command) for command in (0x1C, 0x2C, 0x3C)} | {
    (0x23, 4), (5, 0x1C), (5, 0x24), (5, 0x54),
}
WRITES = {(0x23, 0): 1, (5, 0): 1, (5, 0x18): 1, (5, 0x20): 1, (5, 0x50): 4}
LINK = {(0x32, 0x10), (0x32, 0x30)}


class Display(Machine):
    def __init__(self, image, profile, allow_writes=False, storage_path=None):
        require(profile['id'] in DISPLAY_PROFILES, 'unsupported display component profile')
        recovered = DISPLAY_PROFILES[profile['id']]
        require(profile['sha256'] == recovered['sha256'], 'display component profile hash mismatch')
        super().__init__(image, profile)
        self.display = recovered
        self.allow_writes = allow_writes
        reads, writes = set(READS), set(WRITES) - {(5, 0)}
        if profile['model'] != 'SC-EM800':
            reads.remove((5, 0x54))
            writes.remove((5, 0x50))
        if 'backlight_cache' in recovered:
            reads.add((5, 4))
            writes.add((5, 0))
        self.allowed_commands = reads | LINK | (writes if allow_writes else set())
        self.path = Path(storage_path) if storage_path is not None else None
        self.commits = 0
        self.records = {}
        self.metadata = {}
        for index in range(recovered['record_count']):
            base = recovered['record_table'] + index * 16
            length = self.byte(base + 4)
            pointer = int.from_bytes(self.u.mem_read(base + 8, 4), 'little')
            require(1 <= length <= 40 and 0x20000000 <= pointer <= 0x20010000 - length,
                    'invalid stock display EEPROM metadata')
            self.metadata[index] = (pointer, length)
        # Synthetic distance km, language English, beep on, brightness 60%.
        # Seven-byte meaningful record body plus the stock additive checksum.
        seed = bytes([0, 0, 1, 0, 1 if 'backlight_cache' in recovered else 0,
                      60 if profile['model'] == 'SC-EM800' else 0, 0])
        self.records[recovered['settings_record']] = seed + bytes([-sum(seed) & 255])
        if self.path is not None and self.path.exists():
            require(self.path.stat().st_size <= 16384, 'display state exceeds limit')
            saved = json.loads(self.path.read_text())
            require(isinstance(saved, dict) and saved.get('schema_version') == 1
                    and saved.get('firmware_sha256') == recovered['sha256'],
                    'display state does not match firmware')
            records = saved.get('records')
            require(isinstance(records, dict) and len(records) <= recovered['record_count'],
                    'invalid display records')
            for key, value in records.items():
                require(isinstance(key, str) and key.isdecimal() and int(key) in self.metadata
                        and isinstance(value, str), 'invalid display record')
                data = bytes.fromhex(value)
                require(len(data) == self.metadata[int(key)][1] and sum(data) & 255 == 0,
                        'invalid display record length/checksum')
                self.records[int(key)] = data
        for index, data in self.records.items():
            self.u.mem_write(self.metadata[index][0], data)
        self._hydrate()
        # Modeled boot interface role and readiness. PC mode/owner/proof are
        # deliberately left zero and can only be established by stock handlers.
        node = recovered['node_ram']
        self.write_byte(node + 0x1F, 1)
        self.write_byte(node + 0x15, 1)
        self.write_byte(node + 0x0F, image[40])
        self.write_byte(recovered['ready_ram'], 1)

    @classmethod
    def from_path(cls, path, **kwargs):
        return cls(*identify(path, 'gateway'), **kwargs)

    def _hydrate(self):
        """Modeled boot cache hydration, not full firmware boot execution."""
        p = self.display
        data = self.records[p['settings_record']]
        require(data[0] in (0, 1) and data[1] <= 5 and data[2] in (0, 1),
                'invalid synthetic display setting record')
        if self.profile['model'] == 'SC-EM800':
            require(data[5] in (20, 40, 60, 80, 100), 'invalid synthetic display brightness record')
        if 'backlight_cache' in p:
            require(data[4] in (0, 1, 2), 'invalid synthetic display backlight record')
            self.write_byte(p['backlight_cache'], {0: 1, 1: 0, 2: 2}[data[4]])
        # Stock EEPROM stores user-facing beep and brightness; caches contain
        # inverted beep and a 0..4 brightness index, as the stock getters show.
        for address, value in zip(p['caches'], (data[0], data[1], data[2] ^ 1, data[5] // 20 - 1)):
            self.write_byte(address, value)

    @property
    def connected(self):
        return self.byte(self.display['node_ram'] + 0x0E) == 1

    def _save(self, records):
        if self.path is None:
            return
        saved = dict(schema_version=1, firmware_sha256=self.display['sha256'],
                     records={str(index): data.hex() for index, data in sorted(records.items())})
        temporary = None
        try:
            with tempfile.NamedTemporaryFile(mode='w', dir=self.path.parent, prefix='.display-state-', delete=False) as stream:
                temporary = Path(stream.name)
                json.dump(saved, stream)
                stream.write('\n')
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self.path)
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
        p = self.display
        if address == p['commit']:
            require(self.allow_writes, 'display EEPROM commit requires explicit opt-in')
            pending = []
            records = dict(self.records)
            for index, (pointer, length) in self.metadata.items():
                if self.byte(p['record_state'] + index * 10 + 6) == 1:
                    data = bytes(u.mem_read(pointer, length))
                    require(sum(data) & 255 == 0, 'stock display EEPROM checksum invalid')
                    pending.append(index)
                    records[index] = data
            # Saving precedes the modeled ready/commit flags. A failing disk
            # write cannot acknowledge successful persistence to stock code.
            self._save(records)
            self.records = records
            self.commits += len(pending)
            for index in pending:
                self.write_byte(p['record_state'] + index * 10 + 6, 0)
                self.write_byte(p['record_state'] + index * 10 + 8, 1)
            self.write_byte(p['commit_busy'], 0)
            u.reg_write(UC_ARM_REG_PC, u.reg_read(UC_ARM_REG_LR))
        elif address == p['transport']:
            self.accept_packet()

    def receive(self, payload, source):
        require(2 <= len(payload) <= 9, 'display packet must contain 2..9 bytes')
        command = tuple(payload[:2])
        require(command in self.allowed_commands, 'unsupported display component command')
        if command in WRITES:
            require(len(payload) == 2 + WRITES[command], 'incorrect display SET width')
        elif command in LINK:
            require(len(payload) == 6, 'incorrect display PC-link width')
        else:
            require(len(payload) == 3, 'incorrect display query width')
        self.u.mem_write(INPUT, bytes([max(6, len(payload)), 255]) + payload.ljust(6, b'\xff'))
        before = len(self.output)
        p = self.display
        if payload[0] == 1:
            pointer = p['group1_table'] + (payload[1] // 2) * 4
            entry = struct.unpack('<I', self.u.mem_read(pointer, 4))[0]
            require(0x10000 <= entry < 0x4F000 and entry & 1, 'invalid display stock query entry')
            self.call(entry, source, INPUT)
        elif command in LINK:
            self.call(p['link'] if payload[1] == 0x10 else p['key'], source, INPUT)
        else:
            self.call(p['receive'], source, INPUT)
            self.call(p['scheduler'])
        return self.output[before:]

    def snapshot(self):
        return dict(profile=self.profile['id'], connected=self.connected,
                    modeled_commit=True, commits=self.commits,
                    persisted_record_ids=sorted(self.records), instructions=self.instructions)
