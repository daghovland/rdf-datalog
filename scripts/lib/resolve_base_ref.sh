# resolve_base_ref: pick a sane base ref for `git log <base>..HEAD`-style
# diffing, without relying on the local `main` branch being up to date.
#
# Background (issue #612): a worktree's local `main` ref is set at
# `git worktree add` time and never refreshed, so `main..HEAD` inside a
# long-lived worktree can sweep in commits that are already on
# `origin/main` but missing from the stale local `main`, picking up a much
# older, unrelated commit's timestamp.
#
# Usage: resolve_base_ref [<explicit-ref>]
#   - If <explicit-ref> is given (non-empty), it is echoed back unchanged
#     (explicit caller choice always wins).
#   - Else, if `origin/main` is resolvable, echoes
#     `git merge-base origin/main HEAD` (a concrete, unambiguous commit,
#     robust even if the branch didn't fork from origin/main's exact tip).
#   - Else, falls back to the literal ref "main" (previous behaviour).
resolve_base_ref() {
  local explicit="${1:-}"

  if [ -n "$explicit" ]; then
    echo "$explicit"
    return 0
  fi

  if git rev-parse --verify --quiet origin/main >/dev/null 2>&1; then
    git merge-base origin/main HEAD
    return 0
  fi

  echo "main"
}
