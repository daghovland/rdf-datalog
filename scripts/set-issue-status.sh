#!/usr/bin/env bash
# Set a GitHub issue/PR's Status field (Todo/Agent/In Progress/Review/Done)
# on the "Dagalog" project (user project #11) via the Projects v2 GraphQL API.
#
#   bash scripts/set-issue-status.sh 381 "In Progress"
#   bash scripts/set-issue-status.sh 381 Review
#
# Status only ever advances Todo -> Agent -> In Progress -> Review -> Done:
# - Todo: unreviewed backlog.
# - Agent: the user has reviewed the issue and approved an agent to start
#   it -- this is the pickup signal agents scan for (replaces the old
#   `ready` label as the "you may start this" gate; the `ready` label may
#   still exist on some issues for historical/reference reasons but Status
#   is authoritative). Set by the user, not by agents.
# - In Progress: an agent has claimed it and is actively working (mark this
#   as early as possible -- before or immediately after creating the
#   worktree, before delegating to a sub-agent -- so a second agent scanning
#   for Agent-status work never picks up the same issue).
# - Review: the PR is open and CI is green -- ready for the user's review,
#   NOT yet merged. This is what "Done" used to mean before this field grew
#   a dedicated Review state; agents set this, not Done.
# - Done: the PR has actually merged. Agents must NOT set this themselves --
#   it reflects a real merge, which only the user performs (see CLAUDE.md's
#   "never merge your own PR" rule). Included here only so a human/automation
#   can use this same script for that final transition; passing it from
#   agent code is almost certainly a mistake.
#
# Uses only `gh`'s own `-q` (gojq) queries -- no standalone `jq` dependency.

set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 <issue-number> <Todo|Agent|'In Progress'|Review|Done>" >&2
  exit 1
fi

issue="$1"
status="$2"

project_id="PVT_kwHOAAbH684BbhXV"
field_id="PVTSSF_lAHOAAbH684BbhXVzhWRUuE"

case "$status" in
  Todo) option_id="f75ad846" ;;
  Agent) option_id="15b07ca9" ;;
  "In Progress") option_id="47fc9ee4" ;;
  Review) option_id="e6948ef6" ;;
  Done) option_id="98236657" ;;
  *)
    echo "error: status must be one of: Todo, Agent, 'In Progress', Review, Done (got: $status)" >&2
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
