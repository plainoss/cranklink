"""Container metadata is descriptive and never grants instruction execution."""

LEGACY_FAMILIES = {0x08: 'DU-E6000', 0x0C: 'DU-E6001', 0x15: 'DU-E6002'}
ENCRYPTED_MARKER = bytes.fromhex('6f1b529d847c17dbd72af3d337cb3cc3')


def _version(data):
    if data[0] >> 4 > 9:
        raise ValueError('invalid packed firmware version')
    return '.'.join(str(n) for n in (data[0] >> 4, data[0] & 15, data[1], data[2]))


def legacy_metadata(data):
    """Recognize the legacy header layout, independently of file names or ISA.

    Offsets and structural checks match the native firmware container reader.
    Header validation checks size/trailer consistency, not authenticity, CPU
    compatibility, or whether any particular firmware function can execute.
    """
    if (data.endswith(ENCRYPTED_MARKER) or len(data) < 0xD6 or data[:4] != b'\xff' * 4
            or any(data[n] != 0 for n in (7, 11, 15)) or data[0xC0] != 5):
        return None
    errors = []
    revision = data[0xCC]
    kind = {1: 'dcas2', 2: 'dcas2', 3: 'dcas2i'}.get(revision)
    if kind is None:
        errors.append('unsupported legacy header revision')
    versions = []
    for start in (0xC1, 0xD3):
        try:
            versions.append(_version(data[start:start + 3]))
        except ValueError as error:
            errors.append(str(error))
            versions.append(None)
    declared = int.from_bytes(data[0xC6:0xCA], 'little')
    if declared != len(data):
        errors.append('declared size does not match image size')
    if not data.endswith(b'et'):
        errors.append('missing et trailer')
    return dict(kind=kind, header_offset=0xC0, header_revision=revision,
                version=versions[0], compatibility_version=versions[1],
                firmware_series=data[0xCA], firmware_unit=data[0xCB], board=data[0xCD],
                family=LEGACY_FAMILIES.get(data[0xCA]), declared_bytes=declared,
                metadata_valid=not errors, validation_errors=errors)


def unsupported_detail(metadata):
    if not metadata['metadata_valid']:
        return 'invalid legacy container metadata: ' + '; '.join(metadata['validation_errors'])
    family = metadata['family'] or f"firmware series {metadata['firmware_series']:02X}"
    return (f"recognized {family} {metadata['kind']} D{metadata['version']} container; "
            'CPU instruction set, memory mapping and execution backend/profile are unverified')
