#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Wait for CI on a pull request, add it to the merge queue, and wait until it merges.
# Usage: land.sh <pr-number> [timeout-minutes]
# Exit 0 when merged; 1 on a CI failure, a close, or the timeout (the reason is printed).
set -euo pipefail

pr="${1:?usage: land.sh <pr-number> [timeout-minutes]}"
timeout_min="${2:-90}"
deadline=$(($(date +%s) + timeout_min * 60))

if ! out=$(gh pr checks "$pr" --watch --fail-fast 2>&1); then
  if grep -qi "no checks reported" <<<"$out"; then
    echo "No CI checks on #$pr yet; continuing."
  else
    echo "$out"
    echo "CI failed on #$pr. Fix it on the branch and run land.sh again."
    exit 1
  fi
fi

# With a merge queue, --auto adds the PR to the queue once required checks pass.
gh pr merge "$pr" --auto || gh pr merge "$pr"

while :; do
  state=$(gh pr view "$pr" --json state --jq .state)
  case "$state" in
  MERGED)
    echo "#$pr merged."
    exit 0
    ;;
  CLOSED)
    echo "#$pr was closed without merging."
    exit 1
    ;;
  esac
  if [ "$(date +%s)" -gt "$deadline" ]; then
    echo "#$pr is still $state after ${timeout_min} minutes: check whether the merge queue removed it (gh pr view $pr --web)."
    exit 1
  fi
  sleep 30
done
