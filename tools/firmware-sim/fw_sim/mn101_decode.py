"""Strict MN101 subset decoder from Panasonic 21449-041E XIX-25..30.

pc/base are linear nibble addresses/byte origin; fetch is low nibble first.
Unknown encodings and unmapped code trap. Decoding does not establish execution
coverage or a complete CPU profile.
"""
class DecodeError(RuntimeError):
    pass


def decode(data, base, pc):
    def nib(index):
        offset = (pc + index) // 2 - base
        if offset < 0 or offset >= len(data):
            raise DecodeError(f'unmapped code nibble {pc+index:x}')
        return (data[offset] >> (4*((pc+index)&1))) & 15
    def val(start, count):
        return sum(nib(start+i) << (4*i) for i in range(count))
    def signed(value, bits):
        return value - (1<<bits) if value & (1<<(bits-1)) else value
    def result(op, size, **args):
        return dict(pc=pc, op=op, args=args, size=size,
                    raw=''.join(format(nib(i),'x') for i in range(size)))
    first, second = nib(0), nib(1)
    code = first*16+second
    if code in (0,1,3):
        return result({0:'nop',1:'rts',3:'rti'}[code],2)
    if code == 0xfd:
        return result('add_sp',3,imm=signed(val(2,1),4))
    if code == 0xfc:
        return result('add_sp',4,imm=signed(val(2,2),8))
    if code in (0x16,0x17,0x06,0x07):
        return result('push_a' if code>=0x10 else 'pop_a',2,reg=second&1)
    if first in (0xf,0xe) and 8<=second<=11:
        return result('push_d' if first==15 else 'pop_d',2,reg=second&3)
    if first in (14,15) and 4<=second<=7:
        family = 'a' if second<6 else 'dw'
        return result(('store_' if first==15 else 'load_')+family+'_sp',3,
                      reg=second&1,offset=val(2,1))
    if first == 10:
        src, dst = second>>2, second&3
        if src == dst:
            return result('mov_imm_d',4,reg=dst,imm=val(2,2))
        return result('mov_d_d',2,src=src,dst=dst)
    if first == 0 and second>=12:
        return result('mov_imm_dw' if second<14 else 'mov_imm_a',4,
                      reg=second&1,imm=signed(val(2,2),8) if second<14 else val(2,2))
    if first in (12,13) and second>=14:
        return result('mov_imm_dw' if first==12 else 'mov_imm_a',6,
                      reg=second&1,imm=val(2,4))
    # Relative displacements address bytes; explicit H adds one nibble.
    if first == 1 and second <= 3:
        size = 5 if second < 2 else 6
        disp = signed(val(2,size-2),12 if size==5 else 16)
        return result('jsr',size,target=pc+size+2*disp+(second&1))
    if first == 9 and second <= 3:
        disp=signed(val(2,1),4)
        return result('branch',3,condition='eq' if second<2 else 'ne',
                      target=pc+3+2*disp+(second&1))
    if code in (0xee,0xef):
        return result('jmp',3,target=pc+3+2*signed(val(2,1),4)+(second&1))
    if first in (8,9) and second>=8:
        size=4 if first==8 else 5
        bits=7 if first==8 else 11
        operand=val(2,size-2)
        # The encoded displacement's low bit is H; the remaining bits are
        # the signed byte displacement. See XIX-27/28 d7/d11 ...H fields.
        target=pc+size+2*signed(operand>>1,bits)+(operand&1)
        conditions={8:'ge',9:'always',10:'eq',11:'ne',12:'cc',13:'cs',14:'lt',15:'le'}
        if second==9: return result('jmp',size,target=target)
        return result('branch',size,condition=conditions[second],target=target)
    if code in (0x21,0x29):
        mode=nib(2)
        if 4<=mode<=7:
            return result('mov_psw_d' if code==0x21 else 'mov_d_psw',3,reg=mode&3)
        if code==0x29 and mode<=1:
            return result('ext_d_dw',3,src=2*mode,reg=mode)
        if code==0x29 and mode in (2,3):
            return result('and_psw' if mode==2 else 'or_psw',5,imm=val(3,2))
        if code==0x21 and mode>=8:
            return result('rep',3,count=mode&7)
        if code==0x21 and mode in (0,2):
            return result('jmp_a',3,reg=(mode>>1)&1)
        if code==0x21 and mode in (1,3):
            return result('jsr_a',3,reg=(mode>>1)&1)
    if first==12 and second<4:
        return result('cmp_imm_d',4,reg=second,imm=val(2,2))
    if first in (12,13) and second in (12,13):
        return result('cmp_imm_dw' if first==12 else 'cmp_imm_a',6,
                      reg=second&1,imm=val(2,4))
    if first in (12,13) and 8<=second<=11:
        operand=val(4,2)
        return result('cbne' if first==13 else 'cbeq',6,reg=second&3,
                      imm=val(2,2),target=pc+6+2*signed(operand>>1,7)+(operand&1))
    if code==0x32:
        mode=nib(2)
        return result('cmp_d_d',3,src=mode>>2,dst=mode&3)
    if code in (0x04,0x05):
        count=2 if code==4 else 3
        return result('cmp_imm_abs',4+count,address=val(2,count),imm=val(2+count,2))
    if code in (0x2c,0x2d) and 8<=nib(2)<=11:
        operand=val(5,3)
        return result('cbne' if code==0x2d else 'cbeq',8,reg=nib(2)&3,
                      imm=val(3,2),target=pc+8+2*signed(operand>>1,11)+(operand&1))
    if code==0x2c and nib(2) in (12,13):
        return result('mov_a_dw',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code==0x20 and 4<=nib(2)<=7:
        return result('cmp_a_a',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code==0x28 and 4<=nib(2)<=7:
        return result('cmp_dw_dw',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code==0x25 and nib(2)>=12:
        return result('cmp_dw_a',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code in (0x02,0x14,0x15):
        count=3 if code==0x15 else 2
        return result('store_imm_io' if code==2 else 'store_imm_abs',4+count,
                      address=val(2,count),imm=val(2+count,2))
    if code==0x3d and nib(2)==9:
        return result('store_imm_abs',9,address=val(3,4),imm=val(7,2))
    if code in (0x2c,0x2d) and nib(2)<8:
        mode=nib(2)
        kind='d' if mode<4 else 'a' if mode<6 else 'dw'
        return result(('load_' if code==0x2c else 'store_')+kind+'_abs',7,
                      reg=mode&3 if kind=='d' else mode&1,address=val(3,4))
    if code==0x2f and nib(2)==12:
        return result('add_sp',7,imm=signed(val(3,4),16))
    if code in (0x22,0x23) and 1<=nib(2)<=7:
        size=5 if code==0x22 else 6
        bits=7 if size==5 else 11
        operand=val(3,size-3)
        conditions={1:'gt',2:'hi',3:'ls',4:'nc',5:'ns',6:'vc',7:'vs'}
        return result('branch',size,condition=conditions[nib(2)],
                      target=pc+size+2*signed(operand>>1,bits)+(operand&1))
    if code in (0x2e,0x2f) and nib(2)<8:
        mode=nib(2); count=4 if mode<4 else 2
        kind='a' if mode in (0,1,4,5) else 'dw'
        return result(('load_' if code==0x2e else 'store_')+kind+'_sp',3+count,
                      reg=mode&1,offset=val(3,count))
    if first in (12,13) and 4<=second<=7:
        kind='a' if second<6 else 'dw'
        return result(('load_' if first==12 else 'store_')+kind+'_abs',4,
                      reg=second&1,address=val(2,2))
    if first==13 and second<4:
        return result('store_d_ha',2,reg=second)
    if first==9 and 4<=second<=7:
        return result('store_dw_ha' if second<6 else 'store_a_ha',2,reg=second&1)
    if code in (0x2e,0x2f) and 8<=nib(2)<=11:
        mode=nib(2)
        return result('load_a_a' if code==0x2e else 'store_a_a',3,
                      reg=mode&1,areg=(mode>>1)&1)
    if code in (0x26,0x27):
        mode=nib(2)
        if mode<8:
            count=4 if mode<4 else 2
            return result('load_d_sp' if code==0x26 else 'store_d_sp',3+count,
                          reg=mode&3,offset=val(3,count))
        return result('load_d_a' if code==0x26 else 'store_d_a',7,
                      reg=mode&3,areg=(mode>>2)&1,offset=signed(val(3,4),16))
    if code in (0x30,0x31,0x34,0x35,0x3e,0x3f):
        mode=nib(2); count=4 if code>=0x3e else 2
        disp_count=3 if mode&8 else 2
        size=3+count+disp_count
        operand=val(3+count,disp_count)
        op='tbnz' if code in (0x31,0x35,0x3f) else 'tbz'
        source='io' if code in (0x34,0x35) else 'abs'
        return result(op+'_'+source,size,address=val(3,count),bit=mode&7,
                      target=pc+size+2*signed(operand>>1,11 if mode&8 else 7)+(operand&1))
    if code in (0x2d,0x3d) and nib(2)>=12:
        mode=nib(2); count=2 if code==0x2d else 4
        disp_count=3 if mode&1 else 2
        size=5+count+disp_count
        operand=val(5+count,disp_count)
        return result('cbne_abs' if mode&2 else 'cbeq_abs',size,
                      address=val(3,count),imm=val(3+count,2),
                      target=pc+size+2*signed(operand>>1,11 if mode&1 else 7)+(operand&1))
    if code==0x3d and nib(2)==8:
        return result('cmp_imm_abs',9,address=val(3,4),imm=val(7,2))
    if code==0x2f and nib(2) in (14,15):
        return result('mulu',3,reg=nib(2)&1)
    if code==0x2e and nib(2) in (14,15):
        return result('divu',3,reg=nib(2)&1)
    # Arithmetic/logical forms from manufacturer XIX-26. Carry-in handling
    # and flag effects belong to the execution core, not this decoder.
    if first==8 and second<4:
        return result('add_imm_d',3,reg=second,imm=signed(val(2,1),4))
    if first==8 and 4<=second<=7:
        return result('sub_d_d',2,src=second&3,dst=second&3)
    if first==0 and 8<=second<=11:
        return result('add_imm_d',4,reg=second&3,imm=val(2,2))
    if first==1 and second>=8:
        return result('or_imm_d' if second<12 else 'and_imm_d',4,
                      reg=second&3,imm=val(2,2))
    if code in (0x33,0x3b,0x36,0x37,0x3a,0x2a,0x2b):
        mode=nib(2); src,dst=mode>>2,mode&3
        name={0x33:'add',0x3b:'addc',0x36:'or',0x37:'and',0x3a:'xor',0x2a:'sub',0x2b:'subc'}[code]
        if code in (0x3a,0x2a) and src==dst:
            return result(name+'_imm_d',5,reg=dst,imm=val(3,2))
        return result(name+'_d_d',3,src=src,dst=dst)
    if first==14 and second in (12,13):
        return result('add_imm_a',3,reg=second&1,imm=signed(val(2,1),4))
    if code==0x2e and nib(2) in (12,13):
        return result('add_imm_a',5,reg=nib(2)&1,imm=signed(val(3,2),8))
    if code in (0x24,0x25):
        mode=nib(2); name='sub' if code==0x24 else 'add'
        if mode<=3:
            return result(name+'_dw_dw',3,src=(mode>>1)&1,dst=mode&1)
        if 4<=mode<=7:
            return result(name+'_imm_dw' if mode<6 else name+'_imm_a',7,
                          reg=mode&1,imm=val(3,4))
        if 8<=mode<=11:
            return result(name+'_dw_a',3,src=(mode>>1)&1,dst=mode&1)
    if code in (0x28,0x29) and nib(2)>=8:
        mode=nib(2)
        return result('adduw_d_a' if code==0x28 else 'addsw_d_a',3,
                      src=mode&3,dst=(mode>>2)&1)
    if code in (0x22,0x23) and nib(2)>=8:
        mode=nib(2)
        name=('not_d' if mode<12 else 'ror_d') if code==0x22 else ('asr_d' if mode<12 else 'lsr_d')
        return result(name,3,reg=mode&3)
    # Bit operations preserve the old bit for flags in the execution core.
    if first==11:
        return result('bset_abs' if second<8 else 'bclr_abs',4,
                      address=val(2,2),bit=second&7)
    if code in (0x38,0x3c):
        mode=nib(2); count=2 if code==0x38 else 4
        return result(('bset_' if mode<8 else 'bclr_')+('io' if code==0x38 else 'abs'),3+count,
                      address=val(3,count),bit=mode&7)
    if code==0x3d and nib(2)<8:
        return result('btst_abs',7,address=val(3,4),bit=nib(2))
    if code==0x20 and nib(2)>=12:
        return result('btst_imm_d',5,reg=nib(2)&3,imm=val(3,2))
    # MN101E/L far control transfer: manufacturer MN101L05 XX-9.
    # 3D[A/B] 000B bbbH <abs16> extends the byte address to 20 bits.
    if code==0x3d and nib(2) in (10,11):
        if nib(3)>1:
            raise DecodeError(f'reserved far branch bank bits at nibblePC {pc:x}')
        bank=(nib(3)<<3)|(nib(4)>>1)
        address=(bank<<16)|val(5,4)
        return result('jsr' if nib(2)==11 else 'jmp',9,
                      target=address*2+(nib(4)&1))
    if code == 0x39:
        mode=nib(2)
        address=val(3,4)+(((mode>>1)&3)<<16)
        return result('jsr' if mode&8 else 'jmp',7,target=address*2+(mode&1))
    if code == 0xfe:
        return result('jsrv',3,index=val(2,1),table_byte=0x4080+val(2,1)*4)
    if first in (6,7) and 4<=second<=7:
        return result('load_d_sp' if first==6 else 'store_d_sp',3,
                      reg=second&3,offset=val(2,1))
    if first in (4,5) and second>=8:
        return result('load_d_a' if first==4 else 'store_d_a',2,
                      areg=(second>>2)&1,reg=second&3)
    if first in (6,7) and second>=8:
        return result('load_d_a' if first==6 else 'store_d_a',4,
                      areg=(second>>2)&1,reg=second&3,offset=signed(val(2,2),8))
    if first in (4,5) and second<8:
        size=5 if second<4 else 4
        return result('load_d_abs' if first==4 else 'store_d_abs',size,
                      reg=second&3,address=val(2,size-2))
    if first in (6,7) and second<4:
        return result('load_d_io' if first==6 else 'store_d_io',4,
                      reg=second,address=val(2,2))
    if first in (14,15) and second<4:
        return result('load_dw_a' if first==14 else 'store_dw_a',2,
                      areg=(second>>1)&1,reg=second&1)
    if code == 0x20:
        mode=nib(2)
        if mode<=3:
            return result('mov_a_a',3,src=(mode>>1)&1,dst=mode&1)
        if mode in (8,9):
            return result('mov_sp_a',3,reg=mode&1)
        if mode in (10,11):
            return result('mov_a_sp',3,reg=mode&1)
    if code == 0x24 and nib(2)>=12:
        return result('mov_dw_a',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code == 0x2c and nib(2)>=12:
        return result('mov_a_dw',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    if code == 0x28 and nib(2)<=3:
        return result('mov_dw_dw',3,src=(nib(2)>>1)&1,dst=nib(2)&1)
    raise DecodeError(f'unsupported opcode {code:02x} at nibblePC {pc:x}')
