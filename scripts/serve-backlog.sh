#!/usr/bin/env bash
# Stand up the full local backlog dashboard: dagalog --serve against the
# combined backlog + provenance dataset (issue #356:
# https://github.com/daghovland/rdf-datalog/issues/356), PLUS the standalone
# backlog_endpoint dashboard server pointed at that same dagalog instance
# (issue #381 Stage 1: https://github.com/daghovland/rdf-datalog/issues/381
# -- the dashboard is its own crate/binary, not a route inside
# sparql_endpoint, so getting a working dashboard now takes two processes).
#
# Mirrors backlog/queries/run.sh and provenance/queries/run.sh's own
# "vocab file(s) + glob a directory of .ttl fixtures" pattern, extended to
# also include the backlog snapshot, and to actually --serve instead of
# running one query and exiting.
#
# Usage:
#   scripts/serve-backlog.sh [--print-data-args] [--dagalog-port PORT]
#                             [--dashboard-port PORT] [-- <extra dagalog args>]
#
#   --print-data-args   Print the resolved list of data files (one path per
#                        line) and exit, without starting either server. This
#                        is what tests/serve_backlog_provenance.rs shells out
#                        to, so the test suite always exercises the SAME file
#                        list this script actually loads -- not a second,
#                        hand-copied list that could silently drift.
#
#   --dagalog-port PORT     Port for the dagalog SPARQL endpoint (default 3030).
#   --dashboard-port PORT   Port for the backlog_endpoint dashboard (default 3031).
#
# Any arguments after a bare -- are passed through to the dagalog binary
# only (e.g. `-- --base-iri http://example.org/`); backlog_endpoint always
# just gets --port/--sparql-endpoint derived from the two port flags above.
#
# dagalog serves read-only: the snapshot is a regenerable pull
# (cargo run -p backlog --bin backlog-regenerate), so accepting writes
# through the endpoint would just be silently discarded on the next
# regeneration. Both processes bind 0.0.0.0 (there is no --bind/--host flag)
# -- per #356/#381's own scope this is local-only tooling, not hardened for
# external exposure.
#
# On exit (including Ctrl-C), the background dagalog process is killed so
# this script doesn't leak a server after the dashboard it was started for
# is gone.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

PRINT_DATA_ARGS=0
DAGALOG_PORT=3030
DASHBOARD_PORT=3031
EXTRA_ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --print-data-args) PRINT_DATA_ARGS=1; shift ;;
    --dagalog-port) DAGALOG_PORT="$2"; shift 2 ;;
    --dashboard-port) DASHBOARD_PORT="$2"; shift 2 ;;
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

# Start dagalog in the background (not exec'd) so this script can also
# start backlog_endpoint afterwards. Killed on exit via the trap below.
cargo run --manifest-path "$REPO_ROOT/Cargo.toml" -q --bin dagalog -- \
  "${DATA_ARGS[@]}" --serve --read-only --port "$DAGALOG_PORT" \
  --cors-allow-origin "http://localhost:$DASHBOARD_PORT" \
  --cors-allow-origin "http://127.0.0.1:$DASHBOARD_PORT" \
  "${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"}" &
DAGALOG_PID=$!
trap 'kill "$DAGALOG_PID" 2>/dev/null || true' EXIT

# backlog_endpoint runs in the foreground (NOT exec'd -- exec would replace
# this script's own process image, so the EXIT trap above would never fire
# and dagalog would leak after Ctrl-C). Running it as a normal foreground
# command means Ctrl-C's SIGINT reaches it directly (same process group,
# same controlling terminal); once it exits, this script's own EXIT trap
# runs and kills the background dagalog process.
cargo run --manifest-path "$REPO_ROOT/Cargo.toml" -q -p backlog-endpoint -- \
  --port "$DASHBOARD_PORT" \
  --sparql-endpoint "http://localhost:$DAGALOG_PORT/sparql"
