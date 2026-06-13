# Visual Query Builder Plan

An optional graphical query builder inspired by
[OptiqueVQS](https://link.springer.com/chapter/10.1007/978-3-319-11964-9_3), accessible at
`/?view=build`.  It lets non-SPARQL users compose SELECT queries by clicking rather than
typing.  The output is a standard SPARQL query that runs through the existing `/sparql`
endpoint and can be pushed into the SPARQL textarea for further hand-editing.

No server changes are required.  All state and rendering is client-side.

---

## Interaction model

The builder has three persistent regions:

```
┌─────────────────────────────────────────────────────────────────────────┐
│ NAV  Query | Build | Class Hierarchy                                     │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  CANVAS (top)  — the query graph; node cards left-to-right              │
│                                                                         │
│  ┌────────────────────┐  ──knows──►  ┌────────────────────┐            │
│  │ ● Person  (?s)     │              │ ● Person  (?n1)     │            │
│  │  [active]          │              │                    │            │
│  └────────────────────┘              └────────────────────┘            │
│                                                                         │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  PROPERTY PANE (bottom-left)  — relative to the active node             │
│                                                                         │
│  Data properties                Object properties                       │
│  ─────────────────────────────  ───────────────────────────             │
│  ☑  rdfs:label   → ?s_label    [+ knows →]  (1 linked)                 │
│  ☐  ex:age       → ?s_age      [+ type  →]  not yet added              │
│  ☐  ex:email     → ?s_email                                             │
│                                                                         │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  GENERATED SPARQL (bottom-right / collapsible)                          │
│  SELECT ?s ?s_label ?n1 WHERE {                                         │
│    ?s a <http://example.org/Person> .                                   │
│    OPTIONAL { ?s rdfs:label ?s_label }                                  │
│    ?s <http://example.org/knows> ?n1 .                                  │
│    ?n1 a <http://example.org/Person> .                                  │
│  }                                                                      │
│  [Run]  [Edit in SPARQL editor]                                         │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### Active node

Exactly one node card in the canvas is **active** (highlighted border).  The property pane
always reflects the active node.  Clicking any node card activates it.

### Adding a data property

Checking a data-property checkbox adds that property's variable to SELECT and wraps it in
OPTIONAL in the WHERE clause:

```sparql
OPTIONAL { ?s rdfs:label ?s_label }
```

Unchecking removes it.

### Following an object property

Clicking `[+ knows →]` in the object-property panel:

1. Discovers the most common `rdf:type` of the objects reached via that property (sampled
   from the stored data).
2. Spawns a new node card to the right, connected by a labelled arrow.
3. Shifts focus (active node) to the new node.
4. The new node's property pane populates for its class.

---

## State model

The query graph is a tree of **Node** objects (cycles cannot be created in the UI):

```javascript
// Immutable roots
let qbState = {
  rootNode: null,   // Node | null
  activeId: null,   // string — ID of the focused node
  nodeSeq: 0,       // monotonic counter for variable naming
};

// Node shape
{
  id:         'n0',
  varName:    '?s',
  classIri:   'http://example.org/Person',
  dataProps: [
    {
      propIri:  'http://www.w3.org/2000/01/rdf-schema#label',
      varName:  '?s_label',
      checked:  true,
    },
    ...
  ],
  links: [
    {
      propIri:    'http://example.org/knows',
      targetNode: { /* another Node */ },
    },
    ...
  ],
}
```

---

## Property discovery

### Core query

Run one query per node activation — no `FILTER` built-ins needed:

```sparql
SELECT DISTINCT ?p ?o WHERE {
  ?s a <ClassIri> .
  ?s ?p ?o
} LIMIT 200
```

### Client-side classification

Inspect the `?o` column of each row returned in SPARQL JSON:

```javascript
const dataProps   = new Set();
const objectProps = new Set();

for (const b of bindings) {
  if (!b.p) continue;
  const propIri = b.p.value;
  if (b.o?.type === 'literal') dataProps.add(propIri);
  if (b.o?.type === 'uri')     objectProps.add(propIri);
}
```

A property appears in both sets when it has mixed values (acceptable; show in both panels).

`ISIRI` and `ISLITERAL` are implemented in the executor (confirmed in
`sparql_parser/src/execute.rs`), so an equivalent server-side approach with
`FILTER(isLiteral(?o))` / `FILTER(isIRI(?o))` also works and can replace the client-side
classification for larger datasets — but the sample approach is simpler and always correct.

### Target class discovery for object properties

When the user clicks `[+ prop →]`, detect the most common `rdf:type` of the reached objects:

```sparql
SELECT DISTINCT ?type WHERE {
  ?s a <SourceClass> .
  ?s <propIri> ?o .
  ?o a ?type
} LIMIT 20
```

Offer a picker if multiple types appear; pre-select the first.

---

## SPARQL generation rules

### Variable naming

| Role | Variable |
|---|---|
| Root subject | `?s` |
| Linked node `n` | `?n1`, `?n2`, … |
| Data property on `?x` | `?x_<local>` (e.g. `?s_label`, `?n1_name`) |

`<local>` is the IRI local name, lowercased, with non-alphanumeric chars replaced by `_`.

### Projection

`SELECT` projects all node variables (`?s`, `?n1`, …) and all checked data-property
variables.

### WHERE clause structure

```sparql
WHERE {
  ?s a <RootClass> .
  OPTIONAL { ?s <dataProp1> ?s_dp1 }
  OPTIONAL { ?s <dataProp2> ?s_dp2 }
  ?s <objProp1> ?n1 .
  ?n1 a <LinkedClass1> .
  OPTIONAL { ?n1 <dataPropA> ?n1_dpA }
  ...
}
```

Rules:
- The `?node a <Class>` triple is required (not OPTIONAL) — it anchors the node.
- Object-property links are required — they join nodes (INNER JOIN semantics).
- Data properties are OPTIONAL — instances without a label etc. still appear.
- The WHERE clause is built by a depth-first traversal of the node tree.

### Executor constraints

| Feature | Status |
|---|---|
| Basic graph patterns | ✓ Executed |
| OPTIONAL | ✓ Executed |
| FILTER (isIRI, isLiteral, comparisons, regex) | ✓ Executed |
| DISTINCT, LIMIT, OFFSET | ✓ Executed |
| ORDER BY | Parsed, silently ignored in executor |
| COUNT / aggregates | Parsed, not executed |

The builder must not rely on COUNT or ORDER BY in its discovery queries.

---

## Implementation phases

### Phase 1 — Single-level MVP

**Scope:** class selection → property pane for one node → SPARQL generation and run.

HTML additions:
- New `#build-view` section (client-side routed via `/?view=build`)
- `#class-picker` — text `<input>` with `<datalist id="class-list">` populated on load
- `#node-canvas` — single node card area
- `#data-prop-list` — checkbox list
- `#obj-prop-list` — button list (buttons disabled in Phase 1, enabled in Phase 2)
- `#qb-generated` — `<pre>` with live-generated SPARQL
- `[Run]` and `[Edit in SPARQL editor]` buttons

Class list query (run once on view load):

```sparql
SELECT DISTINCT ?c WHERE { { ?c a owl:Class } UNION { [] a ?c } } LIMIT 200
```

Data flow:
1. User types / picks a class → `onClassSelect(classIri)` fires
2. Property discovery query runs
3. Client classifies properties, populates both panels
4. Any checkbox toggle → `regenerateSparql()` → updates `#qb-generated`
5. `[Run]` button calls the existing `sparqlFetch` + `renderTable` pipeline

Complexity: ~120 lines JS + 40 lines CSS + 30 lines HTML.

---

### Phase 2 — Multi-hop expansion

**Scope:** object properties become clickable; canvas shows multiple node cards with arrows.

Additions:
- Object-property buttons become active; clicking calls `followObjectProp(propIri)`
- `followObjectProp` runs the target-class query, creates a new Node, appends to canvas
- Arrow connector between node cards (CSS `::after` pseudo-element or a flex row with
  centred `→ propLabel` span)
- Clicking any node card calls `setActiveNode(id)` and repopulates the property pane
- A `[×]` button on each non-root card removes the subtree it heads

Complexity: ~80 lines JS + 20 lines CSS.

---

### Phase 3 — Data-property filters

**Scope:** optional filter condition on each checked data property.

Each data-property row gets a small filter input that appears when the property is checked:

```
☑  rdfs:label  → ?s_label   [contains: ___________]
☑  ex:age      → ?s_age     [>: ______]
```

Filter type is inferred from the sampled values: string → regex/contains; number → comparison.

Generated SPARQL wraps the OPTIONAL with a FILTER:

```sparql
OPTIONAL {
  ?s rdfs:label ?s_label
  FILTER(regex(?s_label, "Alice", "i"))
}
```

Complexity: ~60 lines JS + 10 lines CSS.

---

### Phase 4 — Cytoscape canvas (optional)

Replace the horizontal card layout with a Cytoscape.js graph where:
- Each node card is a Cytoscape node (use HTML label via `cy.nodeHtmlLabel` plugin, or styled boxes)
- Object-property links are Cytoscape edges
- Clicking a node activates it

Cytoscape is already integrated for the graph-view tab.  The main new requirement is
`cytoscape-node-html-label` or an equivalent for rendering rich card content inside nodes;
alternatively, keep the canvas as plain HTML and use Cytoscape only for the class-hierarchy
page.

Complexity: ~100 lines JS if Cytoscape HTML labels work cleanly; otherwise keep Phase 2 layout.

---

## CSS sketch

```css
/* Build view layout */
#build-view { display: flex; flex-direction: column; gap: 1rem; }
.qb-canvas   { display: flex; align-items: flex-start; gap: 0;
               overflow-x: auto; padding: 1rem 0; min-height: 140px; }
.qb-bottom   { display: grid; grid-template-columns: 1fr 1fr; gap: 1rem; }

/* Node card */
.node-card   { background: #fff; border: 2px solid #ddd; border-radius: 6px;
               padding: 0.75rem; min-width: 160px; cursor: pointer; }
.node-card.active { border-color: #2b6cb0; box-shadow: 0 0 0 3px rgba(43,108,176,.15); }
.node-card h3 { margin: 0; font-size: 0.9rem; color: #222; }
.node-card .var-name { font-size: 0.75rem; color: #888; font-family: monospace; }

/* Arrow connector */
.node-arrow  { display: flex; align-items: center; padding: 0 0.5rem;
               font-size: 0.78rem; color: #555; white-space: nowrap; }

/* Property pane */
.prop-section h4 { margin: 0 0 0.4rem; font-size: 0.85rem; color: #333; }
.prop-row        { display: flex; align-items: center; gap: 0.4rem;
                   padding: 0.15rem 0; font-size: 0.82rem; }
.prop-var        { font-family: monospace; font-size: 0.78rem; color: #888; }
.btn-follow      { padding: 0.2rem 0.6rem; font-size: 0.78rem; background: #eef3ff;
                   color: #2b6cb0; border: 1px solid #b0c4e8; border-radius: 3px;
                   cursor: pointer; }
.btn-follow:hover { background: #dce8f5; }
.btn-follow.linked { background: #e6f4ea; color: #276749; border-color: #9dd5b0; }

/* Generated SPARQL */
#qb-generated { font-family: "Fira Mono","Consolas",monospace; font-size: 0.78rem;
                background: #f8f8f8; border: 1px solid #ddd; border-radius: 4px;
                padding: 0.6rem; white-space: pre; overflow-x: auto; }
```

---

## File changes

| File | Change |
|---|---|
| `sparql_endpoint/src/frontend.html` | Add `#build-view` HTML, CSS, and JS for phases |
| `sparql_endpoint/src/query_builder.rs` | New: Rust SPARQL-generation logic + unit tests |
| `sparql_endpoint/tests/query_builder_sparql.rs` | New: reqwest semantic tests |
| `sparql_endpoint/tests/frontend_browser.rs` | Add Phase 1–3 browser tests + self-test harness test |

The single-HTML-file constraint applies only to the frontend artefact.  Supporting test code
lives in Rust as normal.

---

## Automatic testing strategy

The builder has three distinct correctness concerns that call for different testing approaches:

| Concern | What can go wrong | Primary test layer |
|---|---|---|
| SPARQL generation | Wrong variables, missing OPTIONAL, wrong nesting | Rust unit tests (Layer 1) |
| Semantic correctness | Generated query returns wrong rows | reqwest integration tests (Layer 2) |
| Interaction model | UI state out of sync with SPARQL; clicks don't fire | Browser automation (Layer 3) |

Layers 1 and 2 run in `cargo test` with no external dependencies.  Layer 3 requires
`geckodriver` but skips gracefully without it (existing pattern).

---

### Layer 1 — Rust unit tests for SPARQL generation

**Why Rust, not just JS tests?**  The generation logic is purely functional (state tree →
SPARQL string) and has no I/O or DOM dependency.  Implementing it in Rust means it is
testable cheaply with `#[test]`, gets the full type-checker, and runs on every `cargo test`
without a browser or network.  The JavaScript in the frontend is a port of this same logic;
a shared fixture file keeps them aligned (see the consistency-check sub-section below).

**New module: `sparql_endpoint/src/query_builder.rs`**

```rust
pub struct QueryNode {
    pub var_name:   String,          // e.g. "s", "n1"
    pub class_iri:  String,
    pub data_props: Vec<DataProp>,
    pub links:      Vec<ObjectLink>,
}

pub struct DataProp {
    pub prop_iri: String,
    pub var_name: String,  // e.g. "s_label"
    pub checked:  bool,
}

pub struct ObjectLink {
    pub prop_iri:   String,
    pub target:     QueryNode,
}

/// Pure function: walks the node tree, emits SPARQL SELECT.
pub fn generate_sparql(root: &QueryNode) -> String { ... }
```

**Test cases to cover:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_node_no_props() {
        // SELECT ?s WHERE { ?s a <Class> . }
    }

    #[test]
    fn single_node_one_data_prop_checked() {
        // SELECT ?s ?s_label WHERE {
        //   ?s a <Class> .
        //   OPTIONAL { ?s rdfs:label ?s_label }
        // }
    }

    #[test]
    fn unchecked_data_prop_omitted_from_select_and_where() { ... }

    #[test]
    fn two_nodes_connected_by_object_prop() {
        // SELECT ?s ?n1 WHERE {
        //   ?s a <A> .
        //   ?s <prop> ?n1 .
        //   ?n1 a <B> .
        // }
    }

    #[test]
    fn three_node_chain_dfs_ordering() {
        // Verifies depth-first triple ordering in WHERE clause
    }

    #[test]
    fn variable_name_sanitises_special_chars() {
        // prop IRI ending in "first-name" → var "s_first_name"
    }

    #[test]
    fn multiple_data_props_each_optional() {
        // Each data prop is a separate OPTIONAL block
    }

    #[test]
    fn fan_out_multiple_links_from_one_node() {
        // ?s has two object-property links; both appear in WHERE
    }
}
```

Run with: `cargo test -p sparql-endpoint query_builder`

---

### Layer 2 — reqwest semantic tests

These tests verify that **queries matching what the builder would emit** return correct rows
from a real `TestServer`.  They run entirely within `cargo test` — no browser, no
geckodriver.

**New file: `sparql_endpoint/tests/query_builder_sparql.rs`**

Each test is named after the builder scenario it represents and documents the expected output
of the generation logic.  This makes the test file double as a specification.

```rust
const FIXTURE: &str = r#"
@prefix ex:   <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl:  <http://www.w3.org/2002/07/owl#> .

