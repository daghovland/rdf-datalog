#!/usr/bin/env bash
# Small CLI wrapper for the backlog query library (issue #286):
#   backlog/queries/run.sh <query-name>
#
# Reuses the dagalog binary's own --query/--format table path (nothing new
# to invent) against the example fixtures under backlog/examples/, standing
# in for a real loader (#284) snapshot until that exists -- pass
# --data-dir/-D to point at a different set of .ttl files once it does (see
# usage below).
#
# Examples:
#   backlog/queries/run.sh ready_not_started
#   backlog/queries/run.sh crates_with_open_bugs
#   backlog/queries/run.sh crate_dependents          # edit the VALUES line
#                                                     # in crate_dependents.sparql
#                                                     # first to change the crate
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKLOG_DIR="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(dirname "$BACKLOG_DIR")"

usage() {
  echo "Usage: $0 [-D <data-dir>] <query-name>" >&2
  echo "" >&2
  echo "Available queries:" >&2
  for f in "$SCRIPT_DIR"/*.sparql; do
    echo "  $(basename "${f%.sparql}")" >&2
  done
  exit 1
}

DATA_DIR="$BACKLOG_DIR/examples"
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

DATA_ARGS=(--data "$BACKLOG_DIR/ontology/vocabulary.ttl")
for f in "$DATA_DIR"/*.ttl; do
  DATA_ARGS+=(--data "$f")
done

exec cargo run --manifest-path "$REPO_ROOT/Cargo.toml" -q --bin dagalog -- \
  "${DATA_ARGS[@]}" -Q "$QUERY_FILE" --format table
