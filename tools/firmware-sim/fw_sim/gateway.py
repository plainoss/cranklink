"""Stock gateway routing, discovery cache and FIFO reassembly."""
from .machine import Machine, INPUT, require, UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2


def fragments(payload):
    require(2 <= len(payload) <= 9, 'only 2..9 byte normal replies are supported')
    chunks = [payload[i:i + 3].ljust(3, b'\xff') for i in range(0, len(payload), 3)]
    return [bytes([(0x80 if i == 0 else 0) | (0x40 if i == len(chunks)-1 else 0) | (i+1)]) + chunk
            for i, chunk in enumerate(chunks)]


class Gateway(Machine):
    def __init__(self, image, profile, slot=13):
        super().__init__(image, profile)
        require(slot == 13, 'current recovered FIFO fixture supports only slot 13')
        self.slot = slot
        if 'peripheral_init' in profile:
            self.call(profile['peripheral_init'])
        self.write_byte(profile['pce_state_ram'] + 5, 19)
        self.call(profile['pce5'])
        require(self.notifications[-1] == profile['expected_pce5'], 'gateway revision mismatch')
        self.call(profile['pce3'], 0x20)
        self.call(profile['sched'])
        self.call(profile['pce4'])
        require(self.notifications[-1] == '24a0', 'master transition failed')

    def call(self, *args, **kwargs):
        super().call(*args, **kwargs)
        # The modeled host/radio peer consumes queued records after each bounded
        # firmware call. Otherwise the real TX ring correctly fills and refuses
        # later replies. This changes only its recovered consumer index.
        if 'tx_consumer_ram' in self.profile:
            self.write_byte(self.profile['tx_consumer_ram'],
                            self.byte(self.profile['idx_ram'] + self.profile['idx_off']))

    def observe(self, u, address, size, opaque):
        super().observe(u, address, size, opaque)
        p = self.profile
        if address == p['queue_hook']:
            packet = self.packet()
            packet['source'] = self.byte(0x4000C200)
            self.output.append(packet)
        elif address == p['notify_hook']:
            self.notifications.append(bytes(u.mem_read(u.reg_read(UC_ARM_REG_R0), u.reg_read(UC_ARM_REG_R1))).hex())
        elif address == p['free_resp_hook']:
            self.responses.append(dict(slot=u.reg_read(UC_ARM_REG_R0), payload=bytes(u.mem_read(
                u.reg_read(UC_ARM_REG_R1), u.reg_read(UC_ARM_REG_R2))).hex()))

    def start(self):
        count = len(self.output)
        self.call(self.profile['pce0c'], 1)
        return self.output[count:]

    def link_stop(self):
        count = len(self.output)
        self.call(self.profile['pce0c'], 0)
        return self.output[count:]

    def restore(self):
        self.call(self.profile['pce3'], 0)
        self.call(self.profile['sched'])
        self.call(self.profile['pce4'])
        require(self.notifications[-1] == '2493', 'normal/slave restoration failed')

    def send(self, payload):
        require(2 <= len(payload) <= 9, 'unsupported request length')
        count = len(self.output)
        self.u.mem_write(INPUT, payload)
        self.call(self.profile['free_forward'], self.slot, INPUT, len(payload))
        require(len(self.output) == count + 1, 'FREE command not enqueued')
        return self.output[-1]

    def receive(self, payload, mutation=None):
        return self.receive_fragments(fragments(payload), mutation)

    def receive_fragments(self, frames, mutation=None):
        p, index = self.profile, self.slot - 2
        frames = list(frames)
        if mutation == 'truncate': frames = frames[:-1]
        if mutation == 'reorder': frames.reverse()
        if mutation == 'corrupt-sequence': frames[-1] = b'\x4f' + frames[-1][1:]
        require(len(frames) <= 12 and all(len(frame) == 4 for frame in frames), 'invalid FIFO fixture')
        self.u.mem_write(p['ring_base'] + index * 192, b''.join(f.ljust(16, b'\x00') for f in frames))
        for key, value in (('fifo_head', 0), ('fifo_count', len(frames)), ('fifo_read', 0)):
            self.write_byte(p[key] + index, value)
        self.u.mem_write(p['node_state_ram'] + 0x30, (1 << self.slot).to_bytes(2, 'little'))
        record = p['rec_base'] + p['rec_off'] + 13 * self.byte(p['idx_ram'] + p['idx_off'])
        before = len(self.responses)
        self.call(p['rx_loop'], 2)
        if mutation is None:
            require(self.byte(p['fifo_count'] + index) == 0, 'FIFO did not drain')
        if len(self.responses) == before:
            return None
        require(len(self.responses) == before+1, 'multiple replies from one payload')
        actual = self.responses[-1]
        raw = bytes(self.u.mem_read(record, 3 + len(bytes.fromhex(actual['payload']))))
        require(raw == bytes([0x10, len(raw)-2, self.slot]) + bytes.fromhex(actual['payload']), f'host record mismatch actual={raw.hex()} reply={actual} record={record:#x}')
        return actual
