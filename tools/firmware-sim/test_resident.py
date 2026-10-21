"""Firmware-free boundary tests plus opt-in acquired-resident regressions."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

from unicorn.arm_const import UC_ARM_REG_LR, UC_ARM_REG_PC, UC_ARM_REG_R0

from fw_sim.cpu import CPU_MODELS, new_cpu
from fw_sim.resident import (FIFO, FIFO_CAPACITY, FIFO_COUNT, FIFO_HEAD, IMAGE_SIZE,
                             INIT, RETURN, SHA256, SPI_DATA, SPI_GATE, UART_STATUS,
                             UART_TX, Resident, key_frame, load_image, run_regression)


class CpuModelTests(unittest.TestCase):
    def test_models_are_selected_explicitly_before_execution(self):
        for name, value in CPU_MODELS.items():
            with self.subTest(cpu=name), patch('fw_sim.cpu.Uc') as constructor:
                engine = new_cpu(name)
                engine.ctl_set_cpu_model.assert_called_once_with(value)
                self.assertIs(engine, constructor.return_value)

    def test_unknown_model_rejected_before_creating_engine(self):
        with patch('fw_sim.cpu.Uc') as constructor:
            with self.assertRaisesRegex(RuntimeError, 'unsupported execution CPU'):
                new_cpu('not-a-cpu')
            constructor.assert_not_called()


class ResidentInputTests(unittest.TestCase):
    def test_size_and_hash_fail_closed_before_emulator_creation(self):
        with tempfile.TemporaryDirectory() as tmp, patch('fw_sim.resident.new_cpu') as engine:
            path = Path(tmp) / 'resident.bin'
            for length, message in ((0, 'exactly 65536'), (IMAGE_SIZE - 1, 'exactly 65536'),
                                    (IMAGE_SIZE + 1, 'exactly 65536'),
                                    (IMAGE_SIZE, 'unsupported resident SHA-256')):
                with self.subTest(length=length):
                    path.write_bytes(b'\x00' * length)
                    with self.assertRaisesRegex(RuntimeError, message):
                        Resident(path)
            engine.assert_not_called()

    def test_read_is_bounded_before_hashing(self):
        stream = Mock()
        stream.read.return_value = b'\x00' * (IMAGE_SIZE + 1)
        stream.__enter__ = Mock(return_value=stream)
        stream.__exit__ = Mock(return_value=False)
        with patch('pathlib.Path.open', return_value=stream):
            with self.assertRaisesRegex(RuntimeError, 'exactly 65536'):
                load_image('oversized.bin')
        stream.read.assert_called_once_with(IMAGE_SIZE + 1)

    def test_five_key_frames_have_real_payloads_and_checksum(self):
        self.assertEqual(key_frame(1).hex(), 'c80131417b00000000000000000000000000')
        self.assertEqual(key_frame(5).hex(), 'c8053383c300000000000000000000000000')
        for key in range(1, 6):
            frame = key_frame(key)
            self.assertEqual(len(frame), 18)
            self.assertEqual(frame[4], ((frame[0] & 63) + sum(frame[1:4])) & 255)
        for key in (0, 6, -1, '1'):
            with self.subTest(key=key), self.assertRaisesRegex(RuntimeError, 'key must be'):
                key_frame(key)


class ResidentBoundaryTests(unittest.TestCase):
    def machine(self, budget=100):
        # Synthetic image supplies only a Thumb return at the initializer;
        # production still requires the complete exact image hash.
        image = bytearray(IMAGE_SIZE)
        image[INIT:INIT + 2] = b'\x70\x47'
        with patch('fw_sim.resident.load_image', return_value=bytes(image)):
            return Resident('synthetic-only', budget=budget)

    def test_fifo_wrap_and_full_rejection_preserve_existing_data(self):
        machine = self.machine()
        machine.put(FIFO_HEAD, FIFO_CAPACITY - 1)
        machine.ingress(1)
        machine.ingress(2)
        self.assertEqual(bytes(machine.u.mem_read(FIFO + 18 * 19, 18)), key_frame(1))
        self.assertEqual(bytes(machine.u.mem_read(FIFO, 18)), key_frame(2))
        for _ in range(FIFO_CAPACITY - 2):
            machine.ingress(3)
        before = bytes(machine.u.mem_read(FIFO, FIFO_CAPACITY * 18))
        with self.assertRaisesRegex(RuntimeError, 'FIFO full or invalid'):
            machine.ingress(4)
        self.assertEqual(bytes(machine.u.mem_read(FIFO, FIFO_CAPACITY * 18)), before)
        self.assertEqual(machine.byte(FIFO_COUNT), FIFO_CAPACITY)

    def test_invalid_fifo_head_and_key_do_not_write(self):
        machine = self.machine()
        machine.put(FIFO_HEAD, FIFO_CAPACITY)
        with self.assertRaisesRegex(RuntimeError, 'FIFO full or invalid'):
            machine.ingress(1)
        machine.put(FIFO_HEAD, 0)
        with self.assertRaisesRegex(RuntimeError, 'key must be'):
            machine.ingress(6)
        self.assertEqual(machine.byte(FIFO_COUNT), 0)

    def test_spi_input_is_supplied_only_at_register_read_boundary(self):
        machine = self.machine()
        machine.set_spi(255)
        self.assertEqual(machine.byte(SPI_GATE), 0)
        # ldr r1,[pc,#4]; ldrb r0,[r1]; bx lr; nop; .word SPI_DATA
        machine.u.mem_write(0x100, bytes.fromhex('01490878704700bf') + SPI_DATA.to_bytes(4, 'little'))
        machine.call(0x100)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R0), 255)
        self.assertEqual(machine.spi_reads, 1)
        machine.set_spi(0, enabled=False)
        self.assertEqual(machine.byte(SPI_GATE), 2)
        machine.call(0x100)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R0), 0)
        self.assertEqual(machine.spi_reads, 2)

    def test_tx_observation_does_not_skip_the_firmware_body(self):
        machine = self.machine()
        packet = bytes(range(18))
        machine.u.mem_write(0x20000000, packet)
        # Body sets r0=42 before returning. An old TX stub would bypass it.
        machine.u.mem_write(UART_TX, bytes.fromhex('2a207047'))
        machine.u.reg_write(UC_ARM_REG_R0, 0x20000000)
        machine.u.reg_write(UC_ARM_REG_LR, RETURN | 1)
        machine.u.emu_start(UART_TX | 1, RETURN, count=10)
        self.assertEqual(machine.tx, [packet])
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_R0), 42)
        self.assertEqual(machine.u.reg_read(UC_ARM_REG_PC), RETURN)

    def test_uart_completion_is_explicit_and_busy_can_be_retained(self):
        machine = self.machine()
        machine.call = Mock()
        machine.u.mem_write(UART_STATUS, b'\x01\x00')
        machine.tick(complete_uart=False)
        self.assertEqual(bytes(machine.u.mem_read(UART_STATUS, 2)), b'\x01\x00')
        machine.tick(complete_uart=True)
        self.assertEqual(bytes(machine.u.mem_read(UART_STATUS, 2)), b'\x00\x00')

    def test_execution_is_bounded_and_does_not_map_application_code(self):
        machine = self.machine(budget=7)
        machine.u.mem_write(0x100, b'\xfe\xe7')
        with self.assertRaisesRegex(RuntimeError, 'instruction budget exhausted'):
            machine.call(0x100)
        with self.assertRaisesRegex(RuntimeError, 'resident entry=0x10000'):
            machine.call(0x10000)


@unittest.skipUnless(os.environ.get('CRANKLINK_RESIDENT_IMAGE'),
                     'set CRANKLINK_RESIDENT_IMAGE to the exact local acquired resident dump')
class AcquiredResidentTests(unittest.TestCase):
    def test_starvation_escape_and_real_uart_completion(self):
        report = run_regression(os.environ['CRANKLINK_RESIDENT_IMAGE'])
        self.assertTrue(report['passed'])
        self.assertFalse(report['hardware_access'])
        self.assertEqual(report['resident_sha256'], SHA256)
        self.assertEqual(len(report['cases']), 5)
        self.assertEqual([case['state'] for case in report['cases']], [2, 1, 2, 5, 1])


if __name__ == '__main__':
    unittest.main()
