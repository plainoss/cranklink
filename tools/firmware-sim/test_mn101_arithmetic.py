"""Synthetic arithmetic, flag and control-flow checks for the MN101 subset."""
import unittest

from fw_sim.mn101_decode import decode
from fw_sim.mn101 import MN101Machine as CPU, BASE


def encoded(raw):
    raw=raw.replace(' ','')
    if len(raw)&1: raw+='0'
    return bytes(int(raw[i],16)|(int(raw[i+1],16)<<4) for i in range(0,len(raw),2))


class ArithmeticTests(unittest.TestCase):
    def test_exhaustive_byte_add_sub_with_carry_and_borrow(self):
        cpu=CPU(b'')
        for subtract in (False,True):
            for carry in (0,1):
                for lhs in range(256):
                    for rhs in range(256):
                        cpu.psw=0xa0|(carry<<1)
                        operation=('sub' if subtract else 'add')+'c'
                        got=cpu.arithmetic(lhs,rhs,8,operation)
                        full=lhs-rhs-carry if subtract else lhs+rhs+carry
                        expected=full&255
                        # Overflow depends on operand signs and the final sign;
                        # carry-in must not alter the original rhs sign.
                        overflow=((lhs^rhs)&(lhs^expected)&128 if subtract
                                  else (~(lhs^rhs))&(lhs^expected)&128)
                        cf=int(full<0 if subtract else full>255)
                        flags=(int(bool(overflow))<<3)|((expected>>7)<<2)|(cf<<1)|int(expected==0)
                        if got!=expected or cpu.psw!=0xa0|flags:
                            self.fail((operation,lhs,rhs,carry,got,cpu.psw,expected,flags))

    def test_plain_arithmetic_ignores_incoming_carry(self):
        cpu=CPU(b'')
        for operation,lhs,rhs,expected,flags in [
            ('add',255,1,0,3),('add',127,1,128,12),
            ('add',128,128,0,11),('sub',0,1,255,6),
            ('sub',128,1,127,8),('sub',127,255,128,14),
            ('add',0,-1,255,4),('and',128,255,128,4),
            ('or',0,0,0,1),('xor',255,255,0,1),
        ]:
            with self.subTest(operation=operation,lhs=lhs,rhs=rhs):
                cpu.psw=0xa2
                self.assertEqual(cpu.arithmetic(lhs,rhs,8,operation),expected)
                self.assertEqual(cpu.psw,0xa0|flags)

    def test_word_carry_borrow_and_signed_limits(self):
        cpu=CPU(b'')
        for operation,lhs,rhs,carry,expected,flags in [
            ('addc',0,65535,1,0,3),('subc',0,65535,1,0,3),
            ('addc',32767,0,1,32768,12),
            ('subc',32768,0,1,32767,8),
        ]:
            with self.subTest(operation=operation,lhs=lhs,rhs=rhs):
                cpu.psw=0x20|(carry<<1)
                self.assertEqual(cpu.arithmetic(lhs,rhs,16,operation),expected)
                self.assertEqual(cpu.psw,0x20|flags)

    def test_carry_instruction_dispatch_preserves_full_rhs(self):
        for raw in ['3b4','2b4']:
            cpu=CPU(encoded(raw))
            cpu.d[:2]=[0,255]
            cpu.psw=0x22
            cpu.pc=BASE*2
            cpu.step()
            self.assertEqual(cpu.d[0],0)
            self.assertEqual(cpu.psw,0x23)


