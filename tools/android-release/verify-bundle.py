#!/usr/bin/env python3
"""Verify a signed Cranklink AAB and record the release candidate's identity."""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

APPLICATION_ID = "app.plainoss.cranklink"
TARGET_SDK = "36"
MIN_SDK = "26"
ABIS = {"arm64-v8a", "x86_64"}
PAGE = 16384


def run(*args):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT).strip()
    except subprocess.CalledProcessError as error:
        raise RuntimeError(error.output.strip()) from None


def native_alignment(data, name):
    """Check 16 KB LOAD alignment from ELF64 program headers, without host binutils."""
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        raise ValueError(f"Unsupported ELF format: {name}")
    offset = struct.unpack_from("<Q", data, 32)[0]
    size, count = struct.unpack_from("<HH", data, 54)
    loads = []
    relros = []
    for index in range(count):
        kind, flags, file_offset, address, _, _, memory_size, alignment = struct.unpack_from(
            "<IIQQQQQQ", data, offset + index * size
        )
        if kind == 1:
            if alignment < PAGE or (address - file_offset) % PAGE:
                raise ValueError(f"ELF LOAD is not 16 KB aligned: {name}")
            loads.append((address, address + memory_size, flags))
        if kind == 0x6474E552:
            relros.append((address, address + memory_size))
    if not loads:
        raise ValueError(f"ELF has no LOAD segments: {name}")
    for start, end in relros:
        # Linkers may pad RELRO past its LOAD. Reject only rounding that would
        # protect writable bytes outside RELRO.
        protected_start = start // PAGE * PAGE
        protected_end = (end + PAGE - 1) // PAGE * PAGE
        for a, b, flags in loads:
            if flags & 2 and ((max(a, protected_start) < min(b, start)) or
                              (max(a, end) < min(b, protected_end))):
                raise ValueError(f"ELF RELRO overlaps writable content at 16 KB: {name}")


def check_manifest(text, version_code, version_name):
    manifest = ET.fromstring(text)
    android = "{http://schemas.android.com/apk/res/android}"
    sdk = manifest.find("uses-sdk")
    expected = [(manifest, "package", APPLICATION_ID),
                (manifest, android + "versionCode", str(version_code)),
                (manifest, android + "versionName", version_name),
                (sdk, android + "targetSdkVersion", TARGET_SDK),
                (sdk, android + "minSdkVersion", MIN_SDK)]
    for element, attribute, value in expected:
        if element is None or element.get(attribute) != value:
            raise ValueError(f"Unexpected manifest value for {attribute}")
    if manifest.find("application").get(android + "debuggable") == "true":
        raise ValueError("Release bundle is debuggable")


def native_libraries(names):
    libraries = sorted(n for n in names if re.search(r"/lib/[^/]+/[^/]+\.so$", n))
    abis = {n.split("/")[-2] for n in libraries}
    if abis != ABIS or not any(n.endswith("/libcranklink_android.so") for n in libraries):
        raise ValueError(f"Bundle must contain libcranklink_android.so for exactly {sorted(ABIS)}; found {sorted(abis)}")
    return libraries


def certificate_sha256(keytool, bundle):
    cert = run(keytool, "-printcert", "-jarfile", str(bundle), "-rfc")
    encoded = re.search(r"-----BEGIN CERTIFICATE-----\s*(.*?)\s*-----END CERTIFICATE-----", cert, re.S)
    if not encoded:
        raise ValueError("AAB signing certificate missing")
    return hashlib.sha256(base64.b64decode(encoded[1])).hexdigest()


def verify(args):
    bundle = args.bundle.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    java_home = Path(os.environ["JAVA_HOME"])
    java, jarsigner, keytool = (str(java_home / "bin" / tool) for tool in ("java", "jarsigner", "keytool"))
    tool = str(args.bundletool.resolve())

    signature = run(jarsigner, "-J-Duser.language=en", "-verify", str(bundle))
    if "jar verified." not in signature or "unsigned entries" in signature.lower():
        raise ValueError("AAB signature could not be verified")
    fingerprint = certificate_sha256(keytool, bundle)
    if fingerprint != args.certificate_sha256.replace(":", "").lower():
        raise ValueError("AAB was signed with an unexpected certificate")

    run(java, "-jar", tool, "validate", f"--bundle={bundle}")
    check_manifest(run(java, "-jar", tool, "dump", "manifest", f"--bundle={bundle}"),
                   args.version_code, args.version_name)
    with zipfile.ZipFile(bundle) as archive:
        native = native_libraries(archive.namelist())
        for name in native:
            native_alignment(archive.read(name), name)
    if "PAGE_ALIGNMENT_16K" not in run(java, "-jar", tool, "dump", "config", f"--bundle={bundle}"):
        raise ValueError("Bundle does not request 16 KB APK page alignment")

    with tempfile.TemporaryDirectory() as temp:
        apks = Path(temp) / "release.apks"
        store_password = Path(temp) / "store-password"
        key_password = Path(temp) / "key-password"
        for path, variable in [(store_password, "ANDROID_KEYSTORE_PASSWORD"), (key_password, "ANDROID_KEY_PASSWORD")]:
            path.touch(mode=0o600)
            path.write_text(os.environ[variable])
        run(java, "-jar", tool, "build-apks", f"--bundle={bundle}", f"--output={apks}",
            "--mode=universal", f"--ks={os.environ['ANDROID_KEYSTORE_PATH']}",
            f"--ks-key-alias={os.environ['ANDROID_KEY_ALIAS']}",
            f"--ks-pass=file:{store_password}", f"--key-pass=file:{key_password}")
        with zipfile.ZipFile(apks) as archive:
            archive.extract("universal.apk", temp)
        zipalign = Path(os.environ["ANDROID_HOME"]) / f"build-tools/{os.environ.get('ANDROID_BUILD_TOOLS', '36.0.0')}/zipalign"
        packaging = run(str(zipalign), "-c", "-P", "16", "-v", "4", str(Path(temp) / "universal.apk"))

    checksum = hashlib.sha256(bundle.read_bytes()).hexdigest()
    result = {
        "applicationId": APPLICATION_ID, "versionCode": args.version_code,
        "versionName": args.version_name, "commit": args.commit,
        "sha256": checksum, "uploadCertificateSha256": fingerprint,
        "nativeLibraries": native, "pageSizeBytes": PAGE,
    }
    (output / "release.json").write_text(json.dumps(result, indent=2) + "\n")
    (output / "SHA256SUMS").write_text(f"{checksum}  {bundle.name}\n")
    (output / "verification.txt").write_text(signature + "\n" + packaging + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bundle", type=Path, required=True)
    parser.add_argument("--bundletool", type=Path, required=True)
    parser.add_argument("--certificate-sha256", required=True)
    parser.add_argument("--version-code", type=int, required=True)
    parser.add_argument("--version-name", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--output", type=Path, required=True)
    verify(parser.parse_args())