ex:alice a ex:Person ; rdfs:label "Alice" ; ex:knows ex:bob ; ex:age "30" .
ex:bob   a ex:Person ; rdfs:label "Bob"   ; ex:age "25" .
ex:Person a owl:Class .
"#;

/// Builder scenario: focus class = Person, no properties selected.
/// Expected: all Person instances, no extra columns.
#[tokio::test]
async fn builder_single_class_returns_all_instances() {
    let server = TestServer::start(FIXTURE).await;
    let sparql = "SELECT ?s WHERE { ?s a <http://example.org/Person> }";
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 2);
}

/// Builder scenario: Person + rdfs:label checked.
/// Expected: alice and bob, each with their label in ?s_label.
#[tokio::test]
async fn builder_person_with_label_prop() {
    let server = TestServer::start(FIXTURE).await;
    let sparql = r#"SELECT ?s ?s_label WHERE {
        ?s a <http://example.org/Person> .
        OPTIONAL { ?s rdfs:label ?s_label }
    }"#;
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 2);
    // Both rows have a label
    assert!(bindings.iter().all(|b| b["s_label"]["type"] == "literal"));
}

/// Builder scenario: Person -[knows]→ Person (two-node chain).
/// Expected: only alice (who has ex:knows), connected to bob.
#[tokio::test]
async fn builder_person_knows_person_two_node_chain() {
    let server = TestServer::start(FIXTURE).await;
    let sparql = r#"SELECT ?s ?n1 WHERE {
        ?s a <http://example.org/Person> .
        ?s <http://example.org/knows> ?n1 .
        ?n1 a <http://example.org/Person> .
    }"#;
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["s"]["value"], "http://example.org/alice");
    assert_eq!(bindings[0]["n1"]["value"], "http://example.org/bob");
}

