# Frontend / Web UI Improvement Plan

This document plans improvements to the Dagalog web UI served by the `sparql_endpoint` crate.
The current UI (`sparql_endpoint/src/frontend.html`) is a single self-contained HTML file with
an inline script and style: a SPARQL query textarea, a results table, and a Turtle upload box.
All improvements described here work within that architecture (no build tooling, no npm).

---

## Current state

| Feature | Status |
|---|---|
| SPARQL SELECT via POST | ✓ |
| Results rendered as HTML table | ✓ |
| URIs shown as `<a href=uri target=_blank>` (external links) | ✓ (opens external page) |
| Turtle upload | ✓ |
| DESCRIBE query | ✗ (parser not implemented) |
| Prefix shortening in results | ✗ |
| Query history | ✗ |
| Graph visualisation | ✗ |

---

## P0 — Quick wins (minimal new code)

### IRI links → resource browser (the main ask)

**What to link to:**
Rather than opening the IRI as an external URL (which for ontology IRIs often returns nothing
useful), each IRI in results should link to a local resource-browser view. Three options in
order of preference:

1. **Pre-filled SELECT query (recommended for now)**
   Link to `/?query=<encoded>` where the query is:
   ```sparql
   SELECT ?p ?o WHERE { <IRI> ?p ?o }
   ```
   This reuses the existing query UI, requires no new server routes, and is immediately
   useful. The user can see all outgoing edges of the resource. The link pre-fills the
   textarea and auto-runs.

2. **Dedicated resource page `/resource?iri=<encoded>`** _(P1 item, see below)_
   A richer page showing outgoing edges, incoming backlinks (referencing subjects), and a
   human-readable label when `rdfs:label` is present. This is what Linked Data browsers
   (LodView, RDFBrowser) provide and is the natural long-term target.

3. **DESCRIBE query** _(requires implementing DESCRIBE in the SPARQL parser)_
   `GET /sparql?query=DESCRIBE+<IRI>` returns a Turtle/JSON-LD document. Most browsers
   would render it as text; it would need to be fetched and displayed. Lower priority since
   SELECT already covers the same data.

**Implementation of option 1 (P0):**
In `renderTable`, change the URI `<a>` element so that `href` points to `/?query=...` and
`target` is `_self`. When the page loads, check `location.search` for a `?query=` parameter;
if present, pre-fill the textarea and call `runQuery()` automatically.

### Prefix shortening in result cells

Long IRIs like `http://www.w3.org/2000/01/rdf-schema#label` should be displayed as `rdfs:label`
with the full IRI in a tooltip (`title` attribute). A small prefix table covering the common
vocabularies (rdf, rdfs, owl, xsd, skos, dc, schema) hardcoded in the JS is sufficient. The
cell `href` and the query pre-fill still use the full IRI.

```
http://www.w3.org/2000/01/rdf-schema#label  →  rdfs:label
http://www.w3.org/2002/07/owl#Class          →  owl:Class
```

---

## P1 — Resource browser

### New route: `GET /resource?iri=<encoded-iri>`

Returns an HTML page (or a fragment) that assembles two SELECT queries on the server or
client side:

| Section | Query |
|---|---|
| Label / comment | `SELECT ?label WHERE { <IRI> rdfs:label ?label }` |
| Outgoing edges | `SELECT ?p ?o WHERE { <IRI> ?p ?o } ORDER BY ?p` |
| Incoming edges | `SELECT ?s ?p WHERE { ?s ?p <IRI> } ORDER BY ?p LIMIT 200` |
| `rdf:type` / class membership | extracted from outgoing edges |

The page renders two collapsible tables (outgoing / incoming), with IRIs again as clickable
resource links, creating a web-of-data browsing experience. Prefix shortening applies
throughout.

This is the same pattern used by DBpedia's Linked Data views and by Virtuoso's `/describe`.
It turns the triplestore into a self-describing knowledge graph explorer without needing
DESCRIBE support in the parser.

**Server side:** add `GET /resource` to `server.rs`, handled by a new `resource_browser.rs`
that executes the two SELECT queries and fills an HTML template. Or implement entirely
client-side by fetching `/sparql` twice from JavaScript on the resource page.
Client-side is simpler (no new server handler, reuses existing query infrastructure).

### Query history (localStorage)

Each executed query is pushed to `localStorage['dagalog-history']` (array, max 50 entries).
A "History" disclosure below the textarea lists recent queries; clicking one restores it.
Entries store: query text, timestamp, row count from last run.

