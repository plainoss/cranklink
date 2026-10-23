"""Synthetic instruction tests: no vendor images or hardware are required."""
import json
import os
from pathlib import Path
import unittest

from fw_sim.mn101 import BASE, STOP, CPU_PROFILES, REGION_GET, MN101Machine, pack_pc, probe, unpack_pc
from fw_sim.mn101_decode import DecodeError, decode


ENTRY = BASE * 2


def code(nibbles):
    """Encode instruction-order nibbles, low nibble first in each byte."""
    values = [int(n, 16) for n in nibbles.replace(" ", "")]
    if len(values) & 1:
        values.append(0)
    return bytes(values[i] | values[i + 1] << 4 for i in range(0, len(values), 2))


class MN101DecodeTests(unittest.TestCase):
    def test_low_first_fetch_and_odd_nibble_entry(self):
        image = code("f a0ba 01")
        ins = decode(image, BASE, ENTRY + 1)
        self.assertEqual((ins["op"], ins["size"], ins["args"]),
                         ("mov_imm_d", 4, {"reg": 0, "imm": 0xab}))
        self.assertEqual(ins["raw"], "a0ba")

    def test_immediate_word_sign_extension_and_address_zero_extension(self):
        for instruction, operation, expected in [
            ("0cff", "mov_imm_dw", -1),
            ("0eff", "mov_imm_a", 255),
            ("ce4321", "mov_imm_dw", 0x1234),
            ("de8765", "mov_imm_a", 0x5678),
            ("fdf", "add_sp", -1),
            ("fc08", "add_sp", -128),
        ]:
            with self.subTest(instruction=instruction):
                ins = decode(code(instruction), BASE, ENTRY)
                self.assertEqual(ins["op"], operation)
                self.assertEqual(ins["args"]["imm"], expected)

    def test_absolute_call_has_nibble_h_and_upper_address_bits(self):
        ins = decode(code("39f4321"), BASE, ENTRY)
        self.assertEqual(ins["op"], "jsr")
        self.assertEqual(ins["size"], 7)
        self.assertEqual(ins["args"]["target"], 0x31234 * 2 + 1)

    def test_far_control_transfer_decodes_twenty_address_bits_and_h(self):
        for bank in range(16):
            for half in (0,1):
                for operation,mode in [('jmp','a'),('jsr','b')]:
                    raw='3d'+mode+format(bank>>3,'x')+format(((bank&7)<<1)|half,'x')+'4321'
                    ins=decode(code(raw),BASE,ENTRY)
                    self.assertEqual(ins['op'],operation)
                    self.assertEqual(ins['size'],9)
                    self.assertEqual(ins['args']['target'],((bank<<16)|0x1234)*2+half)
        with self.assertRaises(DecodeError): decode(code('3db200000'),BASE,ENTRY)

    def test_unknown_unmapped_and_truncated_code_trap(self):
        for image, pc in [(code("bb"), ENTRY), (b"", ENTRY),
                          (code("ce"), ENTRY), (code("00"), ENTRY - 1)]:
            with self.subTest(image=image, pc=pc):
                with self.assertRaises(DecodeError):
                    decode(image, BASE, pc)


