#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Mark an issue In Progress on the project board when work on it starts.
# Usage: start.sh <issue-number> [project-number]
# Sets the Status field of the issue's item on the organization project (default 1) to
# "In Progress", adding the issue to the project first if it is missing. Prints nothing and
# exits 0 when the item is set or already was; exits 1 with the reason otherwise.
#
# Nothing else sets In Progress: plan/apply.py writes only Phase, Layer, Size and Requirements,
# and GitHub's built-in workflow moves an item to Done when its issue closes.
set -euo pipefail

issue="${1:?usage: start.sh <issue-number> [project-number]}"
project="${2:-1}"
status="In Progress"
repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
owner="${repo%/*}"

# Prints: the project id, the Status field id and the option id for $status, one per line.
# shellcheck disable=SC2016 # $owner and $number are GraphQL variables, not shell ones.
ids=$(gh api graphql -F owner="$owner" -F number="$project" -f query='
  query($owner: String!, $number: Int!) {
    organization(login: $owner) { projectV2(number: $number) {
      id
      field(name: "Status") { ... on ProjectV2SingleSelectField { id options { id name } } } } } }' \
  --jq ".data.organization.projectV2 | .id, .field.id,
        (.field.options[] | select(.name == \"$status\") | .id)")
project_id=$(sed -n 1p <<<"$ids")
field_id=$(sed -n 2p <<<"$ids")
option_id=$(sed -n 3p <<<"$ids")
if [ -z "$field_id" ] || [ -z "$option_id" ]; then
  echo "project $owner/$project has no Status field with an \"$status\" option" >&2
  exit 1
fi

# Prints the issue's item id on this project and its current Status, tab-separated, or nothing.
# shellcheck disable=SC2016 # $owner, $name and $issue are GraphQL variables, not shell ones.
item=$(gh api graphql -F owner="$owner" -F name="${repo#*/}" -F issue="$issue" -f query='
  query($owner: String!, $name: String!, $issue: Int!) {
    repository(owner: $owner, name: $name) { issue(number: $issue) {
      projectItems(first: 20) { nodes {
        id project { id }
        fieldValueByName(name: "Status") { ... on ProjectV2ItemFieldSingleSelectValue { name } } } } } } }' \
  --jq ".data.repository.issue.projectItems.nodes[]
        | select(.project.id == \"$project_id\") | [.id, (.fieldValueByName.name // \"\")] | @tsv")
item_id=$(cut -f1 <<<"$item")
current=$(cut -f2 <<<"$item")

if [ -z "$item_id" ]; then
  item_id=$(gh project item-add "$project" --owner "$owner" \
    --url "https://github.com/$repo/issues/$issue" --format json --jq .id)
fi
if [ "$current" = "$status" ]; then
  exit 0
fi
gh project item-edit --id "$item_id" --project-id "$project_id" --field-id "$field_id" \
  --single-select-option-id "$option_id" >/dev/null