### Prefix manager

A collapsible "Prefixes" section above the query textarea.  Pre-populated with common
prefixes (rdf, rdfs, owl, xsd). The user can add/remove entries. Prefixes are persisted to
`localStorage` and prepended to every query before submission (only if not already present
in the query text). This mirrors the convenience that Yasgui and SPARQL Playground provide.

### Store statistics panel

A lightweight read on page load: `SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }` gives the
triple count. Display it in the header as "N triples loaded". Optionally also list named
graphs via `SELECT DISTINCT ?g WHERE { GRAPH ?g { ?s ?p ?o } }`.

---

## P2 — Graph visualisation

### Technology choice: Cytoscape.js

**Cytoscape.js** (MIT licence, CDN-embeddable, no build step) is the best fit:

- Nodes and edges can be added from SPARQL JSON results with minimal glue code.
- Several built-in layouts: `cose` (force-directed, good for general graphs), `dagre`
  (hierarchical, good for class/subClassOf trees), `cola` (constraint-based, good for
  medium-density graphs).
- Clicking nodes can trigger resource-browser navigation.
- Handles graphs up to ~1000 nodes interactively in the browser.
- Plugins are optional; the core is ~500 KB minified.

Alternatives considered:
- **D3.js** — much more powerful but requires writing the force simulation from scratch;
  steep learning curve for graph layout.
- **Vis.js Network** — simpler API but heavier bundle (~1 MB), less maintained.
- **Sigma.js** — WebGL-based, handles tens of thousands of nodes, but overkill here and
  harder to embed without a build step.
- **Graphviz WASM** — deterministic Sugiyama layouts (ideal for subClassOf trees) but
  ~4 MB WASM bundle and no interactivity.

### Graph view tab

When the query variables include `?s ?p ?o` (or any three-variable SELECT that looks like a
triple pattern), offer a **"Graph" tab** next to the existing "Table" tab. The graph view
renders:

- **Nodes**: every distinct IRI or blank-node value that appears in `?s` or `?o` columns.
- **Edges**: rows become directed edges `s → o` labelled with the shortened `?p` value.
- **Node labels**: `rdfs:label` if available (fetched lazily), else the shortened IRI.
- **Node colour**: distinguish by `rdf:type` (query for types lazily on click/hover).
- **Click a node**: navigates to the resource browser for that IRI.
- **Limit**: cap at 200 nodes; show a warning and a "LIMIT" control if the query returns more.

The tab is only shown when the result shape looks graph-like (3 columns that resemble subject,
predicate, object). For other shapes (e.g. aggregations), only the table is shown.

### Class hierarchy view

A dedicated panel or page (`/classes`) that runs:
```sparql
SELECT ?child ?parent WHERE { ?child rdfs:subClassOf ?parent }
```
and renders a collapsible tree using Cytoscape's `dagre` layout or a simple `<details>`-based
HTML tree. This is particularly useful for OWL ontologies like the Gene Ontology where the
class hierarchy is the primary structure.

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

## Implementation order

| Step | Work | Complexity |
|---|---|---|
| 1 | IRI links → `/?query=...` pre-fill (P0) | ~20 lines JS |
| 2 | Prefix shortening with tooltip (P0) | ~30 lines JS |
| 3 | Query history (localStorage) (P1) | ~50 lines JS |
| 4 | Prefix manager (localStorage) (P1) | ~60 lines JS |
| 5 | Resource browser page — client-side (P1) | ~80 lines JS + route |
| 6 | Store statistics in header (P1) | ~10 lines JS |
| 7 | Cytoscape.js graph view tab (P2) | ~100 lines JS + CDN import |
| 8 | Class hierarchy page (P2) | ~60 lines JS |
| 9 | CodeMirror query editor (P3) | ~40 lines JS + CDN import |
| 10 | Query templates dropdown (P3) | ~30 lines JS |
| 11 | Result CSV/JSON export (P3) | ~30 lines JS |
| 12 | Drag-and-drop upload (P4) | ~50 lines JS |

Steps 1–4 require **no server changes** — pure JS in `frontend.html`.
Steps 5–6 require one new server route and a small HTML template.
Steps 7–12 require only frontend changes.

All steps can be done incrementally. The single-file architecture (`frontend.html` with
inline CSS/JS served via `include_str!`) stays intact throughout; no build tooling is
introduced.
