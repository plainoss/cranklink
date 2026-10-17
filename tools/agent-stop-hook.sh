#!/usr/bin/env bash
set -euo pipefail

# Read stdin JSON payload from Antigravity lifecycle
INPUT=$(cat)
EXEC_NUM=$(echo "$INPUT" | jq -r '.executionNum // 1' 2>/dev/null || echo "1")

# Guard against infinite continuation loops
if [ "$EXEC_NUM" -gt 2 ]; then
  echo "{}"
  exit 0
fi

# Run nano-staged on all unstaged changed files
if [ -d "web-instrument/node_modules" ]; then
  OUTPUT=$(npx --prefix web-instrument nano-staged --unstaged --quiet --bail 2>&1) || {
    # If nano-staged failed, tell the agent to continue and fix the issues
    jq -n --arg reason "Automated Guardrail: nano-staged checks failed on changed files. Please fix these issues before finishing:\n$OUTPUT" \
      '{decision: "continue", reason: $reason}'
    exit 0
  }
fi

# If everything passed
echo "{}"
