#!/usr/bin/env bash
# Small CLI wrapper for the provenance SPARQL query library (issue #327):
#   provenance/queries/run.sh <query-name>
#
# Mirrors backlog/queries/run.sh's own pattern: reuses the dagalog binary's
# own --query/--format table path (nothing new to invent) against the
# worked grounding example(s) under provenance/summaries/ -- pass
# --data-dir/-D to point at a different set of .ttl files.
#
# Examples:
#   provenance/queries/run.sh reasoning_for_pr
#   provenance/queries/run.sh all_decision_points
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROVENANCE_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(dirname "$PROVENANCE_DIR")"

usage() {
  echo "Usage: $0 [-D <data-dir>] <query-name>" >&2
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

[ $# -eq 1 ] || usage
QUERY_NAME="$1"
QUERY_FILE="$SCRIPT_DIR/$QUERY_NAME.sparql"
[ -f "$QUERY_FILE" ] || { echo "No such query: $QUERY_NAME (looked for $QUERY_FILE)" >&2; usage; }

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
