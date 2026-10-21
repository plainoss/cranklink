"""Bounded instruction execution and explicitly modeled packet transport."""
import hashlib
from unicorn import UcError, UC_HOOK_CODE
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R4, UC_ARM_REG_R7, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC
from .cpu import DEFAULT_CPU, new_cpu

RETURN, INPUT = 0x4F000, 0x20009000


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


class Machine:
    def __init__(self, image, profile, budget=200000):
        require(hashlib.sha256(image).hexdigest() == profile['sha256'], 'firmware profile hash mismatch')
        require(64 <= len(image) <= 0x3F000, 'invalid mapped image size')
        self.profile, self.budget = profile, budget
        self.instructions = 0
        self._call_deadline = None
        self._peripheral_pending = False
        self._servicing_peripheral = False
        self.peripheral_services = 0
        peripheral_keys = ('peripheral_init', 'peripheral_poll', 'peripheral_busy_ram',
                           'peripheral_service', 'peripheral_channel')
        present = [key in profile for key in peripheral_keys]
        require(not any(present) or all(present), 'incomplete peripheral profile')
        if all(present):
            require(all(isinstance(profile[k], int) for k in peripheral_keys), 'invalid peripheral profile')
            require(all(0x10000 <= profile[k] < 0x10000 + len(image) and profile[k] % 2 == 0
                        for k in ('peripheral_init', 'peripheral_poll', 'peripheral_service')),
                    'peripheral entry outside profiled image')
            require(0x20000000 <= profile['peripheral_busy_ram'] < 0x20010000,
                    'peripheral state outside RAM')
            require(0 <= profile['peripheral_channel'] < 5, 'invalid peripheral channel')
        self.output, self.notifications, self.responses = [], [], []
        self.cpu_model = profile.get('cpu', DEFAULT_CPU)
        self.u = new_cpu(self.cpu_model)
        for address, size in ((0x10000, 0x40000), (0x20000000, 0x10000),
                              (0x40000000, 0x10000), (0xE000E000, 0x2000), (0xF0000000, 0x1000)):
            self.u.mem_map(address, size)
        self.u.mem_write(0x10000, image)
        self.u.hook_add(UC_HOOK_CODE, self.observe)

    def byte(self, address):
        return self.u.mem_read(address, 1)[0]

    def write_byte(self, address, value):
        self.u.mem_write(address, bytes([value]))

    def observe(self, u, address, size, opaque):
        self.instructions += 1
        p = self.profile
        if (not self._servicing_peripheral and address == p.get('peripheral_poll')
                and self.byte(p['peripheral_busy_ram'])):
            # Stop before this instruction. Service runs after emu_start returns,
            # preserving the interrupted CPU context and resuming the same PC.
            self._peripheral_pending = True
            u.emu_stop()

    def packet(self):
        pointer = self.u.reg_read(UC_ARM_REG_R1)
        length = self.byte(pointer)
        require(2 <= length <= 12, f'unsupported firmware packet length {length}')
        return dict(target=self.u.reg_read(UC_ARM_REG_R0), payload=bytes(self.u.mem_read(pointer + 2, length)).hex())

    def accept_packet(self):
        """Virtual bus accepts bytes; firmware continues and consumes pending flags.

        This models a queue operation, not successful delivery or a motor ACK.
        Packet loss is applied separately by the bus after this boundary.
        """
        self.output.append(self.packet())
        self.u.reg_write(UC_ARM_REG_R0, 1)
        self.u.reg_write(UC_ARM_REG_PC, self.u.reg_read(UC_ARM_REG_LR))

    def call(self, entry, r0=0, r1=0, r2=0, stop=None, registers=()):
        outer_call = self._call_deadline is None
        if outer_call:
            self._call_deadline = self.instructions + self.budget
        for reg, value in ((UC_ARM_REG_SP, 0x2000F000), (UC_ARM_REG_LR, RETURN | 1),
                           (UC_ARM_REG_R0, r0), (UC_ARM_REG_R1, r1), (UC_ARM_REG_R2, r2), *registers):
            self.u.reg_write(reg, value)
        handle = None
        if stop is not None:
            def boundary(u, address, size, opaque):
                if address == stop:
                    u.emu_stop()
            handle = self.u.hook_add(UC_HOOK_CODE, boundary)
        try:
            next_entry = entry
            while True:
                remaining = self._call_deadline - self.instructions
                require(remaining > 0, 'combined instruction budget exhausted')
                self._peripheral_pending = False
                self.u.emu_start(next_entry | 1, RETURN, count=remaining)
                pc = self.u.reg_read(UC_ARM_REG_PC)
                if pc == (stop or RETURN):
                    break
                require(self._peripheral_pending, 'instruction budget exhausted or unexpected stop')
                context = self.u.context_save()
                self._servicing_peripheral = True
                try:
                    self.call(self.profile['peripheral_service'], self.profile['peripheral_channel'],
                              registers=((UC_ARM_REG_SP, 0x2000D000),))
                    self.peripheral_services += 1
                finally:
                    self.u.context_restore(context)
                    self._servicing_peripheral = False
                next_entry = pc
        except (UcError, RuntimeError) as error:
            raise RuntimeError(f"{self.profile['id']} entry={entry:#x} pc={self.u.reg_read(UC_ARM_REG_PC):#x}: {error}") from error
        finally:
            if handle is not None:
                self.u.hook_del(handle)
            if outer_call:
                self._call_deadline = None
