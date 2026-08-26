#!/usr/bin/env bash
# Download large ontology files used by the performance integration tests.
#
# These files are NOT committed to the repository because of their size.
# Run this script once before running the ignored performance tests:
#
#   bash scripts/download_test_ontologies.sh
#   cargo test --test performance -- --ignored

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/is_turtle_file.sh
source "$SCRIPT_DIR/lib/is_turtle_file.sh"

DEST="tests/testdata"
mkdir -p "$DEST"

# ── Gene Ontology ────────────────────────────────────────────────────────────
# Source: https://geneontology.org/docs/download-ontology/
# There is no direct Turtle release: https://current.geneontology.org/ontology/go.ttl
# and http://purl.obolibrary.org/obo/go.ttl both 403/404 as of writing. The only
# release available is OWL/XML (go.owl, despite the .owl extension — it is NOT
# Turtle), which must be converted to Turtle via Apache Jena's riot tool.
#
# go.ttl is validated before being accepted (see is_turtle_file below) so a
# corrupted or half-written file — e.g. a riot crash, or (the historical bug,
# https://github.com/daghovland/rdf-datalog/issues/569) go.owl.xml mistakenly
# saved as go.ttl — cannot get silently reused by a later "already present" skip.
GO_TTL="$DEST/go.ttl"

if [ -f "$GO_TTL" ]; then
    if is_turtle_file "$GO_TTL"; then
        echo "go.ttl already present, skipping download."
    else
        echo "ERROR: $GO_TTL exists but does not look like Turtle (starts with XML)."
        echo "  This is the failure mode of https://github.com/daghovland/rdf-datalog/issues/569 —"
        echo "  delete it and re-run this script:"
        echo "    rm $GO_TTL && bash $0"
        exit 1
    fi
else
    GO_OWL_XML_URL="https://current.geneontology.org/ontology/go.owl"
    GO_OWL_XML="$DEST/go.owl.xml"
    GO_TTL_TMP="$DEST/.go.ttl.tmp"

    echo "Downloading Gene Ontology OWL/XML …"
    curl -fL --progress-bar -o "$GO_OWL_XML" "$GO_OWL_XML_URL"

    if command -v riot &>/dev/null; then
        echo "Converting OWL/XML → Turtle with riot …"
        rm -f "$GO_TTL_TMP"
        riot --output=TURTLE "$GO_OWL_XML" > "$GO_TTL_TMP"
        if ! is_turtle_file "$GO_TTL_TMP"; then
            echo "ERROR: riot produced output that doesn't look like Turtle."
            rm -f "$GO_TTL_TMP"
            exit 1
        fi
        # Move into place only once conversion is known-good, so a riot crash or
        # a bad conversion never leaves a corrupt file at $GO_TTL for a later run
        # to silently accept via the "already present" check above.
        mv "$GO_TTL_TMP" "$GO_TTL"
        rm -f "$GO_OWL_XML"
        echo "go.ttl written."
    else
        echo ""
        echo "WARNING: 'riot' (Apache Jena) not found."
        echo "  Install it from https://jena.apache.org/download/ and re-run."
        echo "  There is no direct Turtle release to fall back to — the only"
        echo "  published Gene Ontology release is OWL/XML, which riot must convert."
        echo "  Do NOT copy/rename go.owl(.xml) to go.ttl — it is RDF/XML, not"
        echo "  Turtle, and the Turtle parser will fail on it "
        echo "  (see https://github.com/daghovland/rdf-datalog/issues/569)."
        echo ""
        echo "go.owl.xml saved to $GO_OWL_XML — convert manually with riot once installed."
        exit 1
    fi
fi

# ── IMF ontology ─────────────────────────────────────────────────────────────
# Information Modeling Framework (IMF) ontology used for end-to-end pipeline
# tests.  Replaces storing the pre-generated large.datalog in the repo:
# the tests generate the Datalog rules from the ontology on the fly.
#
# Set IMF_TTL_URL to the actual URL before running, or place imf.ttl manually.
# Example (Equinor internal or public READI release):
IMF_TTL_URL="https://gitlab.com/imf-lab/spec/imf-ontology/-/raw/develop/owl/imf-ontology.owl.ttl?inline=false"
#
IMF_TTL="$DEST/imf.ttl"

if [ -f "$IMF_TTL" ]; then
    echo "imf.ttl already present, skipping download."
elif [ -n "${IMF_TTL_URL:-}" ]; then
    echo "Downloading IMF ontology from $IMF_TTL_URL …"
    curl -fL --progress-bar -o "$IMF_TTL" "$IMF_TTL_URL"
    echo "imf.ttl written."
else
    echo ""
    echo "NOTE: IMF ontology (imf.ttl) not downloaded."
    echo "  Set IMF_TTL_URL to download it, or copy imf.ttl to $IMF_TTL manually."
    echo "  IMF pipeline tests will be skipped without this file."
    echo ""
fi

