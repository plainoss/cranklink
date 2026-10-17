"""Firmware protocol execution with synthetic initialized RAM and virtual TX."""
import struct
from .machine import Machine, INPUT, require, UC_ARM_REG_R4, UC_ARM_REG_R7


class Motor(Machine):
    def __init__(self, image, profile):
        super().__init__(image, profile)
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

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
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
            # Until storage, auth counters and reset are modeled, only normal
            # GET opcodes are admitted. A caller cannot turn this into flashing.
            require(payload[1] & 7 == 4, 'only normal read commands are supported')
            self.call(p['dispatch'], source, INPUT)
            if p['abi'] == 'legacy':
                self.call(p['scheduler'])
            else:
                self.call(p['scheduler'], stop=p['scheduler_end'],
                          registers=((UC_ARM_REG_R4, 1), (UC_ARM_REG_R7, 0xFE)))
        return self.output[count:]
