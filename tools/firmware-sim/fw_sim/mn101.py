"""Bounded MN101 execution over explicitly mapped synthetic RAM.

Unknown instructions, peripheral accesses and firmware calls are never ignored.
"""
import hashlib
from copy import deepcopy
from .mn101_decode import decode

BASE = 0x4000
STOP = 0x7fffe


def unpack_pc(value, address_bits=18):
    return (value & ((1 << address_bits) - 1)) * 2 + ((value >> 23) & 1)


def pack_pc(value):
    return (value // 2) | ((value & 1) << 23)


def unpack_pc_strict(value):
    if value < 0 or value & ~0x83ffff:
        raise RuntimeError("unverified packed PC width")
    return (value & 0x3ffff) * 2 + ((value >> 23) & 1)


def pack_pc_strict(value):
    if not 0 <= value < 0x80000:
        raise RuntimeError("unverified PC width")
    return (value // 2) | ((value & 1) << 23)


class MN101Machine:
    def __init__(self, image, ram_start=0x1000, ram_end=0x2000, address_bits=18):
        if not (0 <= ram_start < ram_end <= 0x2800):
            raise ValueError('invalid synthetic RAM interval')
        self.ram_start, self.ram_end = ram_start, ram_end
        if address_bits not in (18, 20): raise ValueError('unsupported program-address width')
        self.address_bits = address_bits
        self.image = image
        self.ram = bytearray(0x2800)
        self.d = [0x12, 0x34, 0x56, 0x78]
        self.a = [0x1234, 0x5678]
        self.sp = 0x1f00
        self.pc = 0
        self.psw = 0x25
        self.ha = 0x4567
        self.trace = []
        self.repeat_pending = 0
        self.repeat_start = None
        self.unknown_flags = 0

    def read(self, address, width=1):
        if self.ram_start <= address and address + width <= self.ram_end:
            return int.from_bytes(self.ram[address:address+width], 'little')
        if BASE <= address and address + width <= BASE + len(self.image):
            return int.from_bytes(self.image[address-BASE:address-BASE+width], 'little')
        raise RuntimeError(f'unmodeled memory read {address:#x}/{width}')

    def write(self, address, value, width=1):
        if not (self.ram_start <= address and address + width <= self.ram_end):
            raise RuntimeError(f'unmodeled memory write {address:#x}/{width}')
        self.ram[address:address+width] = (value & ((1 << (width*8))-1)).to_bytes(width,'little')

    def dw(self, reg):
        return self.d[2*reg] | self.d[2*reg+1] << 8

    def set_dw(self, reg, value):
        self.d[2*reg:2*reg+2] = [value & 255, (value >> 8) & 255]

    def push(self, value, width):
        self.sp -= width
        self.write(self.sp, value, width)

    def pop(self, width):
        value = self.read(self.sp, width)
        self.sp += width
        return value

    def compare(self, lhs, rhs, bits):
        self.unknown_flags = 0
        mask=(1<<bits)-1
        lhs &= mask
        rhs &= mask
        value=(lhs-rhs)&mask
        overflow=bool((lhs^rhs)&(lhs^value)&(1<<(bits-1)))
        self.psw=(self.psw&0xf0)|(int(overflow)<<3)|(int(bool(value&(1<<(bits-1))))<<2)|(int(lhs<rhs)<<1)|int(value==0)

    def arithmetic(self, lhs, rhs, bits, operation):
        if operation in ('addc', 'subc') and self.unknown_flags & 2:
            raise RuntimeError('unverified MULU carry flag consumed')
        self.unknown_flags = 0
        mask=(1<<bits)-1
        lhs &= mask
        rhs &= mask
        if operation in ('add','sub','addc','subc'):
            carry=(self.psw>>1)&1 if operation in ('addc','subc') else 0
            subtract=operation in ('sub','subc')
            full=lhs-rhs-carry if subtract else lhs+rhs+carry
            value=full&mask
            sign=1<<(bits-1)
            signed_lhs=lhs-(mask+1) if lhs&sign else lhs
            signed_rhs=rhs-(mask+1) if rhs&sign else rhs
            signed_full=(signed_lhs-signed_rhs-carry if subtract
                         else signed_lhs+signed_rhs+carry)
            overflow=not(-sign <= signed_full < sign)
            carry_out=full<0 if subtract else full>mask
            self.psw=(self.psw&0xf0)|(int(overflow)<<3)|(int(bool(value&sign))<<2)|(int(carry_out)<<1)|int(value==0)
            return value
        value=dict(and_=lambda:lhs&rhs,or_=lambda:lhs|rhs,xor_=lambda:lhs^rhs)[operation+'_']()&mask
        self.psw=(self.psw&0xf0)|(int(bool(value&(1<<(bits-1))))<<2)|int(value==0)
        return value

    def step(self):
        if self.repeat_pending:
            if self.repeat_start is None: self.repeat_start=self.pc
            instruction=decode(self.image,BASE,self.repeat_start)
            if instruction['op']!='add_dw_dw':
                raise RuntimeError('unverified repeated instruction '+instruction['op'])
            self.pc=self.repeat_start
            self._execute_instruction()
            self.repeat_pending-=1
            if self.repeat_pending: self.pc=self.repeat_start
            else: self.repeat_start=None
            return
        self._execute_instruction()

    def _execute_instruction(self):
        if not 0 <= self.pc < (1 << (self.address_bits+1)):
            raise RuntimeError('instruction address exceeds configured CPU width')
        ins = decode(self.image, BASE, self.pc)
        self.trace.append(ins)
        op, args = ins['op'], ins['args']
        self.pc += ins['size']
        reg = args.get('reg')
        if op == 'nop': return
        if op == 'rep':
            # The encoded field is the assembly repeat count minus one.
            self.repeat_pending=args['count']+1
            self.repeat_start=None
            return
        if op=='mulu':
            value=self.d[reg*2]*self.d[reg*2+1]
            self.set_dw(reg,value)
            self.psw=(self.psw&0xf0)|(int(bool(value&0x8000))<<2)|int(value==0)
            # Product and VF=0 are established. This subset does not assume
            # the variant-specific NZC effects; consumers must redefine them.
            self.unknown_flags = 7
            return
        if op == 'add_sp': self.sp = (self.sp + args['imm']) & 65535; return
        if op == 'mov_psw_d':
            if self.unknown_flags: raise RuntimeError('unverified MULU flags consumed')
            self.d[reg] = self.psw; return
        if op == 'mov_d_psw': self.psw = self.d[reg]; self.unknown_flags=0; return
        if op == 'and_psw': self.psw &= args['imm']; self.unknown_flags &= args['imm']; return
        if op == 'or_psw': self.psw |= args['imm']; self.unknown_flags &= ~args['imm']; return
        if op == 'ext_d_dw':
            value = self.d[args['src']]
            self.set_dw(reg, value if value<128 else value-256)
            return
        if op == 'branch':
            used={'eq':1,'ne':1,'ge':12,'lt':12,'le':13,'hi':3,'ls':3,
                  'gt':13,'cc':2,'cs':2,'nc':4,'ns':4,'vc':8,'vs':8,'always':0}[args['condition']]
            if self.unknown_flags & used: raise RuntimeError('unverified MULU flags consumed by branch')
            v,n,c,z=[bool(self.psw&(1<<i)) for i in (3,2,1,0)]
            conditions=dict(eq=z,ne=not z,ge=v==n,lt=v!=n,le=(v!=n or z),hi=not(c or z),ls=c or z,gt=not((v!=n) or z),cc=not c,cs=c,nc=not n,ns=n,vc=not v,vs=v,always=True)
            if conditions[args['condition']]: self.pc=args['target']
            return
        if op in ('jsr_a','jmp_a'):
            if op=='jsr_a': self.push(pack_pc(self.pc),3)
            self.pc=self.a[reg]*2
            return
        if op in ('cbne','cbeq'):
            self.compare(self.d[reg],args['imm'],8)
            if (self.d[reg]==args['imm']) == (op=='cbeq'): self.pc=args['target']
            return
        if op in ('cbne_abs','cbeq_abs'):
            value=self.read(args['address'])
            self.compare(value,args['imm'],8)
            if (value==args['imm']) == (op=='cbeq_abs'): self.pc=args['target']
            return
        if op.startswith('cmp_'):
            bits=8
            if op=='cmp_imm_d': lhs,rhs=self.d[reg],args['imm']
            elif op=='cmp_imm_dw': lhs,rhs,bits=self.dw(reg),args['imm'],16
            elif op=='cmp_imm_a': lhs,rhs,bits=self.a[reg],args['imm'],16
            elif op=='cmp_imm_abs': lhs,rhs=self.read(args['address']),args['imm']
            elif op=='cmp_d_d': lhs,rhs=self.d[args['dst']],self.d[args['src']]
            elif op=='cmp_a_a': lhs,rhs,bits=self.a[args['dst']],self.a[args['src']],16
            elif op=='cmp_dw_dw': lhs,rhs,bits=self.dw(args['dst']),self.dw(args['src']),16
            elif op=='cmp_dw_a': lhs,rhs,bits=self.a[args['dst']],self.dw(args['src']),16
            else: raise RuntimeError(op)
            self.compare(lhs,rhs,bits)
            return
        if op=='store_imm_abs': self.write(args['address'],args['imm']); return
        if op=='store_imm_io': self.write(0x3f00+args['address'],args['imm']); return
        if op.startswith(('bset_','bclr_','btst_')):
            if op=='btst_imm_d': self.arithmetic(self.d[reg],args['imm'],8,'and'); return
            if op.endswith('_io'): raise RuntimeError('unmodeled IO operation '+op)
            value=self.read(args['address'])
            mask=1<<args['bit']
            self.arithmetic(value,mask,8,'and')
            if op.startswith('bset_'): self.write(args['address'],value|mask)
            elif op.startswith('bclr_'): self.write(args['address'],value&~mask)
            return
        if op in ('not_d','asr_d','lsr_d','ror_d'):
            before=self.d[reg]
            carry=(self.psw>>1)&1
            if op=='ror_d' and self.unknown_flags&2: raise RuntimeError('unverified MULU carry flag consumed')
            if op=='not_d': self.d[reg]=self.arithmetic(before,255,8,'xor'); return
            value=(before>>1)|((before&128) if op=='asr_d' else carry<<7 if op=='ror_d' else 0)
            self.d[reg]=value
            n=(self.psw&4) if op=='asr_d' else 0 if op=='lsr_d' else int(bool(value&128))<<2
            self.psw=(self.psw&0xf0)|n|((before&1)<<1)|int(value==0)
            self.unknown_flags = self.unknown_flags&4 if op=='asr_d' else 0
            return
        if op.startswith(('add_','sub_','addc_','subc_','and_','or_','xor_')):
            operation,source,dest=op.split('_')
            dst=args.get('dst',reg)
            lhs=self.d[dst] if dest=='d' else self.dw(dst) if dest=='dw' else self.a[dst]
            rhs=args['imm'] if source=='imm' else self.d[args['src']] if source=='d' else self.dw(args['src'])
            value=self.arithmetic(lhs,rhs,8 if dest=='d' else 16,operation)
            if dest=='d': self.d[dst]=value
            elif dest=='dw': self.set_dw(dst,value)
            else: self.a[dst]=value
            return
        if op in ('adduw_d_a','addsw_d_a'):
            value=self.d[args['src']]
            if op=='addsw_d_a' and value&128: value-=256
            self.a[args['dst']]=self.arithmetic(self.a[args['dst']],value,16,'add')
            return
        if op in ('rts','rti'):
            if op == 'rti': self.psw = self.pop(1); self.unknown_flags=0
            self.pc = unpack_pc(self.pop(3), self.address_bits)
            if op == 'rti': self.ha = self.pop(2)
            return
        if op in ('jsr','jmp','jsrv'):
            target = unpack_pc(self.read(args['table_byte'],3), self.address_bits) if op == 'jsrv' else args['target']
            if op != 'jmp': self.push(pack_pc(self.pc),3)
            self.pc = target
            return
        if op.startswith('mov_imm_'):
            if op.endswith('_d'): self.d[reg] = args['imm'] & 255
            elif op.endswith('_dw'): self.set_dw(reg,args['imm'])
            elif op.endswith('_a'): self.a[reg] = args['imm'] & 65535
            else: raise RuntimeError(op)
            return
        if op.startswith('mov_'):
            src, dst = args.get('src'), args.get('dst')
            if op == 'mov_d_d': self.d[dst] = self.d[src]
            elif op == 'mov_a_a': self.a[dst] = self.a[src]
            elif op == 'mov_dw_dw': self.set_dw(dst,self.dw(src))
            elif op == 'mov_dw_a': self.a[dst] = self.dw(src)
            elif op == 'mov_a_dw': self.set_dw(dst,self.a[src])
            elif op == 'mov_sp_a': self.a[reg] = self.sp
            elif op == 'mov_a_sp': self.sp = self.a[reg]
            else: raise RuntimeError(op)
            return
        if op.startswith(('push_','pop_')):
            width = 2 if op.endswith('_a') else 1
            if op.startswith('push_'): self.push(self.a[reg] if width==2 else self.d[reg],width)
            elif width==2: self.a[reg] = self.pop(width)
            else: self.d[reg] = self.pop(width)
            return
        if op.startswith(('load_','store_')):
            direction, kind, source = op.split('_')
            width = 1 if kind=='d' else 2
            if source == 'sp': address = self.sp + args['offset']
            elif source == 'a': address = (self.a[args['areg']] + args.get('offset',0)) & 65535
            elif source == 'abs': address = args['address']
            else: raise RuntimeError(f'unmodeled IO operation {op}')
            if direction == 'load':
                value = self.read(address,width)
                if kind=='d': self.d[reg] = value
                elif kind=='dw': self.set_dw(reg,value)
                else: self.a[reg] = value
            else:
                self.write(address,self.d[reg] if kind=='d' else self.dw(reg) if kind=='dw' else self.a[reg],width)
            return
        raise RuntimeError(f'unimplemented decoded operation {op}')

    def run(self, entry, interrupt=False, budget=200, until=None):
        """Synthetic call frame; until pauses execution, never reports a return."""
        if not 0 < budget <= 100000:
            raise ValueError('instruction budget must be 1..100000')
        if not 0 <= entry < (1 << (self.address_bits + 1)):
            raise RuntimeError('unverified PC width')
        self.trace = []
        self.pc = entry
        initial = dict(d=self.d.copy(), a=self.a.copy(), sp=self.sp,
                       psw=self.psw, ha=self.ha)
        if interrupt:
            self.push(self.ha, 2)
            self.push(pack_pc(STOP), 3)
            self.push(self.psw, 1)
        else:
            self.push(pack_pc(STOP), 3)
        error, paused = None, False
        try:
            for _ in range(budget):
                if self.pc == STOP:
                    break
                if until is not None and until():
                    paused = True
                    break
                self.step()
            if self.pc != STOP and not paused:
                raise RuntimeError('instruction budget exhausted')
        except RuntimeError as exception:
            error = str(exception)
        return deepcopy(dict(entry=entry, interrupt=interrupt,
                    instructions=len(self.trace), returned=error is None and self.pc == STOP,
                    paused=paused, error=error, initial=initial,
                    final=dict(d=self.d, a=self.a, sp=self.sp,
                               psw=self.psw, ha=self.ha), trace=self.trace))


class CPU(MN101Machine):
    """Strict Di2 service subset; broader CPU profiles do not expand admission.

    The Di2 experiment intentionally retains its original instruction traps,
    18-bit program addresses, exact MMIO fixtures and receipt pause boundary.
    """
    def __init__(self, image):
        super().__init__(image, ram_start=0, ram_end=0x2800)
        self.psw = 0xa5
        self.fixtures = {}
        self.allowed_writes = []
        self.observed_writes = []

    def read(self, address, width=1):
        if all(address+i in self.fixtures for i in range(width)):
            return sum(self.fixtures[address+i] << (8*i) for i in range(width))
        return super().read(address, width)

    def write(self, address, value, width=1):
        event = (address, value, width)
        if self.allowed_writes and event == self.allowed_writes[0]:
            self.observed_writes.append(event)
            self.allowed_writes.pop(0)
            return
        super().write(address, value, width)

    def step(self):
        pack_pc_strict(self.pc)
        ins = decode(self.image, BASE, self.pc)
        op, args = ins['op'], ins['args']
        if op in ('rep', 'mulu', 'divu', 'asr_d'):
            self.trace.append(ins)
            self.pc += ins['size']
            raise RuntimeError('unverified instruction semantics: ' + op)
        if op in ('rts', 'rti'):
            unpack_pc_strict(self.read(self.sp + (op == 'rti'), 3))
        elif op == 'jsrv':
            unpack_pc_strict(self.read(args['table_byte'], 3))
        elif op in ('jsr', 'jmp'):
            pack_pc_strict(args['target'])
        super().step()


# These are isolated CPU probes, separate from the ARM packet profiles.
# Addresses are linear nibble PCs. Packet transport and persistent writes
# have not been established for these images.
CPU_PROFILES = [
    ("e6000-3.2.3", "5540b5712ff97924f9f8986640e90592f4790f7900fffc1be7217b0465d7efd1", 39116),
    ("e6001-3.4.7", "d98a82148881e09c7232ee2a46e7e514005140308bd819f9943b5768d4f83175", 39406),
    ("e6002-3.3.0-board1", "a9b861763c69a38ee78f8e00d3a44b9d563fe6cb1289bdc0049441e8ffa92c93", 42390),
    ("e6002-3.3.0-board2", "74808703fac40a8788bb56f9e5990ebd00c0036c6799e9e1378ee33c503f339c", 42390),
    ("e6002-3.4.0-board1", "e1df0249406f90157c7f5997f94014979571a4dd35d19e7e060993a4fc1dd330", 43598),
    ("e6002-3.4.0-board2", "4ad2ace5af461868a8b46b370648f36b04e4785fb4e5a9859a0993d58ce422de", 43598),
]

# Exact stock GET reply builders and caches; these do not initialize a bike
# session or authorize writes. Both board variants share code addresses.
REGION_GET = {
    "e6002-3.3.0-board1": (0xD85D * 2, 0xD76, 0xF32, 0xB1E),
    "e6002-3.3.0-board2": (0xD85D * 2, 0xD76, 0xF32, 0xB1E),
    "e6002-3.4.0-board1": (0xDB93 * 2, 0xDA7, 0xF69, 0xB49),
    "e6002-3.4.0-board2": (0xDB93 * 2, 0xDA7, 0xF69, 0xB49),
}


def probe(path):
    """Execute exact stock stack/GET probes; never claim transport or writes."""
    with open(path, "rb") as stream:
        image = stream.read(0x40001)
    digest = hashlib.sha256(image).hexdigest()
    matches = [profile for profile in CPU_PROFILES if profile[1] == digest]
    if len(matches) != 1:
        raise RuntimeError("unsupported MN101 CPU-probe image SHA-256 " + digest)
    name, _, wrapper = matches[0]
    # The first IRQ and balanced save/restore wrapper must come from the
    # pinned image's vector table, not a caller-selected code address.
    first = unpack_pc(int.from_bytes(image[4:8], "little"))
    if unpack_pc(int.from_bytes(image[0x24:0x28], "little")) != wrapper:
        raise RuntimeError("MN101 profiled vector mismatch")
    cases = []
    for entry, operations in [
        (first, ["rti"]),
        (wrapper, ["add_sp", "store_a_sp", "store_dw_sp", "load_dw_sp",
                   "load_a_sp", "add_sp", "rti"]),
    ]:
        result = MN101Machine(image).run(entry, interrupt=True, budget=32)
        result['kind'] = 'interrupt-stack'
        result["passed"] = (result["returned"] and result["initial"] == result["final"]
                            and [ins["op"] for ins in result["trace"]] == operations)
        cases.append(result)
    if name in REGION_GET:
        entry, selector, cache, response = REGION_GET[name]
        for region in (0, 1, 4):
            cpu = MN101Machine(image, ram_start=0, ram_end=0x2800)
            cpu.write(selector, 1)
            cpu.write(cache, region)
            result = cpu.run(entry, budget=128)
            packet = bytes(cpu.read(response + i) for i in range(8))
            expected = bytes([6, 0, 0x16, 0xAE, 1, region, 0, 0])
            result.update(kind='region-get', region=region, reply=packet.hex())
            result['passed'] = (result['returned'] and packet == expected
                                and result['initial']['sp'] == result['final']['sp'])
            cases.append(result)
    return dict(passed=all(case["passed"] for case in cases), profile=name,
                sha256=digest, cpu="MN101 instruction subset", hardware_access=False,
                level="isolated stock CPU/region GET instructions with synthetic stack/RAM",
                not_validated=["packet transport/session", "settings SET", "authorization",
                               "persistent storage", "reset/boot", "physical MCU/peripherals"],
                cases=cases)
