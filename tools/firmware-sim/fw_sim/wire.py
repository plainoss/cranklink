"""SM-PCE02 host framing; separate from virtual E-TUBE bus fragments."""

def encode(control, payload):
    raw = bytes([control]) + bytes(payload)
    raw += bytes([-sum(raw) & 255])
    out = bytearray([0xBB])
    for value in raw:
        out.extend((0xBD, value ^ 0x20) if value in (0xBB,0xBD) else (value,))
    out.append(0xBB)
    return bytes(out).ljust(128, b'\xff')


class Decoder:
    def __init__(self):
        self.active, self.escaped = False, False
        self.data = bytearray()

    def feed(self, chunk):
        out = []
        for value in chunk:
            if not self.active:
                if value == 0xBB: self.active = True
                continue
            if value == 0xBB:
                if not self.escaped and all(b == 0xFF for b in self.data):
                    # Empty, or fill collected after losing sync: this BB opens.
                    self.data.clear()
                    continue
                data, escaped = bytes(self.data), self.escaped
                self.data.clear()
                self.escaped = False
                self.active = not (bool(data) or escaped)
                if escaped: raise ValueError('truncated escape')
                if data:
                    if len(data) < 2 or sum(data) & 255: raise ValueError('invalid FCS or short frame')
                    out.append((data[0], data[1:-1]))
            elif self.escaped:
                self.escaped = False
                if value not in (0x9B,0x9D):
                    self.active = False
                    self.data.clear()
                    raise ValueError('invalid escape')
                self.data.append(value ^ 0x20)
            elif value == 0xBD:
                self.escaped = True
            else:
                self.data.append(value)
            if len(self.data) > 65536:
                self.active, self.escaped = False, False
                self.data.clear()
                raise ValueError('frame exceeds 65536 bytes')
        return out
