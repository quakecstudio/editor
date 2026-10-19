#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Decides which path a CI run takes (docs/ci.md, "Light changes").
#
#   scripts/ci-changes.sh classify            classify the paths on stdin, one per line
#   scripts/ci-changes.sh diff <base> <head>  classify the files that differ between two commits
#
# Prints "light" when every path is on the light allowlist below and "full"
# otherwise. An empty list, a diff that cannot be computed and an unknown
# command all print "full", so any doubt runs everything. The exit status is 0
# whenever a verdict was printed.
#
# A light path is read only by the cargo xtask ci job and by xtask's own tests,
# which CI runs on every OS on a light change too; docs/ci.md lists the readers
# of each. Everything else, including a path this list does not know, is full.
set -euo pipefail

is_light() {
  case $1 in
  # qs-tests reads every .rs file in the tree for spec citations.
  *.rs) return 1 ;;
  # The spec-citation ratchet, the test vectors and Test Assets read docs/spec.
  docs/spec/*) return 1 ;;
  docs/* | plan/* | .claude/* | .kiro/*) return 0 ;;
  */*) return 1 ;;
  # Markdown at the root: README.md, CLAUDE.md, AGENTS.md, ATTRIBUTION.md and the like.
  *.md) return 0 ;;
  *) return 1 ;;
  esac
}

classify() {
  local path count=0
  while IFS= read -r path || [[ -n "$path" ]]; do
    [[ -n "$path" ]] || continue
    count=$((count + 1))
    if ! is_light "$path"; then
      echo full
      return
    fi
  done
  if [[ "$count" -eq 0 ]]; then
    echo full
  else
    echo light
  fi
}

# --no-renames lists a move as a deletion and an addition, so a file moved from
# a full path to a light one still counts its old path.
diff_commits() {
  local files
  if [[ -z "${1:-}" || -z "${2:-}" ]] ||
    ! files=$(git diff --name-only --no-renames "$1" "$2" -- 2>/dev/null); then
    echo full
    return
  fi
  classify <<<"$files"
}

case ${1:-} in
classify) classify ;;
diff) diff_commits "${2:-}" "${3:-}" ;;
*) echo full ;;
esac
