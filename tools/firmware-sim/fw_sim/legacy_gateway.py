"""Isolated, hash-pinned SC-E8000 PCE probes, not a radio/bus emulator.

Only transport delivery and plain peripheral registers are modeled. Stock code
executes the dispatcher, mode transition, discovery countdown and reply builders.
PC-link startup and motor ownership remain outside this probe's coverage.
"""
import hashlib
from .mn101 import BASE, MN101Machine, unpack_pc
from .mn101_decode import decode

PROFILE = 'sc-e8000-4.0.7'
SHA256 = '950b14687717c3c43b2a59f9958e0d9f0775b2c3e63d9d2e805a9cdbb3efaf22'


class LegacyGateway(MN101Machine):
    def __init__(self, image):
        if len(image) != 98110 or hashlib.sha256(image).hexdigest() != SHA256:
            raise RuntimeError('unsupported SC-E8000 probe image SHA-256')
        super().__init__(image, ram_start=0, ram_end=0x2800, address_bits=20)
        self.output = []
        self.mmio = {}
        self.boundaries = []
        # Synthetic initialized gateway caches; no physical boot is claimed.
        self.write(0x1e64, 13)
        self.write(0x1e65, 13)

    @staticmethod
    def plain_mmio(address, width):
        return (0x3200 <= address and address + width <= 0x3300
                or 0x3f00 <= address and address + width <= 0x4000)

    def read(self, address, width=1):
        if self.plain_mmio(address, width):
            self.boundaries.append(('read', address, width))
            return sum(self.mmio.get(address+i, 0) << (8*i) for i in range(width))
        return super().read(address, width)

    def write(self, address, value, width=1):
        if self.plain_mmio(address, width):
            self.boundaries.append(('write', address, width))
            for i in range(width):
                self.mmio[address+i] = value >> (8*i) & 255
            return
        super().write(address, value, width)

    def step(self):
        if self.pc == 0x1047a * 2:
            self.output.append(bytes(self.read(self.a[0]+i) for i in range(self.d[0])))
            self.boundaries.append(('modeled-notification', self.d[0]))
            self.pc = unpack_pc(self.pop(3), 20)
            return
        ins = decode(self.image, BASE, self.pc)
        if ins['op'] == 'store_d_io' and ins['args']['address'] in (0xf4, 0xf5, 0xf6):
            address = 0x3f00 + ins['args']['address']
            self.write(address, self.d[ins['args']['reg']])
            self.ha = address
            self.pc += ins['size']
            self.trace.append(ins)
            return
        super().step()

    def execute(self, entry):
        self.sp = 0x2700
        result = self.run(entry, budget=50000)
        if not result['returned']:
            raise RuntimeError(result['error'])
        return result

    def command(self, packet):
        if not 1 <= len(packet) <= 20:
            raise ValueError('PCE probe packet must contain 1..20 bytes')
        for i, byte in enumerate(packet):
            self.write(0x2000+i, byte)
        self.a[0], self.d[0] = 0x2000, len(packet)
        start = len(self.output)
        result = self.execute(0x16c45 * 2)
        return self.output[start:], result

    def tick(self):
        """One stock countdown call; deliberately no wall-clock conversion."""
        start = len(self.output)
        result = self.execute(0x16c15 * 2)
        return self.output[start:], result
