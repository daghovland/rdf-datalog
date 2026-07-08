# RDF 1.2 Support Plan

RDF 1.2 ([W3C spec](https://www.w3.org/TR/rdf12-concepts/)) adds **triple terms** and **reified triples** as first-class constructs. This document describes how to add RDF 1.2 support to dagalog.

Tracked in epic: [#143](https://github.com/daghovland/rdf-datalog/issues/143)

---

## What is RDF 1.2 reification?

RDF 1.2 introduces two syntactic forms in Turtle 1.2:

| Form | Syntax | Meaning |
|---|---|---|
| **Triple term** | `<<( :s :p :o )>>` | Embedded triple usable as subject or object |
| **Reified triple** | `<< :s :p :o >> ~:id` | Shorthand: triple term + annotation IRI |

A triple term `<<( :s :p :o )>>` can appear anywhere a subject or object appears:

```turtle
# annotation: who said it?
<<( :alice :knows :bob )>> :assertedBy :carol .

# nested annotation
:doc :contains <<( <<( :alice :knows :bob )>> :assertedBy :carol )>> .
```

The key semantic property: two occurrences of `<<( :s :p :o )>>` with the same s/p/o are **the same triple term** — identity is structural, not by minted IRI.

---

## Current state in dagalog

The datastore already has the right foundation:

```rust
pub struct Datastore {
    pub named_graphs: QuadTable,      // normal triples / named graphs
    pub reified_triples: QuadTable,   // reification storage (triple_id, s, p, o)
    pub resources: GraphElementManager,
}
```

`reified_triples` already exists but is only used for classic RDF reification (rdf:subject / rdf:predicate / rdf:object pattern). RDF 1.2 triple terms will use this table.

The turtle parser (`turtle/src/lib.rs`) uses `oxttl` + `oxrdf`:
- `oxttl 0.2.3` **already parses** `<<` and `<<(...)>>` syntax behind `#[cfg(feature = "rdf-12")]`
- `oxrdf 0.3.3` already has `Term::Triple(Box<Triple>)` for object-position triple terms

---

## Phases

### Phase R0 — Epic issue and GitHub setup

Create a GitHub epic issue. Create sub-issues for each phase below, linked to the epic.

---

### Phase R1 — Data model: `TripleTerm` variant in `ingress`

**File:** `ingress/src/lib.rs`

Add a new variant to `GraphElement`:

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TripleTermKey {
    pub subject:   GraphElementId,
    pub predicate: GraphElementId,
    pub obj:       GraphElementId,
}

pub enum GraphElement {
    NodeOrEdge(RdfResource),
    GraphLiteral(RdfLiteral),
    TripleTerm(TripleTermKey),   // NEW — RDF 1.2 embedded triple
}
```

`GraphElementManager` already interns by structural equality (`HashMap<GraphElement, GraphElementId>`), so two occurrences of the same triple term automatically get the same `GraphElementId`. No extra index needed for interning.

**`dag_rdf` changes:**
- Add `Datastore::add_triple_term(s, p, o) -> GraphElementId`:
  1. Check if `GraphElement::TripleTerm(TripleTermKey{s,p,o})` is already in `resources`
  2. If not, intern it and also store in `reified_triples` as `Quad{triple_id: new_id, subject: s, predicate: p, obj: o}`
  3. Return the `GraphElementId`

**SPARQL query over triple terms:** the `reified_triples` index supports looking up by subject/predicate/obj. To find all triple terms matching `<<( ?s :p ?o )>>`, query `reified_triples.get_quads_with_predicate(p_id)`.

**Tests:** unit tests in `dag_rdf/src/datastore.rs` — intern the same triple twice, get same ID; intern two different triples, get different IDs.

---

### Phase R2 — Turtle 1.2 parser (`turtle` crate)

**File:** `turtle/Cargo.toml` — enable the `rdf-12` feature:
```toml
oxttl = { version = "0.2.3", features = ["rdf-12"] }
oxrdf = { version = "0.3.3", features = ["rdf-12"] }  # if needed
```

**File:** `turtle/src/lib.rs` — extend `intern_term` and `intern_subject`:

```rust
fn intern_term(datastore: &mut Datastore, term: Term) -> Option<GraphElementId> {
    match term {
        Term::NamedNode(n)  => Some(intern_named_node(datastore, n.into_string())),
        Term::BlankNode(n)  => Some(datastore.resources.get_or_create_named_anon_resource(n.into_string())),
        Term::Literal(lit)  => Some(datastore.add_literal_resource(convert_literal(lit))),
        Term::Triple(triple) => {          // NEW
            let s = intern_subject(datastore, triple.subject);
            let p = intern_named_node(datastore, triple.predicate.into_string());
            intern_term(datastore, triple.object).map(|o| datastore.add_triple_term(s, p, o))
        }
    }
}
```

**The subject-position blocker:** `oxrdf 0.3.3` defines `Triple.subject: NamedOrBlankNode`, which does **not** include `Triple`. So for `<<( <<( :a :b :c )>> :p :o )>>` (nested triple in subject position), oxrdf can't represent it.

Two options:
- **Option A (simpler):** Handle only object-position triple terms in this phase; detect and return an error for subject-position triple terms until oxrdf adds proper RDF 1.2 support.
- **Option B (workaround):** oxttl may convert subject-position triple terms to an allocated blank node (the outer triple is emitted separately). Check oxttl's actual output for this case; if it uses a blank node, it will already parse correctly and we just need to wire up the `reified_triples` storage.

Check by running a test with `<<( :s :p :o )>> :q :r` and observing what `TurtleParser` with `rdf-12` feature emits.

**N-Triples 1.2 / N-Quads 1.2:** these formats also support triple terms. The same `intern_term` extension covers them since `NTriplesParser` and `NQuadsParser` also come from oxttl.

**Tests:** `turtle/tests/rdf12.rs` — parse Turtle 1.2 documents with triple terms as objects and subjects; verify they appear in `reified_triples` and `named_graphs` correctly.

---

### Phase R3 — SPARQL 1.2 parser (`sparql_parser` crate)

SPARQL 1.2 adds **triple term patterns** in `WHERE` clauses:

```sparql
SELECT ?s ?ann WHERE {
    <<( ?s :knows ?o )>> :assertedBy ?ann .
}
```

And nested:
```sparql
SELECT ?claim WHERE {
    <<( <<( :alice :knows :bob )>> :assertedBy :carol )>> :believedBy ?claim .
}
```

**AST change** (`sparql_parser/src/ast.rs`):
```rust
pub enum Term {
    Resource(GraphElementId),
    Variable(String),
    TripleTerm(Box<TriplePattern>),   // NEW — <<( s p o )>>
}
```

Where `TriplePattern` already exists (or rename/reuse the existing pattern type).

**Parser change** (`sparql_parser/src/sparql_grammar.rs` — nom-based):
Add a rule for `<<(` ... `)>>` that recursively parses a triple pattern. Grammar production (from SPARQL 1.2 spec):
```
TripleTerm ::= '<<(' subject predicate object ')>>'
```
where subject and object are themselves `Term`s (allowing nesting).

**Executor change** (`sparql_parser/src/execute.rs`):
When evaluating a pattern `{ <<( s_pat p_pat o_pat )>> pred obj }`:
1. Match the outer subject against `named_graphs` — the subject ID must resolve to a `GraphElement::TripleTerm`
2. Look up that triple term in `reified_triples` to get its s/p/o IDs
3. Apply the inner pattern `s_pat p_pat o_pat` against those IDs — extending the substitution

For the case where the triple term is partially bound (e.g., `<<( ?s :p ?o )>>`):
1. Query `reified_triples.get_quads_with_predicate(p_id)` to enumerate candidates
2. Filter to those triple term IDs that appear as subjects in the outer pattern
3. Bind ?s and ?o from the enumerated results

**Tests:** `sparql_parser/tests/rdf12_sparql.rs` (or extend `tests/sparql12_suite.rs`).

---

### Phase R4 — SPARQL endpoint serialisation (Turtle 1.2 output)

When query results contain triple terms (in `SELECT ?x WHERE { <<(...)>> :p ?x }`), the HTTP response must serialise them correctly.

Changes in `sparql_endpoint/src/`:
- SPARQL XML results: triple terms have no standard representation in SPARQL 1.1 XML; may need to use blank node substitution or a SPARQL 1.2 extension format
- Turtle / N-Triples output: use `<<( ... )>>` syntax when serialising a `GraphElement::TripleTerm`
- Content-Type negotiation: advertise Turtle 1.2 / N-Triples 1.2 support

Also update `turtle/src/serialize.rs` to emit `<<( ... )>>` for `GraphElement::TripleTerm`.

---

### Phase R5 — JSON-LD 1.1 (no change required)

JSON-LD 1.1 does not include triple terms — that is a future JSON-LD 2.0 concern. No changes needed in `jsonld_parser`.

---

### Phase R6 — OWL / Datalog reasoning over triple terms

Triple terms interact with OWL reasoning and custom Datalog rules:
- A rule head `[<<( ?s ?p ?o )>>, :assertedBy, ?source]` should be able to produce annotations
- The datalog evaluator (`datalog/src/`) needs to support triple term variables in rule heads and bodies

Defer to a later phase — basic RDF 1.2 parsing + SPARQL querying should ship first.

---

## Implementation order

| Phase | Issue | Effort | Blocker? |
|---|---|---|---|
| R0 | [#143](https://github.com/daghovland/rdf-datalog/issues/143) Create epic | minimal | — |
| R1 | [#144](https://github.com/daghovland/rdf-datalog/issues/144) `GraphElement::TripleTerm` + `add_triple_term` | small | — |
| R2 | [#145](https://github.com/daghovland/rdf-datalog/issues/145) Turtle parser (object-position first; investigate subject-position) | small–medium | R1 |
| R3 | [#146](https://github.com/daghovland/rdf-datalog/issues/146) SPARQL 1.2 parser + executor | medium | R1 |
| R4 | [#147](https://github.com/daghovland/rdf-datalog/issues/147) Serialisation / endpoint | small | R1, R3 |
| R5 | (JSON-LD — skip, not in JSON-LD 1.1) | — | — |
| R6 | Reasoning (future issue) | large | R1, R3 |

---

## Open questions

1. **oxrdf subject-position triple terms** — does `oxrdf 0.3.3` with `rdf-12` feature extend `NamedOrBlankNode` to include `Triple`? If not, Phase R2b requires either waiting for a new oxrdf release or handling subject-position triple terms in a post-processing step (oxttl may already convert them to blank nodes + emitted reification quads).

2. **SPARQL 1.1 XML results format** — no standard encoding for triple terms exists; the SPARQL 1.2 spec adds a new encoding. Check current draft and implement accordingly.

3. **DagSemTools reference** — DagSemTools (the F# ancestor) does have `reified_triples` support. The Rust `Datastore` already mirrors this structure. The actual triple-term interning code (Phase R1) is new and has no direct F# equivalent to translate.

---

## References

- [RDF 1.2 Concepts](https://www.w3.org/TR/rdf12-concepts/)
- [Turtle 1.2](https://www.w3.org/TR/turtle12/)
- [SPARQL 1.2](https://www.w3.org/TR/sparql12-query/)
- [oxttl rdf-12 feature](https://crates.io/crates/oxttl) — already in `turtle/Cargo.toml` dependency
- Epic issue: [#143](https://github.com/daghovland/rdf-datalog/issues/143)
