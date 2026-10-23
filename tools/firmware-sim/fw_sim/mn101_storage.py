"""Stock region storage routines with a narrowly modeled resident ROM API.

The unavailable ROM service programs a byte store. Application checksum,
record selection, completion handling and cold reload execute stock code.
This is not protected-session authentication or physical flash emulation.
"""
import hashlib
import os
from pathlib import Path
import tempfile

from .mn101 import CPU_PROFILES, MN101Machine, unpack_pc


LAYOUTS = {
    '3.3.0': dict(descriptors=0x4c89, records=32, record_state=0x9ae,
                  shared=0x9a3, bank=0xace, active=0xad3, live=0xf32,
                  scan=0x9e1b, load=0x9fc9, hydrate=0xee09, hydrated=0xee2d,
                  setter=0xfa1b, getter=0xd85d, selector=0xd76, reply=0xb1e),
    '3.4.0': dict(descriptors=0x4ea9, records=34, record_state=0x9c7,
                  shared=0x9bc, bank=0xaf9, active=0xafe, live=0xf69,
                  scan=0xa075, load=0xa225, hydrate=0xf29e, hydrated=0xf2c5,
                  setter=0xfecb, getter=0xdb93, selector=0xda7, reply=0xb49),
}
STORAGE_PROFILES = {p[1]: p[0] for p in CPU_PROFILES
                    if p[0].startswith('e6002-') and p[0].split('-')[1] in LAYOUTS}


class ByteStore:
    """Synthetic NVM banks. Publish in-memory state only after atomic save."""
    def __init__(self, path):
        self.path = Path(path)
        if self.path.exists():
            with self.path.open('rb') as stream: raw = stream.read(0x4001)
        else:
            raw = b'\xff' * 0x4000
        if len(raw) != 0x4000: raise ValueError('invalid simulated NVM image length')
        self.data = bytearray(raw)
        self.commits = []

    def program(self, address, data):
        if not data or not (0x2000 <= address and address + len(data) <= 0x4000):
            raise RuntimeError('storage write outside modeled banks')
        if any((old & new) != new for old, new in zip(self.data[address:address+len(data)], data)):
            return False
        candidate = self.data.copy()
        candidate[address:address+len(data)] = data
        temp = None
        try:
            fd, name = tempfile.mkstemp(prefix=self.path.name+'.', dir=self.path.parent)
            temp = Path(name)
            with os.fdopen(fd, 'wb') as stream:
                stream.write(candidate)
                stream.flush()
                os.fsync(stream.fileno())
            temp.replace(self.path)
        except OSError:
            if temp: temp.unlink(missing_ok=True)
            return False
        self.data = candidate
        self.commits.append(dict(address=address, data=bytes(data).hex()))
        return True


