#!/usr/bin/env python3
"""Stock-only EP800 bridge recovery controller; never retries a restore invocation."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

D_SHA = "35521a18a7859482d98fab2523cbb97dfc653def4db00091c6265f2573f46543"
M_SHA = "94986ebf8c21587a2b54b197f19a51c4118918bdca0aff6296a7727f7dfacc44"


def check_file(path, expected):
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
        raise ValueError(f"stock firmware SHA256 mismatch: {path}")


def binary_path(directory, name):
    return directory / (name + (".exe" if sys.platform == "win32" else ""))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--d", required=True, type=Path)
    parser.add_argument("--m", required=True, type=Path)
    parser.add_argument("--serial", required=True, help="your known motor serial, 12 hex digits")
    parser.add_argument("--output", required=True, type=Path, help="new private evidence directory")
    parser.add_argument("--bin-dir", type=Path, default=Path(__file__).resolve().parent / "target/debug")
    parser.add_argument("--countdown", type=int, default=10)
    parser.add_argument("--attempts", type=int, default=1, help="maximum keys-only attempts (1..100)")
    parser.add_argument("--execute", action="store_true", help="enable hardware access and stock D write")
    args = parser.parse_args()
    if len(args.serial) != 12 or any(c not in "0123456789abcdefABCDEF" for c in args.serial):
        parser.error("serial must be exactly 12 hex digits")
    if not 0 <= args.countdown <= 120 or not 1 <= args.attempts <= 100:
        parser.error("countdown must be 0..120; attempts must be 1..100")
    check_file(args.d, D_SHA)
    check_file(args.m, M_SHA)
    probe = binary_path(args.bin_dir, "sustained_key_epochs")
    restore = binary_path(args.bin_dir, "restore_sustained_boot")
    for binary in (probe, restore):
        if not binary.is_file():
            parser.error(f"build binary first: {binary}")
    print("Preflight OK: exact stock D 4.2.0.0; M 4.1.11.0 reference only.", flush=True)
    if not args.execute:
        print("No hardware opened. Add --execute to unlock and write stock D.")
        return 0
    args.output.mkdir(parents=True, exist_ok=False)
    for remaining in range(args.countdown, 0, -1):
        print(f"COUNTDOWN {remaining}: keep bridge secure through finish/reset", flush=True)
        time.sleep(1)
    for attempt in range(1, args.attempts + 1):
        prefix = args.output / f"attempt-{attempt:03}"
        wire = Path(f"{prefix}.keys.wire.jsonl")
        stdout = Path(f"{prefix}.keys.stdout.json")
        stderr = Path(f"{prefix}.keys.stderr.txt")
        print(f"Keys-only attempt {attempt}/{args.attempts}", flush=True)
        with stdout.open("x") as out, stderr.open("x") as err:
            result = subprocess.run([str(probe), "--execute", "--normal-prep", "--trace", str(wire)], stdout=out, stderr=err)
        try:
            summary = json.loads(stdout.read_text())
        except ValueError:
            summary = {}
        print(json.dumps({"attempt": attempt, "exit": result.returncode,
                          "acks": summary.get("ordered_key_acks"),
                          "error": summary.get("error") or stderr.read_text()}), flush=True)
        if result.returncode or not summary.get("full_handshake"):
            if attempt < args.attempts:
                time.sleep(3)
            continue
        # Only raw proof validation in the Rust helper authorizes the attach.
        # Once invoked, NEVER retry, even if an error appears to precede erase.
        print("ATTACH RESTORE: keep bridge held; live identity checked before erase", flush=True)
        journal = Path(f"{prefix}.restore.journal.jsonl")
        trace = Path(f"{prefix}.restore.wire.jsonl")
        command = [str(restore), "--execute", str(wire), str(journal), str(trace),
                   str(args.d.resolve()), str(args.m.resolve()), args.serial]
        with Path(f"{prefix}.restore.output.txt").open("x") as log:
            process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            for line in process.stdout:
                print(line, end="", flush=True)
                log.write(line)
                log.flush()
            code = process.wait()
        print(f"Restore ended: {code}; no automatic write retry. Journal: {journal}", flush=True)
        return code
    print("No full handshake; restore was not invoked. No firmware write.", flush=True)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
