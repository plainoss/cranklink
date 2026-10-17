#!/usr/bin/env bash
set -euo pipefail

# Add PR to GitHub Merge Queue with squash merge
PR_NUM="${1:-$(gh pr view --json number -q .number 2>/dev/null || echo '')}"

if [ -z "$PR_NUM" ]; then
  echo "Error: No PR number provided or found for current branch."
  echo "Usage: ./tools/queue-pr.sh [PR_NUMBER]"
  exit 1
fi

echo "Enabling auto-merge (squash) for PR #${PR_NUM} to enter merge queue..."
gh pr merge "$PR_NUM" --auto --squash
echo "PR #${PR_NUM} is queued for merge once required checks pass."
