#!/usr/bin/env bash
# Regression test for scripts/lib/resolve_base_ref.sh, covering the exact
# staleness scenario from https://github.com/daghovland/rdf-datalog/issues/612:
# a worktree's local `main` is behind `origin/main`, so `main..HEAD` sweeps
# in commits already on `origin/main` and picks up a wrong, much older
# `startedAtTime`.
#
# Run directly:
#   bash scripts/test_resolve_base_ref.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/resolve_base_ref.sh
source "$SCRIPT_DIR/lib/resolve_base_ref.sh"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

FAILURES=0

assert_eq() {
    local desc="$1" expected="$2" actual="$3"
    if [ "$expected" = "$actual" ]; then
        echo "ok - $desc"
    else
        echo "NOT OK - $desc (expected '$expected', got '$actual')"
        FAILURES=$((FAILURES + 1))
    fi
}

ORIG_DIR="$(pwd)"

# ── Explicit ref always wins, regardless of repo state ──────────────────────

mkdir "$TMP_DIR/explicit-repo"
cd "$TMP_DIR/explicit-repo"
git init -q
result="$(resolve_base_ref "some-explicit-ref")"
assert_eq "explicit ref is echoed back unchanged" "some-explicit-ref" "$result"
cd "$ORIG_DIR"

# ── No origin remote at all: falls back to literal "main" ──────────────────

mkdir "$TMP_DIR/no-origin-repo"
cd "$TMP_DIR/no-origin-repo"
git init -q
result="$(resolve_base_ref "")"
assert_eq "falls back to 'main' when origin/main is unresolvable" "main" "$result"
cd "$ORIG_DIR"

# ── The actual #612 scenario: local main stale vs origin/main ───────────────
#
# Reproduce exactly what the issue describes: a worktree's local `main`
# ref was set once (at worktree-creation time) and never updated, while
# `origin/main` on that same worktree keeps advancing (via `git fetch`) as
# other PRs merge. A feature branch cut from the *current* origin/main tip
# therefore has several origin-only commits as ancestors that the stale
# local `main` doesn't know about. Plain `git log main..HEAD` then sweeps
# those older, unrelated commits in alongside the real feature commit, and
# `--reverse | head -1` picks one of them as a bogus (much older)
# "startedAtTime". resolve_base_ref must use origin/main's tip instead.

cd "$TMP_DIR"
git init -q --bare origin.git

# Worktree clone: local main starts at the repo's first commit only.
git clone -q origin.git worktree-sim
cd worktree-sim
git config user.email "test@example.com"
git config user.name "Test"
git checkout -q -b main

echo "base" > file.txt
git add file.txt
git commit -q -m "initial commit on main"
git push -q origin main

# origin/main advances with more commits (other PRs merging elsewhere)
# *without* this worktree's local main being updated to match — done via a
# separate clone so worktree-sim's local main branch pointer is untouched.
cd "$TMP_DIR"
git clone -q origin.git other-clone
cd other-clone
git config user.email "other@example.com"
git config user.name "Other"
git checkout -q -b main origin/main
echo "later main change 1" > later1.txt
git add later1.txt
git commit -q -m "later main commit 1"
echo "later main change 2" > later2.txt
git add later2.txt
git commit -q -m "later main commit 2"
git push -q origin main

# Back in worktree-sim: fetch (updates the origin/main *remote-tracking*
# ref) but never touch local main, exactly like a long-lived worktree.
cd "$TMP_DIR/worktree-sim"
git fetch -q origin

# The feature branch is cut from the current origin/main tip (as a real
# branch would be, e.g. via `git checkout -b feat origin/main`), so its
# history includes the two "later main commit" commits as ancestors that
# the stale local main is unaware of.
git checkout -q -b feature/612-test origin/main
echo "feature work" > feature.txt
git add feature.txt
git commit -q -m "the real feature commit"

# Plain "main" (the pre-#612 default) sweeps in the unrelated origin-only
# commits because local main doesn't know about them.
buggy_log_output="$(git log "main..HEAD" --format=%s | sort)"
expected_buggy="$(printf '%s\n' "later main commit 1" "later main commit 2" "the real feature commit" | sort)"
assert_eq "test setup reproduces the #612 staleness (plain 'main' sweeps in unrelated commits)" \
  "$expected_buggy" "$buggy_log_output"

# resolve_base_ref with no explicit ref must resolve to a base such that
# `git log <result>..HEAD` shows only the real feature commit.
base_ref="$(resolve_base_ref "")"
log_output="$(git log "${base_ref}..HEAD" --format=%s)"
assert_eq "base-ref excludes the unrelated origin-only commits" "the real feature commit" "$log_output"

cd "$ORIG_DIR"

if [ "$FAILURES" -eq 0 ]; then
    echo "All resolve_base_ref tests passed."
    exit 0
else
    echo "$FAILURES test(s) failed."
    exit 1
fi
