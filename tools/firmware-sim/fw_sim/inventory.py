"""Inventory user-supplied images; unprofiled images never become executable."""
import hashlib
import json
from pathlib import Path
from .profiles import PROFILES
from .metadata import ENCRYPTED_MARKER as MARKER, legacy_metadata, unsupported_detail
from .mn101 import CPU_PROFILES
from .mn101_storage import STORAGE_PROFILES


def inventory(roots):
    by_hash = {p['sha256']:p for p in PROFILES}
    cpu_by_hash = {p[1]: p[0] for p in CPU_PROFILES}
    seen, rows, fixtures = set(), [], {}
    for root in roots:
        root = Path(root)
        if not root.exists(): raise ValueError(f'input does not exist: {root}')
        paths = [root] if root.is_file() else sorted(root.rglob('*'))
        for path in paths:
            if not path.is_file() or path.suffix.lower() not in ('.bin','.dat'): continue
            if any(part in ('node_modules','target','.git','.venv') for part in path.parts): continue
            if path.stat().st_size > 16*1024*1024:
                rows.append(dict(file=path.name,status='too-large')); continue
            data = path.read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            if digest in seen: continue
            seen.add(digest)
            profile = by_hash.get(digest)
            encrypted = data.endswith(MARKER)
            row = dict(file=path.name,sha256=digest,bytes=len(data),
                       status='profiled' if profile else 'needs-decoding' if encrypted else 'unprofiled',
                       profile=profile['id'] if profile else None,
                       reason=None if profile else 'encrypted container: provide decoded image' if encrypted else
                       'no verified CPU, memory and protocol entry-point profile for these exact bytes')
            metadata = None if encrypted else legacy_metadata(data)
            if metadata:
                row['container'] = metadata
                row['execution'] = dict(supported=bool(profile),
                                        cpu='profiled' if profile else 'unverified',
                                        backend='profiled' if profile else 'unsupported')
                if not profile:
                    row['reason'] = unsupported_detail(metadata)
                if digest in cpu_by_hash:
                    row['execution'].update(cpu='MN101 instruction subset', backend='isolated-cpu-probe',
                                            cpu_probe=cpu_by_hash[digest])
                    row['reason'] = 'isolated MN101 CPU/region GET probes available; no transport or settings write profile'
                    if digest in STORAGE_PROFILES:
                        row['region_storage'] = dict(supported=True, profile=STORAGE_PROFILES[digest],
                                                     resident_rom='modeled', transport=False,
                                                     authentication=False, erase_rollover=False)
                        row['reason'] = 'MN101 CPU/GET and modeled-ROM region persistence available; no packet transport profile'
            rows.append(row)
            if profile: fixtures[profile['id']] = str(path.resolve())
    return dict(scope='supplied local images only; profiled is not a claim of full-board emulation',
                unique_images=len(rows),profiled_images=len(fixtures),images=rows), fixtures