class RegionDevice(MN101Machine):
    """Exact-image application execution plus a disclosed ROM append service."""
    def __init__(self, image, store, service_status=None):
        digest = hashlib.sha256(image).hexdigest()
        if digest not in STORAGE_PROFILES: raise RuntimeError('unsupported persistent MN101 image hash')
        if service_status is not None and (type(service_status) is not int or service_status not in (1, 0x80)):
            raise ValueError('invalid injected resident-service status')
        self.profile = STORAGE_PROFILES[digest]
        version = self.profile.split('-')[1]
        self.layout = LAYOUTS[version]
        super().__init__(image, ram_start=0, ram_end=0x2800, address_bits=20)
        self.store = store
        self.read_mode = 0
        self.services = []
        self.key_state = 0
        self.key_generation = 0
        self.consumed_key = 0
        self.setup = None
        self.service_status = service_status

    def read(self, address, width=1):
        # Bank five is an explicit fixture. Mode two overlays even RAM aliases.
        if self.read_mode == 2 and 0x2000 <= address and address+width <= 0x3000:
            return int.from_bytes(self.store.data[address:address+width], 'little')
        return super().read(address, width)

    def write(self, address, value, width=1):
        if address == 0x3f0a and width == 1:
            if value not in (0, 2): raise RuntimeError('unmodeled storage window mode')
            self.read_mode = value
            return
        # These observed keyed writes have no invented ready/busy response.
        if address == 0x3e50 and width == 1:
            if value == 0xaa and self.key_state == 0:
                self.key_state = 1; return
            if value == 0 and self.key_state == 2:
                self.key_state = 0; self.key_generation += 1; return
            raise RuntimeError('invalid keyed-service order')
        if address == 0x3f4d and width == 1 and value == 0x55 and self.key_state == 1:
            self.key_state = 2; return
        if address == 0x3fbd and width == 1 and value == 0x4b:
            self.setup = value; return
        return super().write(address, value, width)

    def step(self):
        if self.pc != 0x6f899*2:
            return super().step()
        layout = self.layout
        state = self.a[0]
        if (state != layout['shared'] or self.setup != 0x4b or self.read_mode != 0
                or self.key_state != 0 or self.key_generation <= self.consumed_key):
            raise RuntimeError('unsupported resident write-service state')
        address = self.read(state+2, 2)
        count = self.read(state+5)+1
        source = self.read(state+6, 2)
        record_id = self.read(layout['active'])
        record = layout['record_state'] + 9*record_id
        if (record_id != 11 or self.a[1] != record or self.d[2] != count
                or self.read(layout['bank']) != 5 or self.read(state+4) != 2):
            raise RuntimeError('unsupported resident record identity')
        descriptor = layout['descriptors'] + 8*record_id
        start, end = self.read(descriptor, 2), self.read(descriptor+2, 2)
        length, cache = self.read(descriptor+4), self.read(descriptor+5, 2)
        slot = self.read(record+2, 2)
        if (count != length or length != 2 or source != cache or source+count > 0x2800
                or address != (0x2000 | slot) or slot < start
                or slot+count-1 > end or (slot-start) % length):
            raise RuntimeError('resident request differs from stock record descriptor')
        data = bytes(self.read(source+i) for i in range(count))
        if sum(data) & 255: raise RuntimeError('resident record checksum is invalid')
        if self.service_status is not None:
            # Fault injection never programs storage or fabricates completion.
            status = self.service_status
        else:
            status = 0 if self.store.program(address, data) else 0x80
        self.services.append(dict(target='6f899', address=address, count=count,
                                  source=source, data=data.hex(), status=status))
        self.d[0] = status
        self.pc = unpack_pc(self.pop(3), 20)
        self.setup = None
        self.consumed_key = self.key_generation

    def call(self, byte_entry, budget=40000):
        self.trace = []
        result = self.run(byte_entry*2, budget=budget)
        if not result['returned']:
            raise RuntimeError(result['error'] or 'stock routine did not return')
        return result

    def initialize(self):
        # Bank selection is a fixture; no full hardware boot is claimed.
        self.write(self.layout['bank'], 5)
        counts = [self.call(self.layout[name])['instructions'] for name in ('scan', 'load')]
        self.pc = self.layout['hydrate']*2
        self.trace = []
        original_sp = self.sp
        for _ in range(2000):
            if self.pc == self.layout['hydrated']*2:
                if self.sp != original_sp: raise RuntimeError('unbalanced stock hydration stack')
                return counts + [len(self.trace)]
            self.step()
        raise RuntimeError('stock hydration instruction budget exhausted')

    def region(self):
        return self.read(self.layout['live'])

    def set_region(self, region):
        if type(region) is not int or region not in (0, 1, 4): raise ValueError('unsupported region fixture')
        self.d[:2] = [1, region]
        result = self.call(self.layout['setter'])
        if self.d[0] != 1 or self.region() != region:
            raise RuntimeError('stock region SET did not confirm the requested value')
        return result

    def get_region(self):
        self.write(self.layout['selector'], 1)
        result = self.call(self.layout['getter'])
        reply = bytes(self.read(self.layout['reply']+i) for i in range(8))
        expected = bytes([6, 0, 0x16, 0xae, 1, self.region(), 0, 0])
        if reply != expected: raise RuntimeError('unexpected stock region GET reply')
        return result, reply


def verify_region(image_path, state_path, region):
    """One stock SET followed by a fresh CPU and disk-backed stock reload."""
    with Path(image_path).open('rb') as stream: image = stream.read(0x40001)
    first = RegionDevice(image, ByteStore(state_path))
    cold = first.initialize()
    before = first.region()
    saved = first.set_region(region)
    # Reopen the file, not the previous object's byte array or RAM.
    second = RegionDevice(image, ByteStore(state_path))
    reloaded = second.initialize()
    got, reply = second.get_region()
    if second.region() != region: raise RuntimeError('region did not survive stock cold reload')
    return dict(passed=True, profile=first.profile, before=before, after=second.region(),
                commits=first.store.commits, services=first.services, reply=reply.hex(),
                instructions=dict(initialization=cold, setter=saved['instructions'],
                                  cold_reload=reloaded, getter=got['instructions']),
                hardware_access=False, level='isolated stock storage routines with modeled resident ROM',
                modeled=['resident ROM append API', 'atomic file-backed NVM', 'bank-five/read-window fixture',
                         'keyed-service writes and setup latch'],
                not_validated=['resident ROM instructions', 'physical flash/timing', 'full boot/bank selection',
                               'protected-session authentication', 'transport', 'erase/rollover'])
