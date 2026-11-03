#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Wait for CI on a pull request, then squash-merge it.
# Usage: land.sh <pr-number> [timeout-minutes]
# Exit 0 when merged; 1 on a CI failure, a conflict, a removal from the merge queue, a close,
# the timeout, or a body that puts a closing keyword in a negated sentence (check-pr-body.sh);
# the reason is printed.
#
# The merge queue on main is off (docs/ci.md, "Protection of main"), so the PR merges as soon as
# its required checks pass on the PR itself. The script also works with the queue on: there the
# same merge command adds the PR to the queue, and the script waits for the queue to merge it.
# The checks wait uses `gh pr checks --watch` at a 30 s refresh, not gh's 10 s, so that several
# sessions landing at once stay under GitHub's secondary rate limit; the queue wait polls once a
# minute.
set -euo pipefail

pr="${1:?usage: land.sh <pr-number> [timeout-minutes]}"
timeout_min="${2:-90}"
deadline=$(($(date +%s) + timeout_min * 60))
poll_seconds=10
queue_poll_seconds=60
repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
here=$(dirname "${BASH_SOURCE[0]}")

timed_out() {
  echo "#$pr has not merged after ${timeout_min} minutes ($1): check it with gh pr view $pr --web."
  exit 1
}

# Runs a command, killing it when the deadline passes: gh's watch commands have no timeout.
until_deadline() {
  local left=$((deadline - $(date +%s)))
  if [ "$left" -le 0 ]; then
    return 124
  fi
  perl -e 'alarm shift; exec @ARGV or die "exec $ARGV[0]: $!\n"' "$left" "$@"
}

past_deadline() {
  [ "$(date +%s)" -ge "$deadline" ]
}

# Prints: state, mergeStateStatus, and whether the PR is in the merge queue (queued or -).
pr_status() {
  # shellcheck disable=SC2016 # $owner, $name and $pr are GraphQL variables, not shell ones.
  gh api graphql -F owner="${repo%/*}" -F name="${repo#*/}" -F pr="$pr" -f query='
    query($owner: String!, $name: String!, $pr: Int!) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $pr) { state mergeStateStatus mergeQueueEntry { position } }
      }
    }' --jq '.data.repository.pullRequest
      | [.state, .mergeStateStatus, (if .mergeQueueEntry then "queued" else "-" end)] | @tsv'
}

# A closing keyword in a negated sentence ("does not close #84") still closes the issue on merge
# (check-pr-body.sh). Checked before waiting for CI, to fail fast, and again just before merging,
# in case the body was edited while CI ran.
fail_if_body_closes_wrongly() {
  local body status=0
  body=$(gh pr view "$pr" --json body --jq .body)
  "$here/check-pr-body.sh" <<<"$body" || status=$?
  case "$status" in
  0) ;;
  1)
    echo "#$pr was not merged. Fix its body (gh pr edit $pr --body-file <file>) and run land.sh again."
    exit 1
    ;;
  *)
    echo "check-pr-body.sh failed (exit $status), so #$pr was not merged; its body may be fine."
    exit 1
    ;;
  esac
}

fail_if_conflicting() {
  if [ "$1" = DIRTY ]; then
    echo "#$pr conflicts with main. Merge origin/main into the branch, push and run land.sh again."
    exit 1
  fi
}

fail_if_body_closes_wrongly

# 1. The PR's own checks. A PR that conflicts with main gets no checks, so check for that while
# waiting for them to be reported.
while :; do
  if out=$(until_deadline gh pr checks "$pr" --watch --fail-fast --interval 30 2>&1); then
    break
  fi
  if past_deadline; then
    timed_out "its checks were still running"
  fi
  if ! grep -qi "no checks reported" <<<"$out"; then
    echo "$out"
    echo "CI failed on #$pr. Fix it on the branch and run land.sh again."
    exit 1
  fi
  IFS=$'\t' read -r _ merge_state _ < <(pr_status)
  fail_if_conflicting "$merge_state"
  sleep "$poll_seconds"
done

# 2. The merge. The squash commit takes the PR's title and body. With a merge queue on, the
# same command adds the PR to the queue instead.
fail_if_body_closes_wrongly
IFS=$'\t' read -r state merge_state _ < <(pr_status)
fail_if_conflicting "$merge_state"
case "$state" in
CLOSED)
  echo "#$pr was closed without merging."
  exit 1
  ;;
OPEN) gh pr merge "$pr" --squash ;;
esac

# 3. Confirm the merge, waiting for the queue if there is one.
queued=false
missing=0
while :; do
  IFS=$'\t' read -r state merge_state in_queue < <(pr_status)
  case "$state" in
  MERGED)
    echo "#$pr merged. Merge origin/main into any other open branch before landing it."
    exit 0
    ;;
  CLOSED)
    echo "#$pr was closed without merging."
    exit 1
    ;;
  esac
  if past_deadline; then
    timed_out "state $state, merge state $merge_state"
  fi
  fail_if_conflicting "$merge_state"
  if [ "$in_queue" = queued ]; then
    queued=true
    missing=0
  elif $queued; then
    # Out of the queue after being in it, and still open: the queue removed it. Seen twice in a
    # row so the moment between leaving the queue and the merge is not mistaken for a removal.
    missing=$((missing + 1))
    if [ "$missing" -ge 2 ]; then
      echo "The merge queue removed #$pr; its runs are listed by gh run list --event merge_group."
      echo "Fix the cause on the branch and run land.sh again."
      exit 1
    fi
  fi
  sleep "$queue_poll_seconds"
done