class ExtendedExecutionTests(unittest.TestCase):
    def test_compare_flags_and_cbne_taken_and_not_taken(self):
        for value,flags,taken in [(1,1,False),(0,6,True),(128,8,True)]:
            cpu=CPU(encoded('d81010'))  # CBNE #1,D0,+1 nibble (H=1)
            cpu.d[0]=value
            cpu.psw=0x20
            cpu.pc=BASE*2
            instruction=decode(cpu.image,BASE,cpu.pc)
            cpu.step()
            self.assertEqual(cpu.psw,0x20|flags)
            self.assertEqual(cpu.d[0],value)
            self.assertEqual(cpu.pc,instruction['args']['target'] if taken else BASE*2+6)

    def test_ext_sign_extends_without_changing_flags(self):
        for raw,source,destination in [('290',0,0),('291',2,1)]:
            for byte,word in [(0,0),(127,127),(128,65408),(255,65535)]:
                cpu=CPU(encoded(raw))
                cpu.d[source]=byte
                cpu.psw=0x2f
                cpu.pc=BASE*2
                cpu.step()
                self.assertEqual(cpu.dw(destination),word)
                self.assertEqual(cpu.psw,0x2f)

    def test_bit_set_clear_flags_use_original_value(self):
        for raw,before,after,flags in [
            ('b720',0,128,1),('b720',128,128,4),
            ('bf20',128,0,4),('bf20',0,0,1),
        ]:
            cpu=CPU(encoded(raw), ram_start=0, ram_end=0x2800)
            cpu.write(2,before)
            cpu.psw=0x2f
            cpu.pc=BASE*2
            cpu.step()
            self.assertEqual(cpu.read(2),after)
            self.assertEqual(cpu.psw,0x20|flags)

    def test_relative_call_odd_return_and_indirect_call(self):
        for raw,callee in [('10100 01 a0ba 01',None),
                           ('211 01 0 a0ba 01',BASE+3)]:
            cpu=CPU(encoded(raw))
            # Register indirect calls always select the low nibble.
            if callee is not None: cpu.a[0]=callee
            result=cpu.run(BASE*2)
            self.assertTrue(result['returned'],result)
            self.assertEqual(cpu.d[0],0xab)
            self.assertEqual(result['initial']['sp'],result['final']['sp'])
            self.assertEqual(result['trace'][0]['op'],'jsr_a' if raw.startswith('211') else 'jsr')

    def test_rep_field_means_following_addw_executes_field_plus_one_times(self):
        for operand in range(8,16):
            with self.subTest(operand=operand):
                cpu=CPU(encoded('21'+format(operand,'x')+'25001'))
                cpu.set_dw(0,1)
                result=cpu.run(BASE*2)
                count=operand-8+1
                self.assertTrue(result['returned'],result)
                self.assertEqual(cpu.dw(0),1<<count)
                self.assertEqual(result['instructions'],count+2)
                self.assertEqual([ins['op'] for ins in result['trace']],
                                 ['rep']+['add_dw_dw']*count+['rts'])
                self.assertEqual(cpu.repeat_pending,0)

    def test_rep_execution_charges_each_repeated_instruction_to_budget(self):
        for budget in range(1,6):
            with self.subTest(budget=budget):
                cpu=CPU(encoded('21a25001'))
                cpu.set_dw(0,1)
                result=cpu.run(BASE*2,budget=budget)
                self.assertEqual(result['instructions'],budget)
                self.assertEqual(cpu.dw(0),1<<min(budget-1,3))
                self.assertEqual(result['returned'],budget==5)
                if budget<5: self.assertIn('budget exhausted',result['error'])
                else: self.assertIsNone(result['error'])

    def test_rep_rejects_unsupported_following_operations_before_execution(self):
        for following in ('00','a0ba','01','21a'):
            with self.subTest(following=following):
                cpu=CPU(encoded('21a'+following))
                result=cpu.run(BASE*2)
                self.assertFalse(result['returned'])
                self.assertIn('unverified repeated instruction',result['error'])
                self.assertEqual(result['instructions'],1)
                self.assertEqual(cpu.d[0],0x12)

    def test_mulu_product_is_known_but_unverified_flags_cannot_be_consumed(self):
        for pair,raw in ((0,'2fe'),(1,'2ff')):
            for left,right in ((0,0),(1,1),(255,255),(255,2),(128,128)):
                with self.subTest(pair=pair,left=left,right=right):
                    cpu=CPU(encoded(raw))
                    cpu.d[pair*2:pair*2+2]=[left,right]
                    cpu.pc=BASE*2
                    cpu.step()
                    self.assertEqual(cpu.dw(pair),left*right)
                    self.assertEqual(cpu.unknown_flags,7)
                    self.assertEqual(cpu.psw&8,0)
        for consumer in ('214','900','8c00','8e00','3b4','2b4','22c',
                         '208214','290214'):
            with self.subTest(consumer=consumer):
                cpu=CPU(encoded('2fe'+consumer+'01'))
                result=cpu.run(BASE*2)
                self.assertFalse(result['returned'])
                self.assertIn('unverified MULU',result['error'])

    def test_mulu_flags_must_be_redefined_or_explicitly_masked_before_use(self):
        for redefine in ('c000','84','2928f','29370','294'):
            with self.subTest(redefine=redefine):
                cpu=CPU(encoded('2fe'+redefine+'21401'))
                result=cpu.run(BASE*2)
                self.assertTrue(result['returned'],result)
                self.assertEqual(cpu.unknown_flags,0)
        # VF is established as zero by MULU, so a VF-only branch is safe.
        cpu=CPU(encoded('2fe2260001'))
        result=cpu.run(BASE*2)
        self.assertTrue(result['returned'],result)
        self.assertEqual(cpu.unknown_flags,7)

    def test_extended_ram_requires_explicit_mapping_and_preserves_boundaries(self):
        default=CPU(b'')
        for address in (0,0xfff,0x2000,0x27ff,0x2800,0x3000):
            with self.subTest(default_address=address):
                with self.assertRaises(RuntimeError): default.read(address)
                with self.assertRaises(RuntimeError): default.write(address,1)
        extended=CPU(b'',ram_start=0,ram_end=0x2800)
        for address in (0,0xfff,0x1000,0x2000,0x27ff):
            with self.subTest(extended_address=address):
                extended.write(address,0xab)
                self.assertEqual(extended.read(address),0xab)
        for address,width in ((0x27ff,2),(0x2800,1),(0x3000,1)):
            with self.subTest(address=address,width=width):
                with self.assertRaises(RuntimeError): extended.read(address,width)
                with self.assertRaises(RuntimeError): extended.write(address,1,width)
        for start,end in ((-1,0x2000),(0,0),(0x2000,0x1000),(0,0x2801)):
            with self.subTest(start=start,end=end):
                with self.assertRaises(ValueError): CPU(b'',ram_start=start,ram_end=end)


if __name__=='__main__': unittest.main()
