#!/usr/bin/env python3
"""Catch the current Vite build's silently omitted worker and runtime modules.

This checks the existing worker URL contract, not deployment readiness. Browser
handshake/simulator tests and host cross-origin isolation are also required
before enabling a future deployment workflow.
"""
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
required = (
    "index.html",
    "worker.js",
    "host.js",
    "pkg/cranklink.js",
    "pkg/cranklink_bg.wasm",
    "schemas/ep800-crg.json",
    "schemas/ep600.json",
    "schemas/ep801.json",
)
missing = [name for name in required if not (root / name).is_file()]
if missing:
    sys.exit("Web runtime assets missing from build: " + ", ".join(missing))
print("Web runtime assets present; publishing remains disabled pending browser validation.")
