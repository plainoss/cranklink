"""Inventory user-supplied images; unprofiled images never become executable."""
import hashlib
import json
from pathlib import Path
from .profiles import PROFILES

MARKER = bytes.fromhex('6f1b529d847c17dbd72af3d337cb3cc3')


def inventory(roots):
    by_hash = {p['sha256']:p for p in PROFILES}
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
            rows.append(row)
            if profile: fixtures[profile['id']] = str(path.resolve())
    return dict(scope='supplied local images only; profiled is not a claim of full-board emulation',
                unique_images=len(rows),profiled_images=len(fixtures),images=rows), fixtures
