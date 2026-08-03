#!/usr/bin/env bash
# Summarize the state of every git worktree: branch, ahead/behind vs its
# remote tracking branch, and whether it has uncommitted changes.
#
# Purpose: sub-agents working in .claude/worktrees/* can be interrupted
# mid-task (reboot, API spend limit) leaving real, finished work sitting
# uncommitted or unpushed. Before trusting a sub-agent's "done" report or a
# PR's green CI, run this to see at a glance which worktrees have local
# work that never made it to origin.
#
#   bash scripts/worktree-status.sh
#
# For each worktree (except the main one) prints:
#   <path> [<branch>] ahead=<N> behind=<N> dirty=<yes/no/no-upstream>

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

git worktree list --porcelain | awk '
  /^worktree / { path=$2 }
  /^branch /   { branch=$2; print path, branch }
' | while read -r path branch; do
  # Skip the main worktree (top-level repo checkout).
  if [ "$path" = "$(git rev-parse --show-toplevel)" ]; then
    continue
  fi

  short_branch="${branch#refs/heads/}"
  dirty="no"
  if [ -n "$(git -C "$path" status --porcelain 2>/dev/null)" ]; then
    dirty="yes"
  fi

  upstream="$(git -C "$path" rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
  if [ -z "$upstream" ]; then
    printf '%s [%s] ahead=? behind=? dirty=%s (no upstream — never pushed)\n' \
      "$path" "$short_branch" "$dirty"
    continue
  fi

  read -r ahead behind <<< "$(git -C "$path" rev-list --left-right --count "${upstream}...HEAD" 2>/dev/null | awk '{print $2, $1}')"

  flag=""
  if [ "$dirty" = "yes" ] || [ "${ahead:-0}" -gt 0 ]; then
    flag="  <-- has unpushed/uncommitted work"
  fi

  printf '%s [%s] ahead=%s behind=%s dirty=%s%s\n' \
    "$path" "$short_branch" "${ahead:-0}" "${behind:-0}" "$dirty" "$flag"
done
