#!/usr/bin/env bash
# Rebuilds the production dagalog dataset's bl:/agp: content (issue #565:
# https://github.com/daghovland/rdf-datalog/issues/565) -- see
# docs/plans/WIRE_BACKLOG_SNAPSHOT_565_PLAN.md for the full design.
#
# Before this script existed, data/dataset.ttl on this host was a one-off,
# hand-built concatenation of the backlog ontology + a snapshot.ttl
# regeneration + provenance/summaries/*.ttl -- proven to work (it's what
# deploy/docker-compose.public.yml's dagalog service actually loads via
# --data /data/dataset.ttl) but never repeated, so it drifted stale. This
# script reproduces that exact same four-part shape, so it can be re-run
# on a schedule instead of by hand.
#
# Usage:
#   scripts/regenerate-production-dataset.sh [OPTIONS]
#
#   --repo-root DIR       Repo checkout to read ontology/snapshot/provenance
#                          files from (default: this script's own repo).
#   --out FILE             Where to (atomically) write the merged dataset
#                          (default: <repo-root>/data/dataset.ttl).
#   --skip-regenerate      Reuse the already-checked-in
#                          backlog/examples/snapshot.ttl instead of running
#                          `backlog-regenerate` (which needs `gh` CLI auth
#                          and live network access) -- lets this script be
#                          tested without network, and lets a
#                          provenance-only refresh skip hitting GitHub
#                          again.
#   --no-restart            Skip the `docker compose restart dagalog` step
#                          at the end -- for tests/dry runs, or when the
#                          caller wants to restart on its own schedule.
#   --compose-file FILE     Compose file to restart against (default:
#                          <repo-root>/deploy/docker-compose.public.yml).
#   --env-file FILE         Env file passed to `docker compose --env-file`
#                          (default: <repo-root>/deploy/.env).
#   --print-sources         Print the resolved list of source files (one
#                          per line: the two vocab files, the checked-in
#                          backlog/examples/snapshot.ttl -- the same file a
#                          live regeneration would refresh in place, so
#                          this reflects its *static* location, not a
#                          scratch regeneration path -- and every
#                          provenance/summaries/*.ttl) and exit, without
#                          writing or restarting anything. This is
#                          deliberately the SAME four-part shape
#                          scripts/serve-backlog.sh --print-data-args
#                          resolves for local dev, so a test can assert the
#                          two never silently diverge.
#
# On any failure before the final rename, the existing --out file is left
# completely untouched -- this script only ever replaces it via an atomic
# rename of a fully-written temp file in the same directory, never a
# partial write in place. A failed `backlog-regenerate` (e.g. `gh api`
# down) therefore never corrupts or truncates a working production
# dataset; the script just exits non-zero and the next scheduled run tries
# again.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
OUT=""
SKIP_REGENERATE=0
NO_RESTART=0
COMPOSE_FILE=""
ENV_FILE=""
PRINT_SOURCES=0

while [ $# -gt 0 ]; do
  case "$1" in
    --repo-root) REPO_ROOT="$2"; shift 2 ;;
    --out) OUT="$2"; shift 2 ;;
    --skip-regenerate) SKIP_REGENERATE=1; shift ;;
    --no-restart) NO_RESTART=1; shift ;;
    --compose-file) COMPOSE_FILE="$2"; shift 2 ;;
    --env-file) ENV_FILE="$2"; shift 2 ;;
    --print-sources) PRINT_SOURCES=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 1 ;;
  esac
done

[ -n "$OUT" ] || OUT="$REPO_ROOT/data/dataset.ttl"
[ -n "$COMPOSE_FILE" ] || COMPOSE_FILE="$REPO_ROOT/deploy/docker-compose.public.yml"
[ -n "$ENV_FILE" ] || ENV_FILE="$REPO_ROOT/deploy/.env"

VOCAB_FILES=(
  "$REPO_ROOT/backlog/ontology/vocabulary.ttl"
  "$REPO_ROOT/backlog/ontology/agentprov-vocabulary.ttl"
)

if [ "$PRINT_SOURCES" -eq 1 ]; then
  printf '%s\n' "${VOCAB_FILES[@]}"
  printf '%s\n' "$REPO_ROOT/backlog/examples/snapshot.ttl"
  shopt -s nullglob
  for f in "$REPO_ROOT"/provenance/summaries/*.ttl; do
    printf '%s\n' "$f"
  done
  shopt -u nullglob
  exit 0
fi

WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT

if [ "$SKIP_REGENERATE" -eq 1 ]; then
  SNAPSHOT_FILE="$REPO_ROOT/backlog/examples/snapshot.ttl"
else
  SNAPSHOT_FILE="$WORKDIR/snapshot.ttl"
  echo "regenerating backlog snapshot -> $SNAPSHOT_FILE" >&2
  cargo run --manifest-path "$REPO_ROOT/Cargo.toml" --release -q \
    -p backlog --bin backlog-regenerate -- --out "$SNAPSHOT_FILE"
fi

MERGED_TMP="$(dirname "$OUT")/.$(basename "$OUT").tmp"
mkdir -p "$(dirname "$OUT")"
: > "$MERGED_TMP"
for f in "${VOCAB_FILES[@]}" "$SNAPSHOT_FILE"; do
  cat "$f" >> "$MERGED_TMP"
  echo >> "$MERGED_TMP"
done
shopt -s nullglob
for f in "$REPO_ROOT"/provenance/summaries/*.ttl; do
  cat "$f" >> "$MERGED_TMP"
  echo >> "$MERGED_TMP"
done
shopt -u nullglob

mv "$MERGED_TMP" "$OUT"
echo "wrote $OUT" >&2

if [ "$NO_RESTART" -eq 0 ]; then
  echo "restarting dagalog via $COMPOSE_FILE" >&2
  docker compose -f "$COMPOSE_FILE" --env-file "$ENV_FILE" restart dagalog
fi