/// Builder scenario: Person + label checked + knows → Person + label checked.
/// Verifies OPTIONAL wrapping on both nodes' data properties.
#[tokio::test]
async fn builder_two_nodes_both_with_labels() {
    let server = TestServer::start(FIXTURE).await;
    let sparql = r#"SELECT ?s ?s_label ?n1 ?n1_label WHERE {
        ?s a <http://example.org/Person> .
        OPTIONAL { ?s rdfs:label ?s_label }
        ?s <http://example.org/knows> ?n1 .
        ?n1 a <http://example.org/Person> .
        OPTIONAL { ?n1 rdfs:label ?n1_label }
    }"#;
    let bindings = sparql_bindings(&server, sparql).await;
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0]["s_label"]["value"], "Alice");
    assert_eq!(bindings[0]["n1_label"]["value"], "Bob");
}

// Helper
async fn sparql_bindings(server: &common::TestServer, sparql: &str) -> Vec<serde_json::Value> {
    let resp = server.client
        .post(server.sparql_url())
        .header("Content-Type", "application/sparql-query")
        .body(sparql.to_string())
        .send().await.unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    body["results"]["bindings"].as_array().unwrap().clone()
}
```

Run with: `cargo test --test query_builder_sparql`

---

### Layer 3a — JS self-test harness (browser, but fast)

The generation logic in JavaScript is a port of the Rust logic.  Rather than testing it
only through full interaction flows (which are slow), add a dedicated self-test page at
`/?view=build-selftest`.

When loaded, the page:
1. Constructs `QueryNode` state trees from hardcoded test fixtures (no network needed).
2. Calls `generateSparql(root)` directly.
3. Compares output to expected strings.
4. Writes a JSON summary to `<div id="qb-test-results">`:
   ```json
   { "passed": 8, "failed": 0, "errors": [] }
   ```

A single thirtyfour test then drives this:

```rust
#[tokio::test]
async fn js_sparql_generation_self_test() {
    let driver = match connect_driver().await { Some(d) => d, None => return };
    let server = common::TestServer::start("").await;

    driver.goto(&format!("{}/?view=build-selftest", server.base_url)).await.unwrap();

    assert!(wait_for_element(&driver, "#qb-test-results", 5000).await,
        "self-test results never appeared");

    let json_text = driver.find(By::Css("#qb-test-results"))
        .await.unwrap().text().await.unwrap();
    let results: serde_json::Value = serde_json::from_str(&json_text)
        .expect("test results must be valid JSON");

    assert_eq!(results["failed"], 0,
        "JS self-tests failed: {}", results["errors"]);
}
```

**Why this is better than individual interaction tests for generation logic:**
- One browser session tests all generation cases (~50 ms vs ~2 s per full interaction test).
- The test cases are the same set as the Rust unit tests — they act as a consistency check.
- Adding a new generation case costs one line in the fixture array, not a new test function.

---

### Layer 3b — Full interaction browser tests (per phase)

These test the rendering, event wiring, and state transitions that only exist in the browser.
They are slower and require geckodriver, but catch bugs that neither Rust unit tests nor
the self-test harness can find.

**Phase 1 tests:**

```rust
// Class list populates with at least one entry after loading an OWL fixture
#[tokio::test]
async fn class_list_populates_from_store() { ... }

