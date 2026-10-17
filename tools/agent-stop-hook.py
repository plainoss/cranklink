#!/usr/bin/env python3
import sys
import json
import subprocess
import os

def main():
    try:
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
    except Exception:
        payload = {}

    # Prevent runaway continuation loops
    exec_num = payload.get("executionNum", 1)
    if exec_num > 2:
        print(json.dumps({}))
        return

    # Check which files have been modified or staged
    try:
        diff_unstaged = subprocess.check_output(["git", "diff", "--name-only"], text=True).splitlines()
        diff_staged = subprocess.check_output(["git", "diff", "--cached", "--name-only"], text=True).splitlines()
        untracked = subprocess.check_output(["git", "ls-files", "--others", "--exclude-standard"], text=True).splitlines()
        changed = set(diff_unstaged + diff_staged + untracked)
    except Exception:
        # If git check fails, do not block the agent
        print(json.dumps({}))
        return

    rs_changed = [f for f in changed if f.endswith(".rs")]
    web_changed = [f for f in changed if f.startswith("web-instrument/") and f.endswith((".ts", ".tsx"))]

    # Fast check: Rust formatting
    if rs_changed:
        res = subprocess.run(["cargo", "fmt", "--check"], capture_output=True, text=True)
        if res.returncode != 0:
            print(json.dumps({
                "decision": "continue",
                "reason": "Automated Guardrail: Rust formatting check failed. Please run 'cargo fmt' before stopping."
            }))
            return

    # Fast check: Web Instrument TypeScript
    if web_changed and os.path.isdir("web-instrument"):
        res = subprocess.run(
            ["npm", "--prefix", "web-instrument", "run", "typecheck"],
            capture_output=True,
            text=True
        )
        if res.returncode != 0:
            err = res.stderr or res.stdout
            err_snip = "\n".join(err.splitlines()[:10])
            print(json.dumps({
                "decision": "continue",
                "reason": f"Automated Guardrail: TypeScript typecheck failed. Please fix before stopping:\n{err_snip}"
            }))
            return

    # All checks passed or no code modified
    print(json.dumps({}))

if __name__ == "__main__":
    main()
