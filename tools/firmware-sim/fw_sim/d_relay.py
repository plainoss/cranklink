"""Offline D relay experiment: stock radio codec and stock gateway dispatcher.

No radio SVC, physical bus, motor bootloader, or cycle timing is emulated.
The radio UART ring consumer and bus TX completion are explicit inputs.
"""
import hashlib
from pathlib import Path

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_THUMB, UC_MODE_MCLASS, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC

from .gateway import Gateway
from .machine import INPUT, RETURN, Machine, require
from .profiles import identify

RADIO_SHA256 = '33517f376e42857f52c8f5947438ad4a502c42d6f32539422358fb297863296f'
# Hashes are checked by identify before these entry points can execute.
D_PROFILES = {
    'sc-em800-4.4.0': dict(init=0x283d0, flush=0x283e8, done=0x28560, bus_queue=0x200032f8),
    'sc-em800-4.6.0': dict(init=0x279e0, flush=0x279f8, done=0x27b70, bus_queue=0x200032fc),
}


def fragment(payload, control=0x0b):
    require(len(payload) < 72, 'stock PCE payload limit')
    if len(payload) < 18:
        return [bytes([0, control]) + payload]
    count = (len(payload) + 18) // 18
    return [bytes([i * 32 | (0x80 if i + 1 < count else 0), control])
            + payload[i * 18:(i + 1) * 18] for i in range(count)]


def buffer_packet(packet, index, data):
    require(0 < len(data) <= 64, 'block must have 1..64 data bytes')
    block = data.ljust(64, b'\xff')
    return bytes([packet, 2, 0]) + block + bytes([sum(block) & 255]) + index.to_bytes(2, 'little')


class RadioPce:
    """Execute original Nordic 4.9.0 PCE codec and its real seven-entry UART ring."""
    def __init__(self, path):
        image = Path(path).read_bytes()
        require(hashlib.sha256(image).hexdigest() == RADIO_SHA256, 'radio image hash mismatch')
        self.u = Uc(UC_ARCH_ARM, UC_MODE_THUMB | UC_MODE_MCLASS)
        self.u.mem_map(0x10000, 0x40000)
        self.u.mem_map(0x20000000, 0x10000)
        self.u.mem_write(0x1d000, image)
        # UART is busy while ATT callbacks enqueue. A separate explicit
        # consumer drains records; DMA/physical UART are outside this model.
        self.u.mem_write(0x200026b4, b'\x02')
        self.instructions = 0
        self.u.hook_add(UC_HOOK_CODE, self._observe)

    def _observe(self, u, address, size, opaque):
        self.instructions += 1
        # Queue scheduling is outside the codec; do not execute an SVC.
        require(bytes(u.mem_read(address, 2))[1] != 0xdf, 'unexpected radio SVC')

    def write(self, value):
        require(2 <= len(value) <= 20, 'invalid ATT value')
        self.u.mem_write(INPUT, bytes(value))
        for register, value in ((UC_ARM_REG_SP, 0x2000f000), (UC_ARM_REG_LR, RETURN | 1),
                                (UC_ARM_REG_R0, INPUT), (UC_ARM_REG_R1, len(value))):
            self.u.reg_write(register, value)
        self.u.emu_start(0x1d2ad, RETURN, count=20000)
        require(self.u.reg_read(UC_ARM_REG_PC) == RETURN, 'radio codec instruction limit')

    def drain(self):
        """Explicit UART consumer; retain stock-produced record bytes and XOR."""
        records = []
        consumer = self.u.mem_read(0x200026b2, 1)[0]
        producer = self.u.mem_read(0x200026b3, 1)[0]
        while consumer != producer:
            pointer = 0x200032c0 + consumer * 77
            header = bytes(self.u.mem_read(pointer, 2))
            record = bytes(self.u.mem_read(pointer, header[1] + 3))
            checksum = 0
            for value in record:
                checksum ^= value
            require(checksum == 0, 'stock radio UART XOR mismatch')
            records.append(record)
            consumer = (consumer + 1) % 8
        self.u.mem_write(0x200026b2, bytes([consumer]))
        return records


class DGateway(Gateway):
    def __init__(self, path, consume_host=True):
        image, profile = identify(path, 'gateway')
        require(profile['id'] in D_PROFILES, 'D relay profile not recovered')
        self.consume_host = consume_host
        self.accepted = []
        self._bus_returns = {}
        super().__init__(image, profile)
        self.d_profile = D_PROFILES[profile['id']]
        entry = self.d_profile['init']
        load = int.from_bytes(self.u.mem_read(entry, 2), 'little')
        literal = ((entry + 4) & ~3) + (load & 255) * 4
        self.buffer_state = int.from_bytes(self.u.mem_read(literal, 4), 'little')
        self.call(self.d_profile['init'])
        self.dispatch(b'\x03\x60')
        self.call(self.profile['sched'])
        self.dispatch(b'\x04')
        require(self.notifications[-1] == '24e0', 'stock update/master mode not confirmed')
        self.dispatch(b'\x06\x0d')
        self.notifications.clear()

    def call(self, *args, **kwargs):
        Machine.call(self, *args, **kwargs)
        if self.consume_host and 'tx_consumer_ram' in self.profile:
            self.write_byte(self.profile['tx_consumer_ram'],
                            self.byte(self.profile['idx_ram'] + self.profile['idx_off']))

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
        if address in self._bus_returns:
            packet = self._bus_returns.pop(address)
            result = u.reg_read(UC_ARM_REG_R0)
            if result:
                self.accepted.append(dict(packet, tx_id=result))
        if address == self.profile['queue_hook']:
            self._bus_returns[u.reg_read(UC_ARM_REG_LR) & ~1] = self.output[-1].copy()

    def dispatch(self, logical):
        self.u.mem_write(INPUT, logical)
        self.call(self.profile['pce_dispatcher'], INPUT, len(logical))

    def deliver(self, records):
        for record in records:
            require(record[0] == 0x11 and record[2] == 0, 'unexpected radio PCE record')
            self.dispatch(record[3:-1])

    def complete_bus(self, packets):
        """Supply completion only for stock-accepted physical TX IDs."""
        # Explicit physical queue consumer: after all queued packets have been
        # delivered, advance the recovered read cursor and clear occupancy.
        state = self.d_profile['bus_queue']
        self.write_byte(state, self.byte(state + 1))
        self.write_byte(state + 2, 0)
        for packet in packets:
            self.call(self.d_profile['done'], packet['tx_id'])
        self.call(self.d_profile['flush'])

    def checksum_index(self):
        return int.from_bytes(self.u.mem_read(self.buffer_state + 0x9a, 2), 'little')


class BootPeer:
    """Declared behavior peer, not executable motor firmware or flash timing."""
    def __init__(self):
        self.data = bytearray()
        self.receipts = []

    def program(self, packets):
        require(len(packets) == 17, 'incomplete gateway block transmission')
        data = b''.join(bytes.fromhex(p['payload']) for p in packets[:16])
        checksum = bytes.fromhex(packets[16]['payload'])
        require(len(data) == 64 and checksum[:2] == bytes([0x25, sum(data) & 255]),
                'motor peer block checksum rejected')
        self.data.extend(data)
        self.receipts.append(b'\x88\x31' + (len(self.data) // 64).to_bytes(2, 'little'))
