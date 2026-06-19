#!/usr/bin/env bash
# Download large ontology files used by the performance integration tests.
#
# These files are NOT committed to the repository because of their size.
# Run this script once before running the ignored performance tests:
#
#   bash scripts/download_test_ontologies.sh
#   cargo test --test performance -- --ignored

set -euo pipefail

DEST="tests/testdata"
mkdir -p "$DEST"

# ── Gene Ontology ────────────────────────────────────────────────────────────
# Source: https://geneontology.org/docs/download-ontology/
# The OWL/XML release is converted to Turtle via Apache Jena's riot tool.
# If riot is not available, the Turtle release can be downloaded directly.
GO_TTL="$DEST/go.ttl"

if [ -f "$GO_TTL" ]; then
    echo "go.ttl already present, skipping download."
else
    # Try the Turtle release first (no riot required)
    GO_TTL_URL="https://current.geneontology.org/ontology/go.owl"
    GO_OWL_XML="$DEST/go.owl.xml"

    echo "Downloading Gene Ontology OWL/XML …"
    curl -fL --progress-bar -o "$GO_OWL_XML" "$GO_TTL_URL"

    if command -v riot &>/dev/null; then
        echo "Converting OWL/XML → Turtle with riot …"
        riot --output=TURTLE "$GO_OWL_XML" > "$GO_TTL"
        rm -f "$GO_OWL_XML"
        echo "go.ttl written."
    else
        echo ""
        echo "WARNING: 'riot' (Apache Jena) not found."
        echo "  Install it from https://jena.apache.org/download/ and re-run, OR"
        echo "  download the Turtle release directly:"
        echo "    curl -fL -o $GO_TTL https://current.geneontology.org/ontology/go.owl"
        echo "  (some releases expose a .ttl, check https://current.geneontology.org/ontology/)"
        echo ""
        echo "go.owl.xml saved to $GO_OWL_XML — convert manually."
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

echo "Done."
echo ""
echo "Run IMF tests (no --ignored needed):"
echo "  cargo test --test performance imf -- --nocapture"
echo ""
echo "Run Gene Ontology tests (still ignored — large file):"
echo "  cargo test --test performance gene_ontology -- --ignored --nocapture"
echo ""
echo "Run Gene Ontology benchmarks:"
echo "  cargo bench --bench gene_ontology"
echo ""
echo "Compare bench against a saved baseline:"
echo "  cargo bench --bench gene_ontology -- --save-baseline before"
echo "  # … make your change …"
echo "  cargo bench --bench gene_ontology -- --baseline before"
