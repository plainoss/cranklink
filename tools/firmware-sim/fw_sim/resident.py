"""Bounded EP800 resident-code regression with explicit peripheral inputs.

The user supplies the acquired image. No flash/erase routines or device APIs run.
FIFO ingress, GPIO/SPI values and UART completion are modeled; the firmware's
initializer, FIFO/parser, key state machine, SPI polling and UART TX code execute.
"""
import hashlib
from pathlib import Path

from unicorn import UcError, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE
from unicorn.arm_const import (UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0,
                               UC_ARM_REG_SP)

from .cpu import DEFAULT_CPU, new_cpu
from .machine import require


PROFILE_ID = 'ep800-resident-5.0.6.1'
SHA256 = '59cc3fc007ca0b3595d8ba8e8207cc5257b05be84da8fd80278303d165488d9f'
IMAGE_SIZE = 0x10000
RETURN = 0x4F000
INIT, UART_SERVICE, SPI_FORWARD, UART_TX = 0x6CF4, 0x35D0, 0x3634, 0x61D8
FIFO, FIFO_COUNT, FIFO_HEAD, FIFO_CAPACITY = 0x20000D20, 0x200010B5, 0x200010B6, 20
KEY_STATE, TX_PENDING, SPI_PENDING = 0x200010C0, 0x200010C2, 0x200010B9
SPI_GATE, SPI_DATA, UART_STATUS = 0x40004012, 0x40006010, 0x4000C21C
KEY_VALUES = ((0x31, 0x41), (0x26, 0x53), (0x97, 0x93), (0x84, 0x62), (0x33, 0x83))


def load_image(path):
    # Reject size/hash before creating an emulator. Offsets are valid only for
    # this acquired image, even if another image reports the same version.
    with Path(path).open('rb') as stream:
        image = stream.read(IMAGE_SIZE + 1)
    require(len(image) == IMAGE_SIZE, 'resident image must be exactly 65536 bytes')
    require(hashlib.sha256(image).hexdigest() == SHA256,
            f'unsupported resident SHA-256; {PROFILE_ID} requires {SHA256}')
    return image


def key_frame(key):
    require(isinstance(key, int) and 1 <= key <= 5, 'key must be in 1..5')
    frame = bytearray((0xC8, key, *KEY_VALUES[key - 1]))
    frame.append(((frame[0] & 0x3F) + sum(frame[1:])) & 0xFF)
    return bytes(frame).ljust(18, b'\x00')


class Resident:
    def __init__(self, path, cpu=DEFAULT_CPU, budget=100000):
        image = load_image(path)
        require(isinstance(budget, int) and 1 <= budget <= 1000000,
                'resident instruction budget must be in 1..1000000')
        self.cpu_model, self.budget = cpu, budget
        self.u = new_cpu(cpu)
        for address, size in ((0, IMAGE_SIZE), (RETURN, 0x1000),
                              (0x20000000, 0x10000), (0x40000000, 0x10000),
                              (0xE000E000, 0x2000)):
            self.u.mem_map(address, size)
        self.u.mem_write(0, image)
        self.tx, self.instructions, self.spi_reads, self.mmio_writes = [], 0, 0, 0
        self.spi_value = 0
        self.u.hook_add(UC_HOOK_CODE, self.observe)
        self.u.hook_add(UC_HOOK_MEM_READ, self.read_mmio)
        self.u.hook_add(UC_HOOK_MEM_WRITE, self.write_mmio)
        self.call(INIT)
        self.set_spi(0, enabled=False)

    def byte(self, address):
        return self.u.mem_read(address, 1)[0]

    def put(self, address, value):
        self.u.mem_write(address, bytes((value,)))

    def observe(self, engine, address, size, opaque):
        self.instructions += 1
        if address == UART_TX:
            pointer = engine.reg_read(UC_ARM_REG_R0)
            # Observation only: let the real transmitter run, including busy
            # flag changes. Completion is supplied explicitly in tick().
            self.tx.append(bytes(engine.mem_read(pointer, 18)))

    def read_mmio(self, engine, access, address, size, value, opaque):
        if address == SPI_DATA:
            engine.mem_write(address, bytes((self.spi_value,)) * size)
            self.spi_reads += 1

    def write_mmio(self, engine, access, address, size, value, opaque):
        if 0x40000000 <= address < 0x40010000:
            self.mmio_writes += 1

    def call(self, entry):
        for reg, value in ((UC_ARM_REG_SP, 0x2000F000),
                           (UC_ARM_REG_LR, RETURN | 1), (UC_ARM_REG_R0, 0)):
            self.u.reg_write(reg, value)
        try:
            self.u.emu_start(entry | 1, RETURN, count=self.budget)
        except UcError as error:
            raise RuntimeError(f'resident entry={entry:#x} '
                               f'pc={self.u.reg_read(UC_ARM_REG_PC):#x}: {error}') from error
        require(self.u.reg_read(UC_ARM_REG_PC) == RETURN,
                f'resident instruction budget exhausted at entry={entry:#x}')

    def set_spi(self, value, enabled=True):
        require(isinstance(value, int) and 0 <= value <= 255, 'SPI value must be a byte')
        self.spi_value = value
        # In this profile, bit 1 high inhibits SPI polling. This is a modeled
        # register value, not a claimed physical pin or voltage mapping.
        self.put(SPI_GATE, 0 if enabled else 2)

    def ingress(self, key):
        frame = key_frame(key)
        count, head = self.byte(FIFO_COUNT), self.byte(FIFO_HEAD)
        require(count < FIFO_CAPACITY and head < FIFO_CAPACITY, 'resident RX FIFO full or invalid')
        self.u.mem_write(FIFO + 18 * ((head + count) % FIFO_CAPACITY), frame)
        self.put(FIFO_COUNT, count + 1)

    def tick(self, complete_uart=True):
        if complete_uart:
            # The real UART routines set/test this busy status. A finished
            # physical transmission is modeled here without bypassing TX code.
            self.u.mem_write(UART_STATUS, b'\x00\x00')
        before = len(self.tx)
        self.call(UART_SERVICE)
        self.call(SPI_FORWARD)
        return dict(state=self.byte(KEY_STATE), fifo_count=self.byte(FIFO_COUNT),
                    tx_pending=self.byte(TX_PENDING), spi_pending=self.byte(SPI_PENDING),
                    tx=[packet.hex() for packet in self.tx[before:]])

    def summary(self, name, steps):
        return dict(scenario=name, state=self.byte(KEY_STATE),
                    fifo_count=self.byte(FIFO_COUNT), steps=steps,
                    tx=[packet.hex() for packet in self.tx],
                    instructions=self.instructions, spi_reads=self.spi_reads,
                    mmio_writes=self.mmio_writes)


