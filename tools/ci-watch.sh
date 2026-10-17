#!/usr/bin/env bash
set -euo pipefail

# Watch GitHub Actions CI status without busy-waiting polling loops
PR_NUM="${1:-$(gh pr view --json number -q .number 2>/dev/null || echo '')}"

if [ -z "$PR_NUM" ]; then
  echo "Error: No PR number provided or found for current branch."
  echo "Usage: ./tools/ci-watch.sh [PR_NUMBER]"
  exit 1
fi

echo "Watching GitHub Actions checks for PR #${PR_NUM}..."
gh pr checks "$PR_NUM" --watch