// Selecting a class populates the data-property panel
#[tokio::test]
async fn selecting_class_shows_data_props() { ... }

// Checking a data-property checkbox updates #qb-generated
#[tokio::test]
async fn checking_data_prop_updates_generated_sparql() {
    // Verify that "OPTIONAL" and the prop IRI appear in #qb-generated text
}

// The generated SPARQL is syntactically accepted by the endpoint
#[tokio::test]
async fn generated_sparql_is_valid_and_runs() {
    // Click Run, verify #qb-results shows a result table not an error
}
```

**Phase 2 tests:**

```rust
// Following an object property adds a second node card to the canvas
#[tokio::test]
async fn follow_obj_prop_adds_node_card() { ... }

// Clicking the second card activates it (property pane updates)
#[tokio::test]
async fn clicking_node_card_activates_it() { ... }

// Removing the linked node removes it from #qb-generated
#[tokio::test]
async fn removing_node_updates_generated_sparql() { ... }
```

**Phase 3 tests:**

```rust
// Filter input appears when a data property is checked
#[tokio::test]
async fn filter_input_appears_when_prop_checked() { ... }

// Typing in the filter input adds FILTER(...) to #qb-generated
#[tokio::test]
async fn filter_text_appears_in_generated_sparql() { ... }
```

---

### Consistency between Rust and JS generation

The Rust unit tests (Layer 1) and the JS self-test harness (Layer 3a) must test the same
cases.  Keep a comment in each pointing to the other:

```rust
// sparql_endpoint/src/query_builder.rs
// These test cases are mirrored in frontend.html's `QB_SELF_TESTS` array.
// If you add a case here, add the equivalent to that array, and vice versa.
```

This is not enforced by the compiler, but the Layer 1 + 3a combination will catch drift:
if the Rust logic and JS logic diverge, Layer 3a's `failed > 0` makes the browser test
fail in CI, even if the Rust tests pass.

---

### What each layer catches

| Bug type | Layer 1 | Layer 2 | Layer 3a | Layer 3b |
|---|---|---|---|---|
| Wrong SPARQL syntax emitted | ✓ | via 400 | ✓ | via error banner |
| Wrong SPARQL semantics (wrong rows) | — | ✓ | — | ✓ (indirect) |
| JS/Rust generation divergence | — | — | ✓ | — |
| UI event not wired (checkbox does nothing) | — | — | — | ✓ |
| Wrong variable naming | ✓ | ✓ | ✓ | — |
| Missing OPTIONAL wrapping | ✓ | ✓ | ✓ | — |
| Active-node state bug | — | — | — | ✓ |
| Multi-hop ordering wrong | ✓ | ✓ | ✓ | — |

Layer 1 and Layer 2 together catch most generation bugs without geckodriver.
Layers 3a and 3b add coverage for JS-specific and interaction bugs.

---

### Running the tests

```bash
# Layers 1 + 2 (no geckodriver needed)
cargo test -p sparql-endpoint

# Layer 3 (geckodriver required)
geckodriver --port 4444 &
cargo test --test frontend_browser -p sparql-endpoint
```
