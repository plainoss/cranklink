"""Synthetic decoder cases plus numeric manufacturer worked examples, no firmware."""
import unittest
from fw_sim.mn101_decode import decode,DecodeError


def encoded(raw, half=0):
    # A leading unused nibble exercises instructions starting at PC.H=1.
    raw='0'*half+raw
    if len(raw)&1: raw+='0'
    return bytes(int(raw[i],16)|(int(raw[i+1],16)<<4) for i in range(0,len(raw),2))


class DecoderTests(unittest.TestCase):
    def instruction(self,raw,half=0,base=0x4000):
        return decode(encoded(raw,half),base,base*2+half)

    def test_primary_manual_absolute_jump_and_immediate_examples(self):
        # Panasonic II-42: 392C190 -> 1091C; A005 -> MOV 50,D0.
        jump=self.instruction('392c190')
        self.assertEqual(jump['op'],'jmp')
        self.assertEqual(jump['args']['target'],0x1091c*2)
        self.assertEqual(self.instruction('a005')['args'],dict(reg=0,imm=0x50))

    def test_primary_manual_relative_branch_example(self):
        # Panasonic II-42: byte address1091C, BRA8940 ->10920.
        got=self.instruction('8940',base=0x1091c)
        self.assertEqual(got['args']['target'],0x10920*2)

    def test_all_relative_call_width_and_half_forms(self):
        for raw,delta,size in [('10100',7,5),('11100',8,5),
                               ('121000',8,6),('131000',9,6),
                               ('11cff',-2,5),('13cfff',-1,6)]:
            for half in (0,1):
                with self.subTest(raw=raw,half=half):
                    got=self.instruction(raw,half)
                    self.assertEqual(got['op'],'jsr')
                    self.assertEqual(got['size'],size)
                    self.assertEqual(got['args']['target'],0x8000+half+delta)

    def test_relative_conditional_branches_forward_backward_and_half(self):
        for raw,condition,delta in [('900','eq',3),('913','eq',10),
                                     ('92f','ne',1),('8bdf','ne',1),
                                     ('9adff','eq',2)]:
            got=self.instruction(raw)
            self.assertEqual(got['op'],'branch')
            self.assertEqual(got['args'],dict(condition=condition,target=0x8000+delta))
        for raw,delta in [('eef',1),('ef0',4),('89df',1),('99dff',2)]:
            got=self.instruction(raw)
            self.assertEqual(got['op'],'jmp')
            self.assertEqual(got['args']['target'],0x8000+delta)

    def test_psw_moves_extension_and_masks(self):
        for raw,op,args in [('214','mov_psw_d',dict(reg=0)),
                            ('297','mov_d_psw',dict(reg=3)),
                            ('290','ext_d_dw',dict(src=0,reg=0)),
                            ('291','ext_d_dw',dict(src=2,reg=1)),
                            ('292ef','and_psw',dict(imm=0xfe)),
                            ('29301','or_psw',dict(imm=0x10))]:
            got=self.instruction(raw)
            self.assertEqual((got['op'],got['args']),(op,args))

    def test_indirect_calls_and_jumps_are_distinct(self):
        for mode,op,reg in [('0','jmp_a',0),('1','jsr_a',0),
                             ('2','jmp_a',1),('3','jsr_a',1)]:
            got=self.instruction('21'+mode)
            self.assertEqual((got['op'],got['args']),(op,dict(reg=reg)))

    def test_command_compares_and_memory_immediates(self):
        self.assertEqual(self.instruction('c0ca')['args'],dict(reg=0,imm=0xac))
        self.assertEqual(self.instruction('d8a8b0')['args'],
                         dict(reg=0,imm=0x8a,target=0x8000+17))
        self.assertEqual(self.instruction('15f6f61')['args'],dict(address=0xf6f,imm=0x16))
        self.assertEqual(self.instruction('2d677f0')['args'],dict(reg=0,address=0xf77))
        self.assertEqual(self.instruction('2fc0fff')['args'],dict(imm=-16))

    def test_arithmetic_immediates_width_and_signedness(self):
        cases=[('80f','add_imm_d',dict(reg=0,imm=-1)),
               ('0bff','add_imm_d',dict(reg=3,imm=255)),
               ('ec8','add_imm_a',dict(reg=0,imm=-8)),
               ('2ed08','add_imm_a',dict(reg=1,imm=-128)),
               ('2544321','add_imm_dw',dict(reg=0,imm=0x1234)),
               ('247ffff','sub_imm_a',dict(reg=1,imm=65535)),
               ('1c1f','and_imm_d',dict(reg=0,imm=0xf1)),
               ('1bf0','or_imm_d',dict(reg=3,imm=15)),
               ('2aaf0','sub_imm_d',dict(reg=2,imm=15)),
               ('3af08','xor_imm_d',dict(reg=3,imm=128))]
        for raw,op,args in cases:
            with self.subTest(raw=raw):
                for half in (0,1):
                    got=self.instruction(raw,half)
                    self.assertEqual((got['op'],got['args']),(op,args))
                    self.assertEqual(got['size'],len(raw))

    def test_register_arithmetic_bitfields_and_zero(self):
        for prefix,name in [('33','add'),('3b','addc'),('36','or'),
                            ('37','and'),('3a','xor'),('2a','sub'),('2b','subc')]:
            for mode in range(16):
                if prefix in ('3a','2a') and mode>>2==mode&3: continue
                got=self.instruction(prefix+format(mode,'x'))
                self.assertEqual(got['op'],name+'_d_d')
                self.assertEqual(got['args'],dict(src=mode>>2,dst=mode&3))
        for reg in range(4):
            got=self.instruction('8'+str(4+reg))
            self.assertEqual((got['op'],got['args']),('sub_d_d',dict(src=reg,dst=reg)))
        self.assertEqual(self.instruction('258')['args'],dict(src=0,dst=0))
        self.assertEqual(self.instruction('249')['args'],dict(src=0,dst=1))
        self.assertEqual(self.instruction('28f')['args'],dict(src=3,dst=1))
        self.assertEqual(self.instruction('298')['args'],dict(src=0,dst=0))

    def test_shift_and_bit_operations_all_registers(self):
        for prefix,name in [('22','not'),('23','asr')]:
            for mode in range(8,16):
                op=name if mode<12 else ('ror' if prefix=='22' else 'lsr')
                got=self.instruction(prefix+format(mode,'x'))
                self.assertEqual((got['op'],got['args']),(op+'_d',dict(reg=mode&3)))
        for bit in range(8):
            for prefix,clear in [('b',False),('b',True)]:
                mode=bit+8*clear
                got=self.instruction(prefix+format(mode,'x')+'21')
                self.assertEqual(got['op'],'bclr_abs' if clear else 'bset_abs')
                self.assertEqual(got['args'],dict(address=0x12,bit=bit))
        self.assertEqual(self.instruction('38f21')['args'],dict(address=0x12,bit=7))
        self.assertEqual(self.instruction('3c34321')['args'],dict(address=0x1234,bit=3))
        self.assertEqual(self.instruction('3d54321')['args'],dict(address=0x1234,bit=5))
        self.assertEqual(self.instruction('20ff0')['args'],dict(reg=3,imm=15))

    def test_extended_conditions_and_word_stack_offsets(self):
        for code,size in [('22',5),('23',6)]:
            for mode,condition in {1:'gt',2:'hi',3:'ls',4:'nc',5:'ns',6:'vc',7:'vs'}.items():
                raw=code+str(mode)+'0'*(size-3)
                got=self.instruction(raw,half=1)
                self.assertEqual(got['op'],'branch')
                self.assertEqual(got['args'],dict(condition=condition,target=0x8001+size))
        for raw,op,args in [('2e4a1','load_a_sp',dict(reg=0,offset=0x1a)),
                            ('2f7ff','store_dw_sp',dict(reg=1,offset=255)),
                            ('2e23412','load_dw_sp',dict(reg=0,offset=0x2143)),
                            ('2f1ffff','store_a_sp',dict(reg=1,offset=65535))]:
            got=self.instruction(raw)
            self.assertEqual((got['op'],got['args']),(op,args))
            self.assertEqual(got['size'],len(raw))

    def test_additional_word_byte_and_handy_address_moves(self):
        for raw,op,args in [('c521','load_a_abs',dict(reg=1,address=0x12)),
                           ('d6ff','store_dw_abs',dict(reg=0,address=255)),
                           ('d3','store_d_ha',dict(reg=3)),
                           ('95','store_dw_ha',dict(reg=1)),
                           ('97','store_a_ha',dict(reg=1)),
                           ('2ea','load_a_a',dict(reg=0,areg=1)),
                           ('2f9','store_a_a',dict(reg=1,areg=0)),
                           ('265ff','load_d_sp',dict(reg=1,offset=255)),
                           ('2704321','store_d_sp',dict(reg=0,offset=0x1234)),
                           ('26dffff','load_d_a',dict(reg=1,areg=1,offset=-1))]:
            got=self.instruction(raw)
            self.assertEqual((got['op'],got['args']),(op,args))
            self.assertEqual(got['size'],len(raw))

    def test_bit_and_compare_memory_branches(self):
        for prefix,kind in [('30','tbz_abs'),('31','tbnz_abs'),('34','tbz_io'),
                            ('35','tbnz_io'),('3e','tbz_abs'),('3f','tbnz_abs')]:
            for mode in (0,7,8,15):
                count=4 if prefix in ('3e','3f') else 2
                disp_count=3 if mode&8 else 2
                raw=prefix+format(mode,'x')+'1'+'0'*(count-1)+'0'*disp_count
                got=self.instruction(raw)
                self.assertEqual(got['op'],kind)
                self.assertEqual(got['args'],dict(address=1,bit=mode&7,target=0x8000+len(raw)))
        for prefix,count in [('2d',2),('3d',4)]:
            for mode in range(12,16):
                raw=prefix+format(mode,'x')+'1'+'0'*(count-1)+'ca'+'0'*(3 if mode&1 else 2)
                got=self.instruction(raw)
                self.assertEqual(got['op'],'cbne_abs' if mode&2 else 'cbeq_abs')
                self.assertEqual(got['args'],dict(address=1,imm=0xac,target=0x8000+len(raw)))
                self.assertEqual(got['size'],len(raw))
        self.assertEqual(self.instruction('3d84321ca')['args'],dict(address=0x1234,imm=0xac))

    def test_rep_decodes_encoded_count_and_does_not_expand_instruction(self):
        for count in range(8):
            for half in (0,1):
                got=self.instruction('21'+format(8+count,'x'),half)
                self.assertEqual((got['op'],got['args']),('rep',dict(count=count)))
                self.assertEqual(got['size'],3)
        # Observed writer sequence is represented here using synthetic bytes.
        data=encoded('21a250')
        first=decode(data,0x4000,0x8000)
        following=decode(data,0x4000,0x8000+first['size'])
        self.assertEqual(following['op'],'add_dw_dw')
        self.assertEqual(following['args'],dict(src=0,dst=0))

    def test_unsigned_multiply_divide_word_selector(self):
        for raw,op,reg in [('2fe','mulu',0),('2ff','mulu',1),('2ee','divu',0),('2ef','divu',1)]:
            for half in (0,1):
                got=self.instruction(raw,half)
                self.assertEqual((got['op'],got['args'],got['size']),(op,dict(reg=reg),3))

    def test_documented_far_20bit_control_transfer(self):
        self.assertEqual(self.instruction('3db0c998f')['args'],dict(target=0x6f899*2))
        self.assertEqual(self.instruction('3da1f4321')['args'],dict(target=0xf1234*2+1))
        self.assertEqual(self.instruction('3da000000')['op'],'jmp')
        self.assertEqual(self.instruction('3db000000')['op'],'jsr')
        with self.assertRaises(DecodeError): self.instruction('3db2c998f')
        with self.assertRaises(DecodeError): decode(encoded('3db0c99'),0x4000,0x8000)

    def test_unknown_and_truncated_fetch_fail(self):
        with self.assertRaises(DecodeError): self.instruction('2fd')
        with self.assertRaises(DecodeError): decode(encoded('13'),0x4000,0x8000)
        with self.assertRaises(DecodeError): decode(b'\x30',0x4000,0x7fff)


if __name__=='__main__': unittest.main()
