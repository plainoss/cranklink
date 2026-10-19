#!/usr/bin/env python3
"""Validate public runtime contents before testing and publishing the web build."""
import pathlib
import re
import sys


def check(root: pathlib.Path) -> list[str]:
    errors = []
    required = (
        "index.html",
        "_headers",
        "worker.js",
        "host.js",
        "coi-serviceworker.js",
        "licenses/coi-serviceworker-LICENSE.txt",
        "pkg/cranklink.js",
        "pkg/cranklink_bg.wasm",
        "schemas/ep800-crg.json",
        "schemas/ep600.json",
        "schemas/ep801.json",
        "schemas/ep500.json",
        "schemas/e5100.json",
        "schemas/e8000.json",
        "schemas/e7000.json",
        "schemas/e6100.json",
        "schemas/e6000.json",
    )
    for name in required:
        path = root / name
        if not path.is_file() or not path.stat().st_size:
            errors.append(f"Missing or empty runtime asset: {name}")
    if not (root / ".nojekyll").is_file():
        errors.append("Missing .nojekyll")

    # This is a publication allowlist, not just a presence check. Generated
    # documentation, maps, metadata and stale binaries must never hitchhike.
    allowed_files = set(required) | {".nojekyll"}
    allowed_directories = {"assets", "pkg", "schemas", "licenses"}
    bundled_asset = re.compile(r"assets/index-[A-Za-z0-9_-]+\.(js|css)")
    bundles = {"js": [], "css": []}
    package = root / "pkg"

    wasm = package / "cranklink_bg.wasm"
    if wasm.is_file() and wasm.read_bytes()[:4] != b"\x00asm":
        errors.append("Compiled module does not have a WebAssembly header")
    glue = package / "cranklink.js"
    if glue.is_file():
        source = glue.read_text(encoding="utf-8")
        if "../host.js" not in source:
            errors.append("Generated JS must import the sibling host using ../host.js")
        if re.search(r'''(?:from\s*|import\s*)["']/''', source):
            errors.append("Generated JS contains an origin-root module import")

    # Build-machine paths must be remapped before compiling, including in WASM.
    personal_path = re.compile(rb"/(?:Users|home)/[A-Za-z0-9_.-]+/")
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if path.is_symlink():
            errors.append(f"Symlink in public artifact: {relative}")
        elif path.is_dir():
            if relative.as_posix() not in allowed_directories:
                errors.append(f"Unexpected directory in public artifact: {relative}")
        elif path.is_file():
            name = relative.as_posix()
            match = bundled_asset.fullmatch(name)
            if match:
                bundles[match.group(1)].append(name)
                if not path.stat().st_size:
                    errors.append(f"Empty bundled asset: {relative}")
            elif name not in allowed_files:
                errors.append(f"Unexpected file in public artifact: {relative}")
            if personal_path.search(path.read_bytes()):
                errors.append(f"Build-machine path in public artifact: {relative}")
        else:
            errors.append(f"Unsupported file type in public artifact: {relative}")
    for extension, names in bundles.items():
        if len(names) != 1:
            errors.append(f"Expected exactly one hashed {extension} bundle; found {len(names)}")
    return errors


if __name__ == "__main__":
    root = pathlib.Path(sys.argv[1])
    errors = check(root)
    if errors:
        sys.exit("Invalid web artifact:\n" + "\n".join(errors))
    print("Public runtime assets validated; browser smoke tests must also pass before publishing.")