class MN101ExecutionTests(unittest.TestCase):
    def test_memory_fault_after_pc_reaches_stop_is_not_a_return(self):
        from fw_sim.mn101 import CPU
        # MOV #40,(ioF4) advances to the synthetic return sentinel before its
        # unmapped peripheral write fails. PC equality alone is insufficient.
        image = bytearray(STOP // 2 - BASE)
        image[-3:] = code("024f04")
        for machine in (MN101Machine, CPU):
            with self.subTest(machine=machine.__name__):
                cpu = machine(bytes(image))
                result = cpu.run(STOP - 6)
                self.assertEqual(cpu.pc, STOP)
                self.assertEqual(result["instructions"], 1)
                self.assertIn("unmodeled memory write 0x3ff4", result["error"])
                self.assertFalse(result["returned"])
                self.assertFalse(result["paused"])

    def test_packed_pc_round_trip_including_h_and_reserved_bits(self):
        for pc in [0, 1, 0x1ffff, 0x20000, 0x40000, 0x7fffe, 0x7ffff]:
            with self.subTest(pc=pc):
                self.assertEqual(unpack_pc(pack_pc(pc)), pc)
                self.assertEqual(pack_pc(pc) & 0x7c0000, 0)
        self.assertEqual(pack_pc(0x2469), 0x801234)
        self.assertEqual(unpack_pc(0x7c1234), 0x2468)

    def test_expanded_pc_round_trip_retains_twenty_byte_address_bits(self):
        for pc in (0,1,0x7fffe,0x7ffff,0x80000,0x80001,0x1ffffe,0x1fffff):
            with self.subTest(pc=pc):
                packed=pack_pc(pc)
                self.assertEqual(unpack_pc(packed,address_bits=20),pc)
                self.assertEqual(packed&0x700000,0)
        self.assertEqual(unpack_pc(0x8f1234,address_bits=20),0x1e2469)
        # The original C-family address width remains the default.
        self.assertEqual(unpack_pc(0x8f1234),0x62469)
        with self.assertRaises(ValueError): MN101Machine(b'',address_bits=19)

    def test_far_call_and_rts_execute_above_c_family_address_limit(self):
        target_byte=0x60000
        target=target_byte*2+1
        image=bytearray(target_byte-BASE+8)
        def put(pc,raw):
            for index,nibble in enumerate(raw):
                byte=(pc+index)//2-BASE
                shift=((pc+index)&1)*4
                image[byte]=(image[byte]&~(15<<shift))|(int(nibble,16)<<shift)
        put(ENTRY,'3db0d000001')  # far JSR 60000.H1; caller RTS at odd PC
        put(target,'a0ba01')
        machine=MN101Machine(bytes(image),address_bits=20)
        result=machine.run(ENTRY)
        self.assertTrue(result['returned'],result)
        self.assertEqual(machine.d[0],0xab)
        self.assertEqual(result['initial']['sp'],result['final']['sp'])
        self.assertEqual([ins['pc'] for ins in result['trace']],
                         [ENTRY,target,target+4,ENTRY+9])

    def test_expanded_rts_restores_high_bits_from_manual_frame(self):
        machine=MN101Machine(code('01'),address_bits=20)
        original_sp=machine.sp
        machine.push(0x8f1234,3)
        machine.pc=ENTRY
        machine.step()
        self.assertEqual(machine.pc,0x1e2469)
        self.assertEqual(machine.sp,original_sp)

    def test_rti_restores_exact_six_byte_frame(self):
        machine = MN101Machine(code("03"))
        initial_sp = machine.sp
        machine.push(0xbeef, 2)
        machine.push(0x801234, 3)
        machine.push(0x29, 1)
        self.assertEqual(machine.read(machine.sp, 6), 0xbeef80123429)
        machine.pc = ENTRY
        machine.step()
        self.assertEqual((machine.pc, machine.psw, machine.ha, machine.sp),
                         (0x2469, 0x29, 0xbeef, initial_sp))

    def test_interrupt_save_restore_wrapper_preserves_all_state(self):
        machine = MN101Machine(code("fc8f f40 f62 e62 e40 fc80 03"))
        result = machine.run(ENTRY, interrupt=True)
        self.assertTrue(result["returned"], result)
        self.assertEqual(result["instructions"], 7)
        self.assertEqual(result["initial"], result["final"])

    def test_push_pop_and_odd_instruction_boundaries(self):
        machine = MN101Machine(code("f8 16 fdf fd1 06 e8 01"))
        result = machine.run(ENTRY)
        self.assertTrue(result["returned"], result)
        self.assertEqual(result["initial"], result["final"])
        self.assertEqual(result["instructions"], 7)

    def test_immediate_and_register_moves(self):
        machine = MN101Machine(code("a0ba a1 ce4321 24d 2cf 01"))
        result = machine.run(ENTRY)
        self.assertTrue(result["returned"], result)
        self.assertEqual(machine.a[1], 0x1234)
        self.assertEqual(machine.dw(1), 0x1234)
        self.assertEqual(machine.dw(0), 0x1234)

    def test_absolute_jsr_and_rts_restore_odd_return_address(self):
        # Seven-nibble call, two-nibble caller RTS, odd-address callee.
        machine = MN101Machine(code("3994004 01 a0ba 01"))
        result = machine.run(ENTRY)
        self.assertTrue(result["returned"], result)
        self.assertEqual(machine.d[0], 0xab)
        self.assertEqual(machine.sp, result["initial"]["sp"])
        self.assertEqual([ins["pc"] for ins in result["trace"]],
                         [ENTRY, ENTRY + 9, ENTRY + 13, ENTRY + 7])

    def test_jsrv_reads_packed_pc_from_rom_table(self):
        image = bytearray(code("fe2 01") + bytes(0x100))
        image[0x88:0x8c] = (pack_pc(ENTRY + 0x120)).to_bytes(4, "little")
        image[0x90:0x93] = code("a0dc01")
        result = MN101Machine(bytes(image)).run(ENTRY)
        self.assertTrue(result["returned"], result)
        self.assertEqual(result["final"]["d"][0], 0xcd)

    def test_byte_and_word_memory_are_little_endian(self):
        machine = MN101Machine(code("58 49 f0 e1 01"))
        machine.a[0] = 0x1100
        machine.a[1] = 0x1120
        result = machine.run(ENTRY)
        self.assertTrue(result["returned"], result)
        self.assertEqual(machine.read(0x1100), 0x12)
        self.assertEqual(machine.d[1], 0x12)
        self.assertEqual(machine.read(0x1100, 2), 0x1212)
        self.assertEqual(machine.dw(1), 0x1212)
        machine.write(0x1131, 0xabcd, 2)
        self.assertEqual(machine.read(0x1131), 0xcd)
        self.assertEqual(machine.read(0x1132), 0xab)

    def test_unknown_mmio_rom_write_and_unmapped_code_fail_closed(self):
        for instruction, address, fragment in [
            ("220", 0x1100, "unsupported opcode"),
            ("6000", 0x1100, "IO operation"),
            ("58", BASE, "memory write"),
            ("48", 0x3000, "memory read"),
        ]:
            with self.subTest(instruction=instruction):
                machine = MN101Machine(code(instruction))
                machine.a[0] = address
                result = machine.run(ENTRY)
                self.assertFalse(result["returned"])
                self.assertIn(fragment, result["error"])
        result = MN101Machine(b"").run(ENTRY)
        self.assertFalse(result["returned"])
        self.assertIn("unmapped code", result["error"])

    def test_instruction_budget_is_enforced(self):
        # Absolute JMP back to this instruction; no implicit return.
        machine = MN101Machine(code("3900004"))
        result = machine.run(ENTRY, budget=3)
        self.assertFalse(result["returned"])
        self.assertEqual(result["instructions"], 3)
        self.assertIn("budget exhausted", result["error"])
        self.assertNotEqual(machine.pc, STOP)

    def test_memory_boundaries_reject_crossing_and_rom_mutation(self):
        machine = MN101Machine(b"\x12\x34")
        self.assertEqual(machine.read(BASE, 2), 0x3412)
        for address, width in [(0xfff, 1), (0x1fff, 2), (0x2000, 1),
                               (BASE + 1, 2)]:
            with self.subTest(address=address):
                with self.assertRaises(RuntimeError):
                    machine.read(address, width)
        for address in [0xfff, 0x2000, BASE]:
            with self.subTest(address=address):
                with self.assertRaises(RuntimeError):
                    machine.write(address, 0)


@unittest.skipUnless(os.environ.get("CRANKLINK_MN101_FIXTURES"),
                     "set CRANKLINK_MN101_FIXTURES for stock CPU instruction checks")
class MN101StockExecutionTests(unittest.TestCase):
    def test_pinned_images_return_from_both_isolated_interrupt_probes(self):
        manifest = Path(os.environ["CRANKLINK_MN101_FIXTURES"]).resolve()
        entries = json.loads(manifest.read_text())
        self.assertIsInstance(entries, dict, "manifest must be a profile-id:path object")
        self.assertTrue(entries, "manifest must contain at least one fixture")
        profiles = {profile[0]: profile[1] for profile in CPU_PROFILES}
        for profile_id, path in entries.items():
            with self.subTest(profile=profile_id):
                self.assertIn(profile_id, profiles, "unknown CPU profile ID")
                self.assertIsInstance(path, str, "fixture path must be a string")
                self.assertTrue(path, "fixture path must not be empty")
                image_path = (manifest.parent / path).resolve()
                # probe enforces the exact image hash; missing files raise.
                result = probe(image_path)
                self.assertEqual(result["profile"], profile_id)
                self.assertEqual(result["sha256"], profiles[profile_id])
                self.assertTrue(result["passed"], result)
                self.assertEqual(len(result["cases"]), 5 if profile_id in REGION_GET else 2)
                self.assertEqual([case["instructions"] for case in result["cases"][:2]], [1, 7])
                for case in result["cases"]:
                    self.assertTrue(case["passed"], case)
                    self.assertTrue(case["returned"], case)
                    self.assertIsNone(case["error"])
                    if case['kind'] == 'interrupt-stack':
                        self.assertTrue(case["interrupt"])
                        self.assertEqual(case["initial"], case["final"])
                    else:
                        self.assertEqual(case['kind'], 'region-get')
                        self.assertFalse(case['interrupt'])
                        self.assertEqual(case['reply'], bytes([6, 0, 0x16, 0xae, 1, case['region'], 0, 0]).hex())
                        self.assertEqual(case['initial']['sp'], case['final']['sp'])
                if profile_id in REGION_GET:
                    self.assertEqual([case['region'] for case in result['cases'][2:]], [0, 1, 4])


if __name__ == "__main__":
    unittest.main()
