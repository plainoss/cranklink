"""Only explicit, hash-pinned decoded images are executable."""
import hashlib
import json
from pathlib import Path
from .machine import require
from .metadata import legacy_metadata, unsupported_detail
from .mn101 import CPU_PROFILES
from .mn101_storage import STORAGE_PROFILES

PROFILES = json.loads((Path(__file__).parent.parent / 'profiles.json').read_text())


def identify(path, kind=None):
    # Bound reads before allocating/mapping untrusted input.
    with Path(path).open('rb') as stream:
        data = stream.read(0x40001)
    digest = hashlib.sha256(data).hexdigest()
    matches = [p for p in PROFILES if p['sha256'] == digest and (kind is None or p['kind'] == kind)]
    if len(matches) != 1:
        metadata = legacy_metadata(data)
        detail = unsupported_detail(metadata) if metadata else 'decoded, profiled image required'
        if any(profile[1] == digest for profile in CPU_PROFILES):
            detail = 'isolated MN101 CPU/region GET probes available via cpu-probe; packet transport/settings write profile unavailable'
        if digest in STORAGE_PROFILES:
            detail = 'use cpu-probe or region-storage (modeled resident ROM); packet transport profile unavailable'
        require(False, f'unsupported {kind or "firmware"} SHA-256 {digest}; {detail}')
    return data, matches[0]
