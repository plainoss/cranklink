#!/usr/bin/env python3
"""Build pinned, unchanged browser workers and compare persistent Bluetooth saves.

Firmware and raw browser output stay local. Requires wasm32 Rust target,
wasm-bindgen matching Cargo.lock, and an installed Playwright Chromium runtime.
"""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BASE = '0cfb180f576544d75a63a7fbe36f38670ae8679c'
FIXED = '158e8f5bc8d5e379c08ee3fa4ca4d59c9af10789'


def source_at(revision, destination):
    paths = ['Cargo.toml', 'Cargo.lock', 'src', 'web-instrument/worker.js',
             'web-instrument/host.js', 'web-instrument/ble.js', 'web-instrument/files.js']
    archive = subprocess.run(['git', 'archive', revision, *paths], cwd=ROOT,
                             capture_output=True, check=True).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        for entry in tar:
            if not entry.isfile():
                continue
            target = destination / entry.name
            if not target.resolve().is_relative_to(destination.resolve()):
                raise ValueError('unsafe source archive path')
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(tar.extractfile(entry).read())


def prepare(revision, destination, target):
    destination.mkdir(parents=True, exist_ok=True)
    source_at(revision, destination)
    build = subprocess.run(['cargo', 'build', '--locked', '--no-default-features',
                            '--target', 'wasm32-unknown-unknown', '--lib',
                            '--target-dir', str(target)], cwd=destination,
                           capture_output=True, text=True)
    if build.returncode:
        raise RuntimeError(build.stderr)
    bindgen = subprocess.run(['wasm-bindgen', str(target/'wasm32-unknown-unknown/debug/cranklink.wasm'),
                              '--out-dir', str(destination/'web-instrument/pkg'), '--target', 'web'],
                             capture_output=True, text=True)
    if bindgen.returncode:
        raise RuntimeError(bindgen.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway', type=Path, required=True)
    parser.add_argument('--motor', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--playwright-module', help='installed playwright or @playwright/test module path')
    parser.add_argument('--work-dir', type=Path, help='retain local builds/results to speed up investigation')
    parser.add_argument('--skip-build', action='store_true', help='reuse both builds in --work-dir')
    parser.add_argument('--setting', choices=('lamp', 'tire-circumference'), default='lamp')
    args = parser.parse_args()
    if args.skip_build and not args.work_dir:
        parser.error('--skip-build requires --work-dir containing both pinned builds')
    temporary = tempfile.TemporaryDirectory(prefix='cl-browser-pr11-') if not args.work_dir else None
    work = (args.work_dir or Path(temporary.name)).resolve()
    work.mkdir(parents=True, exist_ok=True)
    try:
        for name, revision in [('before', BASE), ('after', FIXED)]:
            if not args.skip_build:
                print(f'Building {name} browser worker at {revision}', flush=True)
                prepare(revision, work/name, work/'target')
        command = ['node', str(ROOT/'tools/firmware-sim/browser-pr11/runner.mjs'),
                   '--work-dir', str(work), '--gateway', str(args.gateway.resolve()),
                   '--motor', str(args.motor.resolve()), '--python', os.sys.executable,
                   '--output', str(args.output.resolve()), '--setting', args.setting]
        if args.playwright_module:
            command += ['--playwright-module', args.playwright_module]
        subprocess.run(command, cwd=ROOT, check=True)
    finally:
        if temporary:
            temporary.cleanup()


if __name__ == '__main__':
    main()
