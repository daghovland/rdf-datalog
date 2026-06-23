# Visual Query Builder — Remaining Work

An OptiqueVQS-style graphical query builder accessible at `/?view=build`.
Phases 1–3 are fully implemented (class picker, data properties, object-property multi-hop,
data-property filters). This document tracks what remains.

---

## Implemented (reference)

| Phase | Scope | Status |
|---|---|---|
| 1 | Class picker, data-property checkboxes, live SPARQL preview, Run | Done |
| 2 | Object-property multi-hop, multi-card canvas with arrows, node removal | Done |
| 3 | Per-property filter input, `FILTER(regex(...))` in generated SPARQL | Done |
| — | Custom autocomplete class picker (filter by short name or full IRI) | Done |
| — | "Build query" link from resource-page type badges | Done |

---

## Phase 4 — Cytoscape canvas (optional)

Replace the horizontal card layout with a Cytoscape.js graph where:
- Each node card is a Cytoscape node rendered with `cytoscape-node-html-label`
  (or styled boxes as a simpler alternative)
- Object-property links are Cytoscape edges
- Clicking a node activates it (property pane follows)

Cytoscape.js is already integrated for the graph-view tab, so the CDN load is already
present.  The main new requirement is the HTML-label plugin, or an alternative that
renders rich card content inside nodes.

**Decision point:** if the plugin adds significant complexity, keep Phase 2's plain-HTML
canvas and use Cytoscape only for the existing result graph view.

Estimated complexity: ~100 lines JS if the HTML-label plugin works cleanly.

---

## Bug: class tree leaks OWL restriction blank nodes

**Symptom** (reported against `tests/testdata/imf.ttl`, a real OWL ontology): the class
picker (`#qb-class-tree`, built by `loadQbClassTree()`) lists dozens of blank-node IDs
(`b1`, `b2`, …) alongside real named classes.

**Root cause**: `loadQbClassTree()` runs
`SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }` and treats every
`?child`/`?parent` binding as a candidate class, without filtering to IRIs. OWL ontologies
routinely use anonymous `owl:Restriction` expressions as `rdfs:subClassOf` targets (e.g.
`ex:Person rdfs:subClassOf [ a owl:Restriction ; owl:onProperty … ]`) — these are not
classes a user can build a query against, but they pass through unfiltered and can even
become tree *roots* (if the restriction blank node is never itself a `?child`).
Confirmed against `imf.ttl`: the root set the buggy query produces is exactly 27 blank
node IDs plus one real class — matching the reported symptom exactly.

Contrast: `loadQbClassFlat()` (the no-hierarchy fallback) already filters
`b.c?.type === 'uri'`. The tree-building path is just missing the equivalent filter.

**Fix**: add `FILTER(isIRI(?child) && isIRI(?parent))` to the tree query in
`loadQbClassTree()` so restriction blank nodes are dropped before the root/children
computation runs, instead of leaking into the picker.

**Status**: fixed. **Tests**: `qb_class_tree_excludes_owl_restriction_blank_nodes` in
`tests/frontend_browser.rs`, using a minimal fixture (`QB_BLANK_NODE_FIXTURE`) with one named
class whose only superclass is an OWL restriction blank node, plus a second named subclass to
confirm normal hierarchy still nests correctly once the blank-node edge is filtered out.

---

## Executor constraints

These limit what the builder can safely generate today.  Track them when adding new
filter types or discovery queries.

| Feature | Status |
|---|---|
| Basic graph patterns | ✓ Executed |
| OPTIONAL | ✓ Executed |
| FILTER (comparisons, `regex()`, `lang()`, `bound()`, `EXISTS`/`NOT EXISTS`) | ✓ Executed |
| DISTINCT, LIMIT, OFFSET | ✓ Executed |
| ORDER BY | Parsed; silently ignored in executor |
| COUNT / aggregates | Parsed; not yet executed |
| FILTER inside OPTIONAL | **Not executed** — builder emits required triple + top-level FILTER instead |

---

## VQS productive-extension index (wired into the builder UI)

`GET /vqs/productive-values?class=<IRI>&property=<IRI>` (see `sparql_endpoint/src/vqs_routes.rs`)
returns the productive values of a data property for instances of a class, computed from a
precomputed index instead of a live SPARQL query. Implemented per
`docs/plans/VQS_INDEX_PLAN.md` (all 7 phases complete, `vqs_index` crate).

Wired into `renderQbDataProps()` in `frontend.html`: checking a data property's checkbox fetches
this endpoint once and caches the result on the property's state object. The filter input gets a
`<datalist>` of known values for autocomplete, plus a live hint — "N known value(s)" normally,
or a red dead-end warning when the typed text doesn't match any known value (pure logic in
`filterValueIsProductive()`, mirrored by `filter_value_is_productive()` in `query_builder.rs`,
per the project's Rust↔JS test-mirroring convention). When the index doesn't cover the
(class, property) pair the hint is hidden — not shown as an error.

Requires `rdfs:domain`/`rdfs:range` declared on the properties being indexed; pure instance data
with no schema triples yields an empty navigation graph and every lookup reports `covered: false`.

Tests: `qb_checking_covered_data_prop_shows_known_value_count` and
`qb_typing_unproductive_filter_value_shows_warning` in `tests/frontend_browser.rs`.

---

## Running tests

```bash
# Layers 1 + 2 (no geckodriver needed)
cargo test -p sparql-endpoint

# Layer 3 (geckodriver required; tests skip gracefully without it)
geckodriver --port 4444 &
cargo test --test frontend_browser -p sparql-endpoint
```
