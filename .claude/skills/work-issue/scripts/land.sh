#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Wait for CI on a pull request, add it to the merge queue, and wait until it merges.
# Usage: land.sh <pr-number> [timeout-minutes]
# Exit 0 when merged; 1 on a CI failure, a conflict, a removal from the queue, a close, the
# timeout, or a body that puts a closing keyword in a negated sentence (check-pr-body.sh); the
# reason is printed.
#
# Waits use gh's watch commands: `gh pr checks --watch` for the PR's checks and `gh run watch`
# for the merge queue's runs. A short poll covers only the gaps no watch exists for: checks not
# reported yet, the queue not yet having started its runs, and the merge itself. The watches
# refresh every 30 s (checks) and 60 s (runs), not gh's 10 s and 3 s, so that several agents
# landing at once stay under GitHub's secondary rate limit.
set -euo pipefail

pr="${1:?usage: land.sh <pr-number> [timeout-minutes]}"
timeout_min="${2:-90}"
deadline=$(($(date +%s) + timeout_min * 60))
poll_seconds=10
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

# Prints: state, mergeStateStatus, the queue entry's head commit (or -), auto-merge (on or off).
pr_status() {
  # shellcheck disable=SC2016 # $owner, $name and $pr are GraphQL variables, not shell ones.
  gh api graphql -F owner="${repo%/*}" -F name="${repo#*/}" -F pr="$pr" -f query='
    query($owner: String!, $name: String!, $pr: Int!) {
      repository(owner: $owner, name: $name) {
        pullRequest(number: $pr) {
          state mergeStateStatus
          mergeQueueEntry { headCommit { oid } }
          autoMergeRequest { enabledAt }
        }
      }
    }' --jq '.data.repository.pullRequest
      | [.state, .mergeStateStatus, (.mergeQueueEntry.headCommit.oid // "-"),
         (if .autoMergeRequest then "on" else "off" end)] | @tsv'
}

# A closing keyword in a negated sentence ("does not close #84") still closes the issue on merge
# (check-pr-body.sh). Checked before waiting for CI, to fail fast, and again just before queueing,
# in case the body was edited while CI ran.
fail_if_body_closes_wrongly() {
  local body status=0
  body=$(gh pr view "$pr" --json body --jq .body)
  "$here/check-pr-body.sh" <<<"$body" || status=$?
  case "$status" in
  0) ;;
  1)
    echo "#$pr was not queued. Fix its body (gh pr edit $pr --body-file <file>) and run land.sh again."
    exit 1
    ;;
  *)
    echo "check-pr-body.sh failed (exit $status), so #$pr was not queued; its body may be fine."
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
  IFS=$'\t' read -r _ merge_state _ _ < <(pr_status)
  fail_if_conflicting "$merge_state"
  sleep "$poll_seconds"
done

# 2. With a merge queue, --auto adds the PR to the queue once required checks pass.
fail_if_body_closes_wrongly
gh pr merge "$pr" --auto || gh pr merge "$pr"

# 3. The queue's runs, then the merge.
queued=false
missing=0
watched=" "
while :; do
  IFS=$'\t' read -r state merge_state queue_oid auto < <(pr_status)
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
  if past_deadline; then
    timed_out "state $state, merge state $merge_state, auto-merge $auto"
  fi
  fail_if_conflicting "$merge_state"

  if [ "$queue_oid" = - ]; then
    # Out of the queue after being in it, and still open: the queue removed it. Seen twice in a
    # row so the moment between leaving the queue and the merge is not mistaken for a removal.
    if $queued; then
      missing=$((missing + 1))
      if [ "$missing" -ge 2 ]; then
        echo "The merge queue removed #$pr (auto-merge $auto); its runs are listed by"
        echo "gh run list --event merge_group. Fix the cause on the branch and run land.sh again."
        exit 1
      fi
    fi
    sleep "$poll_seconds"
    continue
  fi
  queued=true
  missing=0

  new_run=false
  for run in $(gh run list --commit "$queue_oid" --event merge_group \
    --json databaseId --jq '.[].databaseId'); do
    case "$watched" in *" $run "*) continue ;; esac
    new_run=true
    if ! out=$(until_deadline gh run watch "$run" --exit-status --compact --interval 60 2>&1); then
      if past_deadline; then
        timed_out "merge queue run $run was still running"
      fi
      conclusion=$(gh run view "$run" --json conclusion --jq .conclusion)
      # A run cancelled because the queue rebuilt its group is not this PR's failure.
      if [ "$conclusion" != cancelled ]; then
        echo "$out"
        echo "Merge queue run $run for #$pr ended $conclusion. Fix it on the branch and run land.sh again."
        exit 1
      fi
    fi
    watched="$watched$run "
  done
  if ! $new_run; then
    sleep "$poll_seconds"
  fi
done
