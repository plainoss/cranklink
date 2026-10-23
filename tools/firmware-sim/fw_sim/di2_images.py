"""Hash-pinned Di2 research inputs. Acquisition never grants execution coverage."""
import hashlib
import json
from pathlib import Path
import urllib.request

from .pull import REQUEST_HEADERS, SHIMANO_OFFICIAL_URL_BASE

MAX_BYTES = 2 * 1024 * 1024
IMAGES = (
    dict(filename='BTDN110.4.5.0.dat', model='BT-DN110', board=1,
         sha256='65af4a0e68d52b43bc94660ab3e5d14c97021d59f528ac3846c8f221364c9f3b'),
    dict(filename='2BTDN110.4.5.0.dat', model='BT-DN110', board=2,
         sha256='46d1b37f00f5d9d1b54cb7112c4d8841f996438b05f614b4a9947366781a76ef'),
    # E-TUBE's EW-WU111 firmware declaration uses the EWWU101 main family.
    # This mapping does not identify the radio/board fitted to a physical bike.
    dict(filename='EWWU101.4.6.0.dat', model='EW-WU101/EW-WU111 main', board=1,
         sha256='1e68ca8b0fc07d2fc8dc831f6564e27e60d57670409829104636a4d4c7183183'),
    dict(filename='UPDATENRF-DI2-ap.4.7.1.hex', model='first-generation Nordic Di2 application',
         sha256='0a52a9240febce92e2a033122c02aeaa23eadbaf1c8648628974dc257e3b9fa1'),
    dict(filename='UPDATENRF-DI2-bt.1.4.0.hex', model='first-generation Nordic Di2 bootloader',
         sha256='78570dc816f26b88eabf91e31c3155ac51affcc46e7f061cef63b3595960fc63'),
    dict(filename='UPDATENRF-sd.3.0.0.hex', model='first-generation Nordic SoftDevice package',
         sha256='4c76d6f0f27a2ae276f566054b3d1e2adac4ee4d605ae98e80978e4edd834f2f'),
)


def intel_hex_memory(raw):
    """Check record lengths/checksums, EOF, addresses and conflicting overlaps."""
    if len(raw) > MAX_BYTES:
        raise ValueError('HEX input exceeds size limit')
    memory, upper, ended = {}, 0, False
    for line in raw.decode('ascii').splitlines():
        if not line:
            continue
        if ended or not line.startswith(':') or len(line) > 521:
            raise ValueError('invalid HEX record or data after EOF')
        record = bytes.fromhex(line[1:])
        if len(record) < 5 or len(record) != record[0] + 5 or sum(record) & 255:
            raise ValueError('invalid HEX checksum or length')
        count, address, kind = record[0], int.from_bytes(record[1:3], 'big'), record[3]
        data = record[4:4 + count]
        if kind == 0:
            if address + count > 0x10000:
                raise ValueError('HEX data crosses address bank')
            for index, value in enumerate(data):
                target = upper + address + index
                # nRF flash and UICR only. Never allocate a sparse flat image.
                if not (target < 0x100000 or 0x10001000 <= target < 0x10002000):
                    raise ValueError('HEX address outside research flash/UICR bounds')
                if target in memory and memory[target] != value:
                    raise ValueError('conflicting HEX overlap')
                memory[target] = value
        elif kind == 1 and count == 0 and address == 0:
            ended = True
        elif kind == 4 and count == 2 and address == 0:
            upper = int.from_bytes(data, 'big') << 16
        elif kind == 5 and count == 4 and address == 0:
            pass  # Start-linear metadata; reset vectors remain image bytes.
        else:
            raise ValueError('unsupported or malformed HEX record type')
    if not ended or not memory:
        raise ValueError('HEX requires data and EOF')
    return memory


def assess_image(meta, raw):
    if len(raw) > MAX_BYTES or hashlib.sha256(raw).hexdigest() != meta['sha256']:
        raise ValueError(f"SHA-256 mismatch or oversized image: {meta['filename']}")
    result = dict(meta, bytes=len(raw), execution='unprofiled', topology_validated=False)
    if meta['filename'].endswith('.dat'):
        if (len(raw) < 0xd6 or raw[:4] != b'\xff'*4 or raw[0xc0] != 5
                or raw[0xcc] != 3 or not raw.endswith(b'et')
                or int.from_bytes(raw[0xc6:0xca], 'little') != len(raw)
                or raw[0xcd] != meta['board']):
            raise ValueError('unexpected DCAS2i research header')
        vectors = []
        for offset in range(4, 0x80, 4):
            packed = int.from_bytes(raw[offset:offset+4], 'little')
            address, half = packed & 0x3ffff, (packed >> 23) & 1
            index = address - 0x4000
            if packed & ~0x83ffff or not 0 <= index < len(raw)-1:
                continue
            first = (raw[index] >> (4*half)) & 15
            second = (raw[index+half] >> (4*(1-half))) & 15
            if (first, second) == (0, 3):
                vectors.append(offset)
        result.update(format='DCAS2i', series=raw[0xca], unit=raw[0xcb],
                      version=f'{raw[0xc1] >> 4}.{raw[0xc1] & 15}.{raw[0xc2]}.{raw[0xc3]}',
                      mn101_rti_vector_offsets=vectors,
                      blocker='MN101-family mapping corroboration only; no reviewed CPU backend or recovered service handlers')
    else:
        memory = intel_hex_memory(raw)
        addresses = sorted(memory)
        regions = []
        for address in addresses:
            if not regions or address != regions[-1]['end']:
                regions.append(dict(start=address, end=address+1))
            else:
                regions[-1]['end'] += 1
        base = addresses[0]
        vectors = bytes(memory.get(base+index, 255) for index in range(8))
        stack, reset = int.from_bytes(vectors[:4], 'little'), int.from_bytes(vectors[4:], 'little')
        result.update(format='Intel HEX', regions=regions, initial_stack=stack, reset_vector=reset,
                      arm_vector_candidate=(0x20000000 <= stack < 0x20010000
                                            and bool(reset & 1) and (reset & ~1) in memory),
                      blocker='Radio/SoftDevice is separate from the main MCU; no profiled gateway or battery lifecycle execution')
    return result


def acquire(output, offline=False):
    """Fetch exact research inputs or revalidate a local cache, never execute them."""
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    rows = []
    for meta in IMAGES:
        path = output / meta['filename']
        if path.exists():
            with path.open('rb') as stream:
                raw = stream.read(MAX_BYTES + 1)
        elif offline:
            raise ValueError(f'missing offline image: {path.name}')
        else:
            request = urllib.request.Request(SHIMANO_OFFICIAL_URL_BASE + meta['filename'],
                                             headers=REQUEST_HEADERS)
            with urllib.request.urlopen(request, timeout=30) as response:
                raw = response.read(MAX_BYTES + 1)
        row = assess_image(meta, raw)
        if not path.exists():
            # A competing process must not replace an existing research input.
            with path.open('xb') as stream:
                stream.write(raw)
        row['url'] = SHIMANO_OFFICIAL_URL_BASE + meta['filename']
        rows.append(row)
    report = dict(scope='verified research acquisition, not executable firmware coverage',
                  executable_profiles_added=0, topology_validated=False, images=rows)
    (output / 'di2-acquisition.json').write_text(json.dumps(report, indent=2) + '\n')
    return report
