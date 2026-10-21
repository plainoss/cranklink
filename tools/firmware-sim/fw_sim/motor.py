"""Firmware protocol execution with synthetic initialized RAM and virtual TX."""
import struct
from .machine import Machine, INPUT, require, UC_ARM_REG_R4, UC_ARM_REG_R7
from .speed_patch import speed_patch_ceiling


class Motor(Machine):
    storage = None

    def __init__(self, image, profile):
        super().__init__(image, profile)
        self._image = bytes(image)
        self.speed_patch_ceiling = speed_patch_ceiling(self._image)
        self.storage = None
        if profile['abi'] == 'legacy':
            self.u.mem_write(profile['table'], b'\xff' * 33)
        # Direct handler entry replaces boot and the interrupt receive loop.
        # Role selection is a fixture; PC-link mode/owner are set by firmware.
        self.write_byte(profile['role_ram'], 1)
        self.write_byte(profile['series_ram'], image[40])
        if 'cargo_ram' in profile:
            self.u.mem_write(profile['cargo_ram'], profile['cargo_fixture'].encode('ascii').ljust(7,b'\0'))
        if 'rx_cache' in profile:
            self.u.mem_write(profile['rx_cache'], image[47:50])

    def install_storage(self, path=None):
        from .persistence import PersistentRecords
        require(self.storage is None, "storage already installed")
        self.storage = PersistentRecords(self, path)
        return self.storage

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
        if self.storage is not None and self.storage.observe(address):
            return
        if address in (self.profile['transport'], self.profile['queue']):
            self.accept_packet()

    @property
    def connected(self):
        return self.byte(self.profile['mode_ram']) == 1

    def receive(self, payload, source):
        require(2 <= len(payload) <= 9, 'motor packet must contain 2..9 bytes')
        payload = payload.ljust(6, b'\xff')
        self.u.mem_write(INPUT, bytes([len(payload), 255]) + payload)
        count = len(self.output)
        p = self.profile
        if payload[0] == 1:
            require(payload[1] in (0x1c,0x2c,0x3c,0x84,0x34,0xa4,0xac), 'unsupported stock query')
            entry = struct.unpack('<I', self.u.mem_read(p['group1_table'] + (payload[1]//2)*4, 4))[0]
            require(0x10000 <= entry < 0x4f000 and entry & 1, 'invalid firmware dispatch table')
            self.call(entry, source, INPUT)
        elif payload[:2] == b'\x32\x10':
            self.call(p['link'], source, INPUT)
        elif payload[:2] == b'\x32\x30':
            self.call(p['key'], source, INPUT)
        else:
            # Writes are opt-in and limited to the exact-profile storage/auth
            # path. Reset, flashing and unrelated setting writes stay excluded.
            allowed_write = (payload[0],payload[1]) in ((0x10,0x10),(0x35,0),(0x16,0xd8),(0x16,0xe0))
            # Speed SET is an opt-in simulator capability for the exact patch.
            # The unchanged firmware performs its checks and modeled commit.
            speed_write = (self.speed_patch_ceiling is not None
                           and len(payload) == 6 and payload[:2] == b'\x16\xb0'
                           and payload[4:] == b'\x01\xff')
            allowed_write = allowed_write or speed_write
            require(payload[1] & 7 == 4 or self.storage is not None and allowed_write,
                    'only normal reads or explicitly enabled persistent settings are supported')
            self.call(p['dispatch'], source, INPUT)
            if p['abi'] == 'legacy':
                self.call(p['scheduler'])
            else:
                self.call(p['scheduler'], stop=p['scheduler_end'],
                          registers=((UC_ARM_REG_R4, 1), (UC_ARM_REG_R7, 0xFE)))
        return self.output[count:]
