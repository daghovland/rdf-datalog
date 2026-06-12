# Frontend / Web UI Improvement Plan

This document plans improvements to the Dagalog web UI served by the `sparql_endpoint` crate.
The current UI (`sparql_endpoint/src/frontend.html`) is a single self-contained HTML file with
an inline script and style.
All improvements described here work within that architecture (no build tooling, no npm).

---

## Current state

| Feature | Status |
|---|---|
| SPARQL SELECT via POST | ✓ |
| Results rendered as HTML table | ✓ |
| URIs in results link to resource browser | ✓ |
| Prefix shortening in results (with full-IRI tooltip) | ✓ |
| Prefix manager (localStorage, collapsible) | ✓ |
| Auto-prepend prefixes to submitted queries | ✓ |
| Query history (localStorage, max 50) | ✓ |
| Triple count in header | ✓ |
| Turtle upload | ✓ |
| Resource browser (`/?resource=<iri>`) | ✓ |
| `rdfs:label` as resource heading | ✓ |
| `rdf:type` class badges on resource page | ✓ |
| Collapsible outgoing / incoming edge tables | ✓ |
| Graph view tab (Cytoscape.js, 3-variable queries) | ✓ |
| Class hierarchy view (`/classes`) | ✗ |
| DESCRIBE query | ✗ (parser not implemented) |

---

## P2 — Graph visualisation (remaining)

### Class hierarchy view

A dedicated panel or page (`/classes`) that runs:
```sparql
SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }
```
and renders a collapsible tree using Cytoscape's `dagre` layout or a simple `<details>`-based
HTML tree. This is particularly useful for OWL ontologies like the Gene Ontology where the
class hierarchy is the primary structure.

`dagre` layout requires the `cytoscape-dagre` plugin (also CDN-embeddable) plus `dagre` itself:
```html
<script src="https://cdn.jsdelivr.net/npm/dagre@0.8.5/dist/dagre.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/cytoscape-dagre@2.5.0/cytoscape-dagre.js"></script>
```

---

## P3 — Query experience improvements

### Syntax highlighting in the textarea

Replace the plain `<textarea>` with **CodeMirror 6** (MIT, CDN-embeddable).  Provides:
- SPARQL keyword highlighting.
- Bracket matching, auto-indent.
- Ctrl+Enter to run the query.
- Much better editing experience for multi-line queries.

CDN import via `<script type="module">` — no build step needed.

### Query templates / examples dropdown

A `<select>` dropdown above the textarea with named example queries:

| Label | Query |
|---|---|
| All triples (LIMIT 10) | `SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10` |
| All classes | `SELECT DISTINCT ?c WHERE { ?c a owl:Class }` |
| Class hierarchy | `SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }` |
| Instances of class… | `SELECT ?i WHERE { ?i a <CLASS> }` |
| Labels | `SELECT ?s ?label WHERE { ?s rdfs:label ?label } LIMIT 50` |

Selecting an entry fills the textarea.

### Keyboard shortcut

`Ctrl+Enter` (or `Cmd+Enter` on Mac) runs the query. CodeMirror makes this trivial;
with a plain textarea it requires a `keydown` handler.

### Result export

"Download CSV" and "Download JSON" buttons appear beneath the result table.
The CSV is assembled client-side from the `bindings` array (no server changes needed).

---

## P4 — Upload UX improvements

### Drag-and-drop file upload

Allow dragging a `.ttl`, `.owl`, or `.jsonld` file onto the upload card. The file content
fills the textarea (small files) or is POSTed directly to `/upload` (large files via
`FormData`). Content-type is inferred from the file extension.

### Named-graph upload

Add a "Target graph" input (default: default graph). Passes a `graph=<iri>` query parameter
to `/upload`, which the server routes to the Graph Store Protocol PUT/POST handler on the
appropriate named graph.

---

## Implementation order (remaining)

| Step | Work | Complexity |
|---|---|---|
| 8 | Class hierarchy page `/classes` (P2) | ~60 lines JS |
| 9 | CodeMirror query editor (P3) | ~40 lines JS + CDN import |
| 10 | Query templates dropdown (P3) | ~30 lines JS |
| 11 | Result CSV/JSON export (P3) | ~30 lines JS |
| 12 | Drag-and-drop upload (P4) | ~50 lines JS |

All steps require only frontend changes (no server-side work).
The single-file architecture (`frontend.html` with inline CSS/JS) stays intact throughout.
