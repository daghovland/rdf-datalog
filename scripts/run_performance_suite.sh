#!/usr/bin/env bash
# Run the full ignored performance-test suite and Criterion benchmarks in one
# shot, capturing system info, wall time, and peak RSS for every test/bench
# to a single durable timestamped log file.
#
# Intended use: a short-lived, larger rented cloud box (e.g. a Hetzner
# instance) spun up specifically to run the large-ontology tests/benchmarks
# that don't fit on the normal dev box's RAM. Since the box is short-lived
# and expensive, results must be captured completely on the first run.
#
# Linux-only: relies on GNU `/usr/bin/time -v` for peak-RSS reporting.
# macOS's BSD `time` does not support `-v` and this script will refuse to
# run rather than silently produce a log with no memory figures.
#
# Usage:
#   bash scripts/run_performance_suite.sh
#
# Output:
#   perf-run-<timestamp>.log   in the repo root (gitignored) — a single file
#                               to scp off the box before teardown.
#
# NOT captured by this script (copy separately before teardown):
#   target/criterion/           Criterion's HTML benchmark reports. This
#                                lives in the shared CARGO_TARGET_DIR and
#                                will not survive box teardown.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ── Preconditions ────────────────────────────────────────────────────────────

if ! /usr/bin/time -v true >/dev/null 2>/tmp/perf_suite_time_check.$$; then
    echo "ERROR: '/usr/bin/time -v' is not available or not supported on this system." >&2
    echo "This script requires GNU time (Linux). macOS's built-in BSD 'time' does not" >&2
    echo "support '-v' and cannot report peak RSS. Install GNU time (e.g. 'apt install" >&2
    echo "time' on Debian/Ubuntu) or run this script on the Linux rental box it was" >&2
    echo "written for." >&2
    rm -f /tmp/perf_suite_time_check.$$
    exit 1
fi
rm -f /tmp/perf_suite_time_check.$$

# ── 1. Ensure test data is present ──────────────────────────────────────────

TESTDATA_DIR="$REPO_ROOT/tests/testdata"
REQUIRED_FILES=("go.ttl" "imf.ttl" "wikidata-sample.nt")

missing=0
for f in "${REQUIRED_FILES[@]}"; do
    if [ ! -f "$TESTDATA_DIR/$f" ]; then
        missing=1
        break
    fi
done

if [ "$missing" -eq 1 ]; then
    echo "Some test data files are missing from $TESTDATA_DIR — running download_test_ontologies.sh …"
    bash "$REPO_ROOT/scripts/download_test_ontologies.sh"
else
    echo "All expected test data files already present in $TESTDATA_DIR, skipping download."
fi

# ── 2. Set up the timestamped log file ──────────────────────────────────────

TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_FILE="$REPO_ROOT/perf-run-${TIMESTAMP}.log"

{
    echo "==================================================================="
    echo "Performance suite run: $TIMESTAMP"
    echo "==================================================================="
    echo
    echo "--- uname -a ---"
    uname -a
    echo
    echo "--- nproc ---"
    nproc
    echo
    echo "--- free -h ---"
    free -h
    echo
    echo "--- df -h ---"
    df -h
    echo
    echo "--- rustc --version ---"
    rustc --version
    echo
    echo "--- cargo --version ---"
    cargo --version
    echo
    echo "--- git rev-parse HEAD ---"
    git rev-parse HEAD
    echo
    echo "==================================================================="
    echo
} | tee "$LOG_FILE"

# ── 3 & 4. Run tests and benchmarks, each wrapped in /usr/bin/time -v ───────
# Every command's stdout+stderr (which includes the `time -v` report on
# stderr) is teed to the log file, so the log is self-contained even if the
# operator loses the terminal scrollback.

run_step() {
    local description="$1"
    shift
    {
        echo
        echo "==================================================================="
        echo "STEP: $description"
        echo "Command: $*"
        echo "Started: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
        echo "==================================================================="
        echo
    } | tee -a "$LOG_FILE"

    # Run under `time -v`, teeing combined output to the log. Don't let a
    # failing test/bench abort the whole suite — we want as much captured as
    # possible from a single, expensive rental run.
    set +e
    /usr/bin/time -v "$@" 2>&1 | tee -a "$LOG_FILE"
    local status="${PIPESTATUS[0]}"
    set -e

    {
        echo
        echo "Finished: $(date -u +%Y-%m-%dT%H:%M:%SZ)  (exit status: $status)"
        echo
    } | tee -a "$LOG_FILE"
}

run_step "cargo test --release --test performance -- --ignored --nocapture" \
    cargo test --release --test performance -- --ignored --nocapture

run_step "cargo bench --bench lubm" \
    cargo bench --bench lubm

run_step "cargo bench --bench gene_ontology" \
    cargo bench --bench gene_ontology

# ── 5. Final reminder ────────────────────────────────────────────────────────

{
    echo
    echo "==================================================================="
    echo "Performance suite run complete."
    echo "Log file: $LOG_FILE"
    echo
    echo "REMINDER: copy target/criterion/ off this box separately before"
    echo "teardown -- the Criterion HTML benchmark reports are NOT included"
    echo "in this log file and live in the shared CARGO_TARGET_DIR, which"
    echo "will not survive teardown."
    echo "==================================================================="
} | tee -a "$LOG_FILE"

echo
echo "Done. scp this file off the box: $LOG_FILE"
