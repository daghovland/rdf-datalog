#!/usr/bin/env bash
# Regression test for scripts/lib/is_turtle_file.sh, covering the exact
# corruption this codebase hit in
# https://github.com/daghovland/rdf-datalog/issues/569: `go.ttl` silently
# containing RDF/XML (an OWL/XML ontology release saved under the wrong
# filename), which the Turtle parser then failed on with a confusing
# "Invalid IRI code point ' '" error instead of a clear diagnostic.
#
# Run directly:
#   bash scripts/test_is_turtle_file.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/is_turtle_file.sh
source "$SCRIPT_DIR/lib/is_turtle_file.sh"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

FAILURES=0

assert_true() {
    local desc="$1" file="$2"
    if is_turtle_file "$file"; then
        echo "ok - $desc"
    else
        echo "NOT OK - $desc (expected is_turtle_file to accept $file)"
        FAILURES=$((FAILURES + 1))
    fi
}

assert_false() {
    local desc="$1" file="$2"
    if is_turtle_file "$file"; then
        echo "NOT OK - $desc (expected is_turtle_file to reject $file)"
        FAILURES=$((FAILURES + 1))
    else
        echo "ok - $desc"
    fi
}

# ── Real Turtle content is accepted ──────────────────────────────────────────

printf '@prefix : <http://example.org/> .\n:a :b :c .\n' > "$TMP_DIR/classic.ttl"
assert_true "classic @prefix/@base Turtle is accepted" "$TMP_DIR/classic.ttl"

# riot's actual --output=TURTLE format: SPARQL-style BASE/PREFIX, no leading
# '@', no trailing '.'. This is the exact first line dagalog's own oxttl-based
# parser was verified (during the #569 investigation) to parse without error.
printf 'BASE   <http://purl.obolibrary.org/obo/go.owl>\nPREFIX : <http://purl.obolibrary.org/obo/go.owl#>\n:x a :Foo .\n' > "$TMP_DIR/riot-style.ttl"
assert_true "riot's SPARQL-style BASE/PREFIX Turtle is accepted" "$TMP_DIR/riot-style.ttl"

# ── The actual #569 corruption: RDF/XML (OWL/XML) saved as .ttl ─────────────

printf '<?xml version="1.0"?>\n<rdf:RDF xmlns="http://purl.obolibrary.org/obo/go.owl#">\n' > "$TMP_DIR/go-owl-xml-as-ttl.ttl"
assert_false "OWL/XML with an XML declaration is rejected" "$TMP_DIR/go-owl-xml-as-ttl.ttl"

printf '<!DOCTYPE rdf:RDF [\n<!ENTITY go "http://purl.obolibrary.org/obo/GO_">\n]>\n' > "$TMP_DIR/doctype.ttl"
assert_false "a DOCTYPE preamble is rejected" "$TMP_DIR/doctype.ttl"

printf '<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">\n' > "$TMP_DIR/bare-rdf-root.ttl"
assert_false "a bare <rdf:RDF> root element is rejected" "$TMP_DIR/bare-rdf-root.ttl"

# ── Edge cases ────────────────────────────────────────────────────────────────

: > "$TMP_DIR/empty.ttl"
assert_false "an empty file is rejected" "$TMP_DIR/empty.ttl"

assert_false "a missing file is rejected" "$TMP_DIR/does-not-exist.ttl"

if [ "$FAILURES" -eq 0 ]; then
    echo "All is_turtle_file tests passed."
    exit 0
else
    echo "$FAILURES test(s) failed."
    exit 1
fi
