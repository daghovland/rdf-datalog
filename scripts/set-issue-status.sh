#!/usr/bin/env bash
# Set a GitHub issue/PR's Status field (Todo/In Progress/Done) on the
# "Dagalog" project (user project #11) via the Projects v2 GraphQL API.
#
#   bash scripts/set-issue-status.sh 381 "In Progress"
#   bash scripts/set-issue-status.sh 381 Done
#
# Matching bl:WorkflowStatus (backlog/ontology/vocabulary.ttl): Status only
# ever advances Todo -> Ready -> InProgress -> Done, where "Ready" is the
# `ready` label (not a Status column -- the Project's Status field itself
# only has Todo/In Progress/Done, see CLAUDE.md's Implementation workflow).
#
# Marking "In Progress" as early as possible (before or immediately after
# creating the worktree, before delegating to a sub-agent) is what lets a
# second agent scanning for ready work notice an issue is already claimed --
# see CLAUDE.md step 1. Marking "Done" happens when the PR is opened and CI
# is green (ready for review, not yet merged) -- NOT when the issue closes;
# closing happens automatically on merge via "Closes #N" in the PR body.
#
# Uses only `gh`'s own `-q` (gojq) queries -- no standalone `jq` dependency.

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <issue-number> <Todo|'In Progress'|Done>" >&2
  exit 1
fi

issue="$1"
status="$2"

project_id="PVT_kwHOAAbH684BbhXV"
field_id="PVTSSF_lAHOAAbH684BbhXVzhWRUuE"

case "$status" in
  Todo) option_id="f75ad846" ;;
  "In Progress") option_id="47fc9ee4" ;;
  Done) option_id="98236657" ;;
  *)
    echo "error: status must be one of: Todo, 'In Progress', Done (got: $status)" >&2
    exit 1
    ;;
esac

item_id="$(gh api graphql -f query='
  query($owner: String!, $repo: String!, $issue: Int!) {
    repository(owner: $owner, name: $repo) {
      issue(number: $issue) {
        projectItems(first: 10) {
          nodes {
            id
            project { number }
          }
        }
      }
    }
  }' -f owner=daghovland -f repo=rdf-datalog -F issue="$issue" \
  -q '.data.repository.issue.projectItems.nodes[] | select(.project.number == 11) | .id')"

if [ -z "$item_id" ]; then
  echo "error: issue #$issue is not on the Dagalog project (#11) -- add it first" >&2
  exit 1
fi

gh api graphql -f query='
  mutation($project: ID!, $item: ID!, $field: ID!, $option: String!) {
    updateProjectV2ItemFieldValue(input: {
      projectId: $project
      itemId: $item
      fieldId: $field
      value: { singleSelectOptionId: $option }
    }) {
      projectV2Item { id }
    }
  }' -f project="$project_id" -f item="$item_id" -f field="$field_id" -f option="$option_id" \
  >/dev/null

echo "Issue #$issue: Status -> $status"
