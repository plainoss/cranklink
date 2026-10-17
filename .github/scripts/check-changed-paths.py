#!/usr/bin/env python3
"""Determine if relevant files changed between git base and HEAD for CI jobs.

Supports:
- pull_request events (compares against base.sha or origin/main)
- merge_group events (compares against merge_group.base_sha)
- push events (compares against before SHA or HEAD~1)
- local execution / workflow_dispatch (defaults to origin/main or HEAD~1)
"""
import fnmatch
import json
import os
import pathlib
import subprocess
import sys

CLI_PATTERNS = [
    "src/**",
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    ".github/workflows/build-cli.yml",
    ".github/scripts/check-changed-paths.py",
]

WEB_PATTERNS = [
    "src/**",
    "Cargo.toml",
    "Cargo.lock",
    "web-instrument/**",
    ".github/workflows/deploy-pages.yml",
    ".github/scripts/check-web-assets.py",
    ".github/scripts/serve-web-build.py",
    ".github/scripts/check-changed-paths.py",
]


def get_base_sha() -> str:
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")

    if event_path and pathlib.Path(event_path).is_file():
        try:
            with open(event_path, "r", encoding="utf-8") as f:
                payload = json.load(f)

            if event_name == "pull_request" and "pull_request" in payload:
                sha = payload["pull_request"].get("base", {}).get("sha")
                if sha:
                    return sha

            if event_name == "merge_group" and "merge_group" in payload:
                sha = payload["merge_group"].get("base_sha")
                if sha:
                    return sha

            if event_name == "push":
                before = payload.get("before")
                if before and before != "0000000000000000000000000000000000000000":
                    return before
        except Exception as e:
            print(f"[check-changed-paths] Warning reading GITHUB_EVENT_PATH: {e}", file=sys.stderr)

    # Fallbacks
    for candidate in ("origin/main", "HEAD~1"):
        res = subprocess.run(["git", "rev-parse", "--verify", candidate], capture_output=True, text=True)
        if res.returncode == 0:
            return candidate

    return ""


def get_changed_files(base_ref: str) -> list[str]:
    if not base_ref:
        return []
    # Try three-dot diff first, fallback to two-dot
    for diff_arg in (f"{base_ref}...HEAD", f"{base_ref}..HEAD"):
        res = subprocess.run(["git", "diff", "--name-only", diff_arg], capture_output=True, text=True)
        if res.returncode == 0:
            return [line.strip() for line in res.stdout.splitlines() if line.strip()]

    # Fallback: diff against base_ref directly
    res = subprocess.run(["git", "diff", "--name-only", base_ref, "HEAD"], capture_output=True, text=True)
    if res.returncode == 0:
        return [line.strip() for line in res.stdout.splitlines() if line.strip()]

    return []


def matches_any(file_path: str, patterns: list[str]) -> bool:
    for pat in patterns:
        if fnmatch.fnmatch(file_path, pat):
            return True
        if pat.endswith("/**") and (file_path.startswith(pat[:-3]) or file_path == pat[:-3].rstrip("/")):
            return True
    return False


def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ("cli", "web"):
        print("Usage: check-changed-paths.py [cli|web] [--force]", file=sys.stderr)
        sys.exit(1)

    target = sys.argv[1]
    patterns = CLI_PATTERNS if target == "cli" else WEB_PATTERNS

    # Force check via argument or tag push / workflow_dispatch
    event_name = os.environ.get("GITHUB_EVENT_NAME", "")
    ref = os.environ.get("GITHUB_REF", "")
    if "--force" in sys.argv or ref.startswith("refs/tags/v") or event_name == "workflow_dispatch":
        print(f"[check-changed-paths] Force/Release trigger detected for {target}. Enforcing checks.")
        set_output("true")
        return

    base_ref = get_base_sha()
    if not base_ref:
        print(f"[check-changed-paths] Could not resolve base git ref. Defaulting to running checks for {target}.")
        set_output("true")
        return

    changed_files = get_changed_files(base_ref)
    matching = [f for f in changed_files if matches_any(f, patterns)]

    if matching:
        print(f"[check-changed-paths] Changes detected for '{target}' ({len(matching)} files matched, base: {base_ref}):")
        for f in matching[:10]:
            print(f"  + {f}")
        if len(matching) > 10:
            print(f"  ... and {len(matching) - 10} more")
        set_output("true")
    else:
        print(f"[check-changed-paths] No relevant changes detected for '{target}' (compared against {base_ref}).")
        set_output("false")


def set_output(has_changes: str):
    github_output = os.environ.get("GITHUB_OUTPUT")
    if github_output:
        with open(github_output, "a", encoding="utf-8") as f:
            f.write(f"has_changes={has_changes}\n")
    print(f"OUTPUT: has_changes={has_changes}")


if __name__ == "__main__":
    main()
