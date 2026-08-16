#!/usr/bin/env bash
# Small CLI wrapper for the provenance SPARQL query library (issue #327):
#   provenance/queries/run.sh <query-name> [<values-replacement>]
#
# Mirrors backlog/queries/run.sh's own pattern: reuses the dagalog binary's
# own --query/--format table path (nothing new to invent) against the
# worked grounding example(s) under provenance/summaries/ -- pass
# --data-dir/-D to point at a different set of .ttl files.
#
# The optional second positional argument re-parameterizes a query that has
# a `VALUES ?var { ... }` line (currently related_to_file.sparql and
# related_to_crate.sparql -- see #353) without hand-editing the .sparql
# file: it's substituted verbatim into that line's braces in a scratch copy
# (outside the repo, via mktemp; the checked-in file is never touched), so
# quoting is the caller's responsibility -- pass the whole VALUES content
# exactly as SPARQL expects it (a quoted string literal for related_to_file,
# a prefixed IRI for related_to_crate).
#
# Examples:
#   provenance/queries/run.sh reasoning_for_pr
#   provenance/queries/run.sh all_decision_points
#   provenance/queries/run.sh related_to_file '"sparql_parser/src/lib.rs"'
#   provenance/queries/run.sh related_to_crate crate:sparql_parser
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROVENANCE_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(dirname "$PROVENANCE_DIR")"

usage() {
  echo "Usage: $0 [-D <data-dir>] <query-name> [<values-replacement>]" >&2
  echo "" >&2
  echo "Available queries:" >&2
  for f in "$SCRIPT_DIR"/*.sparql; do
    echo "  $(basename "${f%.sparql}")" >&2
  done
  exit 1
}

DATA_DIR="$PROVENANCE_DIR/summaries"
while getopts "D:h" opt; do
  case "$opt" in
    D) DATA_DIR="$OPTARG" ;;
    h) usage ;;
    *) usage ;;
  esac
done
shift $((OPTIND - 1))

[ $# -eq 1 ] || [ $# -eq 2 ] || usage
QUERY_NAME="$1"
QUERY_FILE="$SCRIPT_DIR/$QUERY_NAME.sparql"
[ -f "$QUERY_FILE" ] || { echo "No such query: $QUERY_NAME (looked for $QUERY_FILE)" >&2; usage; }

if [ $# -eq 2 ]; then
  VALUES_REPLACEMENT="$2"
  TMP_QUERY="$(mktemp "${TMPDIR:-/tmp}/provenance-query-XXXXXX.sparql")"
  trap 'rm -f "$TMP_QUERY"' EXIT
  # Replace only the { ... } contents of the query's `VALUES ?var { ... }`
  # line, whatever the variable name -- avoids sed delimiter/escaping
  # headaches for paths and crate IRIs by keeping the replacement as a
  # single awk -v value (no shell re-interpolation of its contents).
  awk -v repl="$VALUES_REPLACEMENT" '
    /^[[:space:]]*VALUES[[:space:]]+\?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\{/ {
      match($0, /^[[:space:]]*VALUES[[:space:]]+\?[A-Za-z_][A-Za-z0-9_]*[[:space:]]*\{[[:space:]]*/)
      print substr($0, 1, RLENGTH) repl " }"
      next
    }
    { print }
  ' "$QUERY_FILE" > "$TMP_QUERY"
  QUERY_FILE="$TMP_QUERY"
fi

DATA_ARGS=(
  --data "$REPO_ROOT/backlog/ontology/vocabulary.ttl"
  --data "$REPO_ROOT/backlog/ontology/agentprov-vocabulary.ttl"
  # related_to_file/related_to_crate (#351) need the backlog snapshot's
  # bl:touchesFile/bl:touchesCrate facts, which live outside provenance/
  # entirely -- harmless extra data for the other queries, which don't
  # reference bl:PullRequest facts beyond the IRI itself.
  --data "$REPO_ROOT/backlog/examples/snapshot.ttl"
)
for f in "$DATA_DIR"/*.ttl; do
  DATA_ARGS+=(--data "$f")
done

exec cargo run --manifest-path "$REPO_ROOT/Cargo.toml" -q --bin dagalog -- \
  "${DATA_ARGS[@]}" -Q "$QUERY_FILE" --format table
