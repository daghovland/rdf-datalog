#!/usr/bin/env bash
# Stand up sparql_endpoint --serve against the combined backlog + provenance
# dataset (issue #356: https://github.com/daghovland/rdf-datalog/issues/356).
#
# Mirrors backlog/queries/run.sh and provenance/queries/run.sh's own
# "vocab file(s) + glob a directory of .ttl fixtures" pattern, extended to
# also include the backlog snapshot, and to actually --serve instead of
# running one query and exiting.
#
# Usage:
#   scripts/serve-backlog.sh [--print-data-args] [-- <extra dagalog args>]
#
#   --print-data-args   Print the resolved list of data files (one path per
#                        line) and exit, without starting a server. This is
#                        what tests/serve_backlog_provenance.rs shells out to,
#                        so the test suite always exercises the SAME file
#                        list this script actually loads -- not a second,
#                        hand-copied list that could silently drift.
#
# Any arguments after the recognized flags (or after a bare --) are passed
# through to the dagalog binary, e.g.:
#   scripts/serve-backlog.sh --port 3031
#
# Serves read-only: the snapshot is a regenerable pull
# (cargo run -p backlog --bin backlog-regenerate), so accepting writes
# through the endpoint would just be silently discarded on the next
# regeneration. Binds 0.0.0.0 (there is no --bind/--host flag) -- per #356's
# own scope this is local-only tooling, not hardened for external exposure.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

PRINT_DATA_ARGS=0
EXTRA_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --print-data-args) PRINT_DATA_ARGS=1; shift ;;
    --) shift; EXTRA_ARGS+=("$@"); break ;;
    *) EXTRA_ARGS+=("$1"); shift ;;
  esac
done

DATA_FILES=(
  "$REPO_ROOT/backlog/ontology/vocabulary.ttl"
  "$REPO_ROOT/backlog/ontology/agentprov-vocabulary.ttl"
  "$REPO_ROOT/backlog/examples/snapshot.ttl"
)
for f in "$REPO_ROOT"/provenance/summaries/*.ttl; do
  DATA_FILES+=("$f")
done

if [ "$PRINT_DATA_ARGS" -eq 1 ]; then
  printf '%s\n' "${DATA_FILES[@]}"
  exit 0
fi

DATA_ARGS=()
for f in "${DATA_FILES[@]}"; do
  DATA_ARGS+=(--data "$f")
done

exec cargo run --manifest-path "$REPO_ROOT/Cargo.toml" -q --bin dagalog -- \
  "${DATA_ARGS[@]}" --serve --read-only "${EXTRA_ARGS[@]}"
