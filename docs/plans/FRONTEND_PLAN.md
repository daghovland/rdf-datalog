# Frontend / Web UI Improvement Plan

This document plans improvements to the Dagalog web UI served by the `sparql_endpoint` crate.
The current UI (`sparql_endpoint/src/frontend.html`) is a single self-contained HTML file with
an inline script and style.
All improvements described here work within that architecture (no build tooling, no npm).

---

## Current state (all implemented)

| Feature | Status |
|---|---|
| SPARQL SELECT via POST | ✓ |
| Results rendered as HTML table | ✓ |
| URIs in results link to resource browser | ✓ |
| Prefix shortening in results (with full-IRI tooltip) | ✓ |
| Prefix manager (localStorage, collapsible) | ✓ |
| Auto-prepend prefixes to submitted queries | ✓ |
| Query templates dropdown | ✓ |
| Ctrl+Enter keyboard shortcut | ✓ |
| Query history (localStorage, max 50) | ✓ |
| Result CSV export | ✓ |
| Result JSON export | ✓ |
| Triple count in header | ✓ |
| Turtle upload | ✓ |
| Drag-and-drop file upload (.ttl/.owl/.jsonld) | ✓ |
| Resource browser (`/?resource=<iri>`) | ✓ |
| `rdfs:label` as resource heading | ✓ |
| `rdf:type` class badges on resource page | ✓ |
| Collapsible outgoing / incoming edge tables | ✓ |
| Graph view tab (Cytoscape.js, 3-variable queries) | ✓ |
| Class hierarchy view (`/?view=classes`) | ✓ |

---

## Possible future improvements

### Syntax highlighting (P3 stretch goal)

Replace the plain `<textarea>` with **CodeMirror 6** (MIT, CDN-embeddable).
Would provide SPARQL keyword highlighting, bracket matching, and better multi-line editing.
CDN import via `<script type="module">` — no build step needed.

### Class hierarchy with dagre layout

The current class hierarchy uses a `<details>`-based collapsible tree.
A richer Cytoscape + `dagre` layout would give a proper hierarchical graph view.
Requires two additional CDN scripts:

```html
<script src="https://cdn.jsdelivr.net/npm/dagre@0.8.5/dist/dagre.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/cytoscape-dagre@2.5.0/cytoscape-dagre.js"></script>
```

### Named-graph upload target

Add a "Target graph" input to the upload panel. Passes a `graph=<iri>` query parameter
to `/upload`, routing to the Graph Store Protocol PUT/POST handler for a named graph.

### DESCRIBE query support

Requires implementing DESCRIBE in the SPARQL parser. Would let the resource browser
use `DESCRIBE <iri>` instead of two manual SELECT queries.

### Graph view: save layout

After nodes are dragged to preferred positions, the layout should be saveable
so it can be restored on the next visit for the same query.

**Plan:**
1. After any node drag ends (`cyInstance.on('dragfree', 'node', ...)`), collect
   `{ id, x, y }` positions from `cyInstance.nodes().map(n => ({ id: n.id(), ...n.position() }))`.
2. Persist under a localStorage key derived from the query string
   (e.g. `dagalog-layout-<sha1-or-truncated-hash-of-query>`).
3. On `renderGraph`, after `cytoscape({...})` returns, check localStorage for a saved layout
   matching the current query. If found, apply positions with `cy.nodes().forEach(n => n.position(savedPos[n.id()]))` 
   and call `cy.fit()` to adjust viewport.
4. Add a "Reset layout" button in `.cy-toolbar` that discards the saved layout and re-runs
   the cose layout via `cyInstance.layout({ name: 'cose', ... }).run()`.
5. Optionally: a "Save PNG" button using `cyInstance.png({ output: 'blob' })` + `triggerDownload`.