def run_regression(path, cpu=DEFAULT_CPU):
    cases = []
    for name, spi, enabled in (('spi-idle', 0, False), ('spi-zero', 0, True),
                               ('spi-ff', 255, True)):
        machine = Resident(path, cpu=cpu)
        machine.set_spi(spi, enabled)
        machine.ingress(1)
        steps = [machine.tick(), machine.tick()]
        machine.ingress(2)
        steps.extend(machine.tick() for _ in range(8))
        expected = 1 if name == 'spi-zero' else 2
        require(machine.byte(KEY_STATE) == expected, f'{name}: unexpected key state')
        require(machine.byte(FIFO_COUNT) == (1 if name == 'spi-zero' else 0),
                f'{name}: unexpected FIFO occupancy')
        if name == 'spi-zero':
            require(any(packet[1:4] == b'\x70\x00\x00' for packet in machine.tx),
                    'zero SPI did not produce transfer replies')
        else:
            require([packet[2] for packet in machine.tx if packet[1] == 0x11] == [1, 2],
                    f'{name}: missing actual UART ACK1/ACK2')
        cases.append(machine.summary(name, steps))

    # Keep the queued second key in place, then change only SPI input. This
    # regression tests escape from an existing stall, not just a clean start.
    machine = Resident(path, cpu=cpu)
    machine.set_spi(0)
    machine.ingress(1)
    steps = [machine.tick(), machine.tick()]
    machine.ingress(2)
    steps.extend(machine.tick() for _ in range(4))
    require(machine.byte(KEY_STATE) == 1 and machine.byte(FIFO_COUNT) == 1,
            'zero-to-ff did not start stalled')
    machine.set_spi(255)
    steps.extend(machine.tick() for _ in range(4))
    for key in (3, 4, 5):
        machine.ingress(key)
        steps.extend((machine.tick(), machine.tick()))
    require(machine.byte(KEY_STATE) == 5 and machine.byte(FIFO_COUNT) == 0,
            'zero-to-ff failed to reach state 5')
    require([packet[2] for packet in machine.tx if packet[1] == 0x11] == [1, 2, 3, 4, 5],
            'zero-to-ff did not transmit five ordered key ACKs')
    cases.append(machine.summary('spi-zero-to-ff-five-keys', steps))

    # Completion must be modeled: repeated service without it cannot magically
    # transmit a second frame or unblock the queue.
    machine = Resident(path, cpu=cpu)
    machine.set_spi(0)
    machine.ingress(1)
    steps = [machine.tick(complete_uart=False), machine.tick(complete_uart=False)]
    machine.ingress(2)
    steps.extend(machine.tick(complete_uart=False) for _ in range(8))
    require(len(machine.tx) == 1 and machine.byte(KEY_STATE) == 1,
            'UART busy boundary did not hold')
    cases.append(machine.summary('uart-never-completes', steps))

    return dict(schema_version=1, passed=True, hardware_access=False,
                profile=PROFILE_ID, resident_sha256=SHA256, cpu_model=cpu,
                modeled=['UART RX FIFO ingress', 'UART completion per service slice',
                         'SPI ready GPIO', 'constant SPI RX register value',
                         'other MMIO as initialized memory'],
                executed=['RAM initializer', 'cooperative UART/SPI service',
                          'FIFO parser', 'five-key state machine',
                          'SPI polling/checksum', 'UART TX/busy routines'],
                not_validated=['physical CPU/ISA', 'reset or application boot',
                               'M firmware execution', 'interrupt/wire timing',
                               'physical pin mapping or pull-up voltage',
                               'flash/erase/reset operations'], cases=cases)
