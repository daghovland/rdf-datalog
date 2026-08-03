#!/usr/bin/env bash
# For each given PR number: if it's a draft and every CI check has
# concluded SUCCESS, mark it ready for review and post a short confirmation
# comment. Never merges anything — that's always a human decision.
#
#   bash scripts/pr-ready-if-green.sh 368 369 370 371
#
# With no arguments, checks every open draft PR in the repo.
#
# Uses only `gh`'s own `-q` (gojq) queries — no standalone `jq` dependency.

set -euo pipefail

if [ "$#" -gt 0 ]; then
  prs=("$@")
else
  mapfile -t prs < <(gh pr list --state open --json number,isDraft \
    -q '.[] | select(.isDraft) | .number')
fi

if [ "${#prs[@]}" -eq 0 ]; then
  echo "No draft PRs to check."
  exit 0
fi

for pr in "${prs[@]}"; do
  title="$(gh pr view "$pr" --json title -q '.title')"
  is_draft="$(gh pr view "$pr" --json isDraft -q '.isDraft')"
  total="$(gh pr view "$pr" --json statusCheckRollup -q '[.statusCheckRollup[]?] | length')"
  success="$(gh pr view "$pr" --json statusCheckRollup -q '[.statusCheckRollup[]? | select(.conclusion == "SUCCESS")] | length')"

  if [ "$total" -eq 0 ]; then
    echo "PR #$pr ($title): no CI checks reported yet, skipping."
    continue
  fi

  if [ "$success" -ne "$total" ]; then
    echo "PR #$pr ($title): CI not fully green ($success/$total SUCCESS), skipping."
    gh pr view "$pr" --json statusCheckRollup \
      -q '.statusCheckRollup[]? | "  " + .name + ": " + (.conclusion // .status)'
    continue
  fi

  if [ "$is_draft" != "true" ]; then
    echo "PR #$pr ($title): CI green, already ready for review."
    continue
  fi

  echo "PR #$pr ($title): CI fully green ($success/$total), marking ready."
  gh pr ready "$pr"
  gh pr comment "$pr" --body "CI is fully green ($success/$total checks passing). Marked ready for review — not merging, that's still a human decision."
done
