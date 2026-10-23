"""Experimental execution of pinned Di2 PC-link handlers.

Packet delivery and the firmware's send service are explicit modeled boundaries.
This is service-handler coverage, not WU111 radio/full-bike execution. A stop
receipt pauses BEFORE subsequent housekeeping; it does not prove shifting works.
"""
from pathlib import Path
from .di2_images import IMAGES, MAX_BYTES, assess_image
from .mn101 import CPU, unpack_pc

PROFILES = {
    'BTDN110.4.5.0.dat': dict(entry=0x6749, mode=0x47, owner=0x45, send_vector=4),
    '2BTDN110.4.5.0.dat': dict(entry=0x678c, mode=0x47, owner=0x45, send_vector=4),
    'EWWU101.4.6.0.dat': dict(entry=0x5854, mode=0x24, owner=0x22, send_vector=None),
}


class Di2Service(CPU):
    """Retain actual firmware RAM across modeled radio disconnects."""
    def __init__(self, path):
        path = Path(path)
        if path.name not in PROFILES:
            raise ValueError('no Di2 service profile for this filename')
        with path.open('rb') as stream:
            raw = stream.read(MAX_BYTES + 1)
        metadata = next(meta for meta in IMAGES if meta['filename'] == path.name)
        self.assessment = assess_image(metadata, raw)
        super().__init__(raw)
        self.profile = PROFILES[path.name]
        self.packets = []
        self.suspended = False
        vector = self.profile['send_vector']
        self.send_entry = (unpack_pc(self.read(0x4080 + vector * 4, 3))
                           if vector is not None else None)

    @property
    def connected(self):
        return self.read(self.profile['mode']) != 0

    @property
    def owner(self):
        return self.read(self.profile['owner'])

    def step(self):
        if self.pc == self.send_entry:
            # Recovered JSRV4 ABI: D0=destination, A0 points at a two-byte
            # length followed by the packet produced by original firmware.
            # Delivery/queue allocation are modeled, never claimed executed.
            pointer = self.a[0]
            length = self.read(pointer, 2)
            if not 2 <= length <= 9 or not 0 <= self.d[0] <= 63:
                raise RuntimeError('unmodeled Di2 send-service arguments')
            payload = bytes(self.read(pointer + 2 + i) for i in range(length))
            self.packets.append(dict(target=self.d[0], payload=payload.hex()))
            self.d[0] = 1  # Modeled successful queue allocation, not bus ACK.
            self.pc = unpack_pc(self.pop(3))
            return
        super().step()

    def link(self, mode, owner, budget=10000):
        if self.suspended:
            raise RuntimeError('service execution suspended; recreate before another call')
        if mode not in (0, 1, 4, 5) or not 0 <= owner < 32:
            raise ValueError('unsupported PC-link fixture')
        self.a[0] = 0x2000
        # Modeled receive ABI. Original accessor JSRV2 reads parameter two
        # and three at A0+4 and A0+5; no handler result is injected into RAM.
        self.ram[0x2000:0x2008] = bytes([0, 0, 0x32, 0x10, mode, owner, 0, 0])
        before = len(self.packets)
        expected = bytes([0x32, 0x12, 0, owner, 0xff, 0xff]).hex()
        def receipt():
            return mode == 0 and any(p['target'] == owner and p['payload'] == expected
                                     for p in self.packets[before:])
        result = self.run(self.profile['entry'] * 2, budget=budget, until=receipt)
        self.suspended = not result['returned']
        result.update(packets=[dict(p) for p in self.packets[before:]],
                      connected=self.connected, owner=self.owner,
                      receipt_boundary=result['paused'], full_topology_validated=False)
        return result