# ── Wikidata N-Triples sample ─────────────────────────────────────────────────
# Source: https://dumps.wikimedia.org/wikidatawiki/entities/
# The truthy dump contains the best-rank (truthy) direct property statements.
# We stream just the first WIKIDATA_LINES lines so the on-disk file stays small.
# N-Triples is one triple per line, so any line boundary is a safe truncation point.
WIKIDATA_NT="$DEST/wikidata-sample.nt"
WIKIDATA_LINES=10000000  # ~10M triples ≈ 800–1000 MB uncompressed

if [ -f "$WIKIDATA_NT" ]; then
    echo "wikidata-sample.nt already present, skipping download."
else
    echo "Streaming Wikidata truthy N-Triples dump (first ${WIKIDATA_LINES} lines) …"
    echo "  Source: https://dumps.wikimedia.org/wikidatawiki/entities/latest-truthy.nt.gz"
    echo "  (Only the first ${WIKIDATA_LINES} lines are kept; the full dump is many GB.)"

    # head closes the pipe after N lines, which sends SIGPIPE to gzip/curl (exit 141).
    # That is expected — disable pipefail for this pipeline only.
    set +o pipefail
    curl -fL --no-progress-meter \
        "https://dumps.wikimedia.org/wikidatawiki/entities/latest-truthy.nt.gz" \
      | gzip -dc 2>/dev/null \
      | head -n "$WIKIDATA_LINES" > "$WIKIDATA_NT" || true
    set -o pipefail

    ACTUAL_LINES=$(wc -l < "$WIKIDATA_NT")
    if [ "$ACTUAL_LINES" -lt 10000 ]; then
        echo "ERROR: only ${ACTUAL_LINES} lines written — download may have failed."
        rm -f "$WIKIDATA_NT"
        exit 1
    fi
    echo "wikidata-sample.nt written (${ACTUAL_LINES} lines)."
fi

# ── DBLP N-Triples sample ────────────────────────────────────────────────────
# Source: https://dblp.org/rdf/dblp.nt.gz (main bibliography, no citations)
# This is genuine N-Triples (one triple per line), unlike dblp.ttl.gz which is
# pretty-printed multi-line Turtle and unsafe to truncate by line.
# We stream just the first DBLP_LINES lines so the on-disk file stays small.
DBLP_NT="$DEST/dblp-sample.nt"
DBLP_LINES=15000000  # ~15M triples, used by the dblp_benchmark.rs diagnostic suite

if [ -f "$DBLP_NT" ]; then
    echo "dblp-sample.nt already present, skipping download."
else
    echo "Streaming DBLP N-Triples dump (first ${DBLP_LINES} lines) …"
    echo "  Source: https://dblp.org/rdf/dblp.nt.gz"
    echo "  (Only the first ${DBLP_LINES} lines are kept; the full dump is ~5 GB compressed.)"

    # head closes the pipe after N lines, which sends SIGPIPE to gzip/curl (exit 141).
    # That is expected — disable pipefail for this pipeline only.
    set +o pipefail
    curl -fL --no-progress-meter \
        "https://dblp.org/rdf/dblp.nt.gz" \
      | gzip -dc 2>/dev/null \
      | head -n "$DBLP_LINES" > "$DBLP_NT" || true
    set -o pipefail

    ACTUAL_LINES=$(wc -l < "$DBLP_NT")
    if [ "$ACTUAL_LINES" -lt 10000 ]; then
        echo "ERROR: only ${ACTUAL_LINES} lines written — download may have failed."
        rm -f "$DBLP_NT"
        exit 1
    fi
    echo "dblp-sample.nt written (${ACTUAL_LINES} lines)."
fi

# ── LUBM (Lehigh University Benchmark) ───────────────────────────────────────
# LUBM data is generated synthetically by the Rust benchmark itself.
# No download is required. Run the LUBM benchmarks with:
#
#   cargo bench --bench lubm
#
# To compare BF vs. full re-materialisation across scales:
#
#   cargo bench --bench lubm -- bf_vs_full_remat
#
# For memory overhead numbers:
#
#   cargo bench --bench lubm -- memory_overhead
#
# To verify the benchmark compiles and runs without panicking (no data needed):
#
#   cargo bench --bench lubm -- --test

echo "Done."
echo ""
echo "Run IMF tests (no --ignored needed):"
echo "  cargo test --test performance imf -- --nocapture"
echo ""
echo "Run Gene Ontology tests (still ignored — large file):"
echo "  cargo test --test performance gene_ontology -- --ignored --nocapture"
echo ""
echo "Run Wikidata tests (ignored — large file):"
echo "  cargo test --test performance wikidata -- --ignored --nocapture"
echo ""
echo "Run DBLP benchmark diagnostic (ignored — large file):"
echo "  cargo test --test dblp_benchmark -- --ignored --nocapture"
echo ""
echo "Run Gene Ontology benchmarks:"
echo "  cargo bench --bench gene_ontology"
echo ""
echo "Run LUBM benchmarks (data is generated synthetically — no download needed):"
echo "  cargo bench --bench lubm"
echo "  cargo bench --bench lubm -- bf_vs_full_remat"
echo "  cargo bench --bench lubm -- memory_overhead"
echo ""
echo "Compare bench against a saved baseline:"
echo "  cargo bench --bench gene_ontology -- --save-baseline before"
echo "  # … make your change …"
echo "  cargo bench --bench gene_ontology -- --baseline before"
