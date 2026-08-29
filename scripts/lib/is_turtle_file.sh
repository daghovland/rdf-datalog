#!/usr/bin/env bash
# is_turtle_file <path> — sanity-checks that a file looks like Turtle rather
# than RDF/XML (or another format masquerading with a .ttl extension).
#
# This exists because of https://github.com/daghovland/rdf-datalog/issues/569:
# scripts/download_test_ontologies.sh used to instruct users (in its riot-not-
# found fallback message) to save the Gene Ontology's OWL/XML release directly
# as go.ttl. The Turtle parser then failed with a confusing
# "Invalid IRI code point ' '" error at the very start of the file — the
# literal bytes of an XML declaration ("<?xml version=\"1.0\"?>") being
# misread as a malformed Turtle IRIREF.
#
# A real Turtle file — including riot's own SPARQL-style
# "BASE <...>" / "PREFIX p: <...>" output — never starts with an XML
# declaration, a DOCTYPE, or an RDF/XML root element, so rejecting those is an
# unambiguous, dependency-free check that catches this failure mode (and any
# other "wrong format saved with a .ttl name") before it reaches the parser.
#
# Returns 0 (true) if the file exists, is non-empty, and does not look like
# XML; returns 1 (false) otherwise.
is_turtle_file() {
    local file="$1"
    [ -s "$file" ] || return 1
    local head
    head="$(head -c 64 "$file")"
    case "$head" in
        '<?xml'*|'<!DOCTYPE'*|*'<rdf:RDF'*) return 1 ;;
        *) return 0 ;;
    esac
}
