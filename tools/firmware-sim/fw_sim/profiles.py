"""Only explicit, hash-pinned decoded images are executable."""
import hashlib
import json
from pathlib import Path
from .machine import require

PROFILES = json.loads((Path(__file__).parent.parent / 'profiles.json').read_text())


def identify(path, kind=None):
    # Bound reads before allocating/mapping untrusted input.
    with Path(path).open('rb') as stream:
        data = stream.read(0x40001)
    digest = hashlib.sha256(data).hexdigest()
    matches = [p for p in PROFILES if p['sha256'] == digest and (kind is None or p['kind'] == kind)]
    require(len(matches) == 1, f'unsupported {kind or "firmware"} SHA-256 {digest}; decoded, profiled image required')
    return data, matches[0]
