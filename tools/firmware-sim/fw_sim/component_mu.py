"""Hash-fenced MU-UR500 stock handlers; synthetic boot caches, virtual TX only.

This fixture does not model actuator completion or reset/boot. Opt-in saves
model gear-record NVM commit at a declared physical service boundary.
GET packet parsing, selection, scheduler, and reply construction execute stock
Thumb instructions. No replies, authorization results or setter outcomes are
fabricated by the host.
"""
import struct
from .machine import Machine, INPUT, require

PROFILE = {
    'id': 'mu-ur500-3.3.1', 'kind': 'component', 'model': 'MU-UR500',
    'sha256': '29f137dae1ab9c335ccd313e70f70ee04058c52c08b00236d5872b1beb93bc59',
    'dispatch': 0x164F4, 'scheduler': 0x1711A,
    'transport': 0x1476C, 'queue': 0x1549A,
    'group1_table': 0x111E0, 'group32_table': 0x11380,
    'mode_ram': 0x20000C3C, 'owner_ram': 0x20000C42,
    'adjustment_ram': 0x20000C6D, 'target_adjustment_ram': 0x20000C6F,
    'gear_variant_ram': 0x20000C86, 'adjustment_variant_ram': 0x20000C87,
}


class MuUr500(Machine):
    allowed_commands = frozenset((1, op) for op in (0x1c,0x2c,0x34,0x3c,0x84,0xa4,0xac)) | {(0x32,0x10),(0x32,0x30),(0x0b,0x14),(0x0b,0x1c)}

    storage = None

    def __init__(self, image, adjustment=0, gears=11, allow_saves=False, storage_path=None):
        require(-4 <= adjustment <= 4, 'unsupported synthetic adjustment seed')
        require(gears in (5, 8, 11), 'unsupported synthetic gear seed')
        require(allow_saves or storage_path is None, 'MU storage path requires explicitly enabled writes')
        super().__init__(image, PROFILE)
        self.storage = None
        self.allowed_commands = self.allowed_commands | ({(0x0b,0x10),(0x0b,0x18)} if allow_saves else set())
        # These synthetic application caches stand in for boot/NVM hydration.
        # Stock identity uses the image header, never a fabricated response.
        self.write_byte(0x20000496, image[40])
        self.write_byte(0x20000497, image[41])
        self.u.mem_write(0x200009F0, bytes(11))
        self.write_byte(PROFILE['adjustment_ram'], adjustment & 255)
        self.write_byte(PROFILE['gear_variant_ram'], (5,8,11).index(gears))
        self.write_byte(PROFILE['adjustment_variant_ram'], (5,8,11).index(gears))
        # Empty record addresses make unsupported NVM access fail honestly.
        for index in range(13):
            self.u.mem_write(0x200006C0 + 10*index, b'\xff\xff')
        if allow_saves:
            from .component_mu_storage import MuGearRecords
            self.storage = MuGearRecords(self,gears,storage_path)
            # Synthetic postboot actuator idle state enables the stock setter.
            self.write_byte(0x20000C65,1)

    @property
    def connected(self):
        return self.byte(PROFILE['mode_ram']) == 1

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
        if self.storage is not None and self.storage.observe(address):
            return
        if address in (PROFILE['transport'], PROFILE['queue']):
            self.accept_packet()

    def receive(self, payload, source):
        require(2 <= len(payload) <= 9, 'unsupported component packet length')
        group, command = payload[:2]
        require((group,command) in self.allowed_commands, 'MU command not admitted')
        if group == 0x0b and command in (0x10,0x18):
            require(len(payload) == 6, 'MU SET requires the complete four-byte parameter tuple')
        if group == 1:
            require(command in (0x1c,0x2c,0x34,0x3c,0x84,0xa4,0xac), 'unsupported identity query')
            table = PROFILE['group1_table']
        elif group == 0x32:
            require(command in (0x10,0x30), 'unsupported lifecycle command')
            table = PROFILE['group32_table']
        else:
            require(group == 0x0b, 'unsupported MU command family')
            table = None
        self.u.mem_write(INPUT, bytes([len(payload),255]) + payload.ljust(9,b'\xff'))
        before = len(self.output)
        if table is not None:
            entry = struct.unpack('<I', self.u.mem_read(table + (command//2)*4,4))[0]
            require(0x10000 <= entry < 0x10000+0xfb58 and entry & 1, 'invalid component dispatch entry')
        else:
            entry = PROFILE['dispatch']
        self.call(entry, source, INPUT)
        self.call(PROFILE['scheduler'])
        return self.output[before:]
