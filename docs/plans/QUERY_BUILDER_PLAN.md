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

## VQS productive-extension index (backend, not yet wired to the UI)

`GET /vqs/productive-values?class=<IRI>&property=<IRI>` (see `sparql_endpoint/src/vqs_routes.rs`)
returns the productive values of a data property for instances of a class, computed from a
precomputed index instead of a live SPARQL query. Implemented per
`docs/plans/VQS_INDEX_PLAN.md` (all 7 phases complete, `vqs_index` crate).

**Not yet called by the frontend.** The builder's data-property filter inputs (Phase 3 above)
still let the user type any value, even ones that would return zero rows — the dead-end problem
the VQS paper addresses. Wiring this endpoint in (e.g. to grey out / autocomplete unproductive
filter values) is unimplemented frontend work, tracked as a future phase here.

Requires `rdfs:domain`/`rdfs:range` declared on the properties being indexed; pure instance data
with no schema triples yields an empty navigation graph and every lookup reports `covered: false`.

---

## Running tests

```bash
# Layers 1 + 2 (no geckodriver needed)
cargo test -p sparql-endpoint

# Layer 3 (geckodriver required; tests skip gracefully without it)
geckodriver --port 4444 &
cargo test --test frontend_browser -p sparql-endpoint
```
