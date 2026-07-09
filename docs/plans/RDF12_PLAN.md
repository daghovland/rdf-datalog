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

## TDD protocol for all phases

All implementation follows the CLAUDE.md TDD protocol:

1. **Tests first** — write all tests for the phase before touching the implementation. Test sources: W3C RDF 1.2 conformance suites (see Phase R0.5), DagSemTools ported tests, and inline examples. Mark all tests `#[ignore]`.
2. **Stub** — add just enough type stubs and function signatures to let the test file compile.
3. **Implement** — unignore one test, implement just enough to pass it, verify no regressions, then move to the next.

The user reviews ignored tests before implementation begins.

---

## Named-graph semantics for triple terms

This is a critical design point that affects all phases.

A triple term `<<( :s :p :o )>>` is **not a triple in any named graph** — it is a globally-identified reference to the *idea* of a triple, identified purely by structure. The same triple term appearing in two different named graphs is the same object:

```trig
# Both graphs reference THE SAME triple term (same structural identity):
:g1 { <<( :alice :knows :bob )>> :assertedBy :carol . }
:g2 { <<( :alice :knows :bob )>> :believedBy :dave . }
```

Contrast with actual assertion in a named graph, which is a completely separate fact:
```trig
:g0 { :alice :knows :bob }   # asserts the triple is in g0
                              # does NOT create a triple term
```

**Storage in the datastore:**

```
reified_triples:  ONE row  → (triple_term_id, alice, knows, bob)
                             graph-agnostic; the "graph" slot holds the triple term's own ID

named_graphs:     TWO rows → (triple_term_id, assertedBy, carol, g1)
                             (triple_term_id, believedBy, dave,  g2)
                             the annotation triples ARE graph-scoped
```

**SPARQL executor pattern** for `GRAPH ?g { <<( ?s :knows ?o )>> :assertedBy ?ann }`:

1. Query `named_graphs` with predicate `:assertedBy`, binding `?ann` and `?g`
2. Filter subjects to those that resolve to `GraphElement::TripleTerm`
3. For each such subject ID, look up in `reified_triples` to bind `?s` and `?o`

The inner triple pattern variables (`?s`, `?o`) come from `reified_triples`; the outer graph variable (`?g`) comes from `named_graphs`. The join key is the triple term's `GraphElementId`.

This cross-index join is the core execution pattern for SPARQL over triple terms in named graphs and must be implemented in Phase R3.

---

## Current state in dagalog

The datastore already has the right foundation:

```rust
pub struct Datastore {
    pub named_graphs: QuadTable,      // normal triples / named graphs
    pub reified_triples: QuadTable,   // triple term decomposition (triple_id → s, p, o)
    pub resources: GraphElementManager,
}
```

`reified_triples` already existed for classic RDF reification but is now used for RDF 1.2 triple terms.

The turtle parser (`turtle/src/lib.rs`) uses `oxttl` + `oxrdf`:
- `oxttl 0.2.3` **already parses** `<<` and `<<(...)>>` syntax behind `#[cfg(feature = "rdf-12")]`
- `oxrdf 0.3.3` already has `Term::Triple(Box<Triple>)` for object-position triple terms

---

## Phases

### Phase R0 — Epic issue and GitHub setup

Completed: epic [#143](https://github.com/daghovland/rdf-datalog/issues/143) created; sub-issues #144–#147, #149 created.

---

### Phase R0.5 — Test infrastructure ([#149](https://github.com/daghovland/rdf-datalog/issues/149))

**Do this before starting R2.** Vendor the W3C RDF 1.2 conformance test suites and write all tests (initially `#[ignore]`) so R2 and R3 can work test-by-test.

**Vendor test data** into `tests/testdata/`:
- `w3c_rdf12_turtle/`   — from https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-turtle/
- `w3c_rdf12_ntriples/` — from https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-nt/
- `w3c_rdf12_nquads/`   — from https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-nq/
- `w3c_rdf12_trig/`     — from https://w3c.github.io/rdf-tests/rdf/rdf12/rdf-trig/

**Add `tests/w3c_rdf12_conformance.rs`** following the pattern of `tests/w3c_rdf_conformance.rs`:
- Parse each `manifest.ttl` to enumerate `PositiveSyntax`, `NegativeSyntax`, `PositiveEval`, `NegativeEval` entries
- Generate one `#[test] #[ignore]` per entry
- Each eval test compares parsed output against expected N-Triples/N-Quads via graph isomorphism

**Add `turtle/tests/rdf12.rs`** with inline TDD tests from DagSemTools, all `#[ignore]`:
```rust
#[test] #[ignore] // #145
fn test_triple_term_as_object() { /* <<( :s :p :o )>> :ann :val */ }

#[test] #[ignore] // #145
fn test_triple_term_as_subject() { /* <<( :s :p :o )>> :q :r */ }

#[test] #[ignore] // #145
fn test_nested_triple_term() { /* <<( <<( :a :b :c )>> :d :e )>> :f :g */ }

#[test] #[ignore] // #145
fn test_same_triple_term_in_two_named_graphs() {
    // Parses a TriG file with <<( :alice :knows :bob )>> in :g1 and :g2;
    // verifies only ONE row in reified_triples, TWO rows in named_graphs.
}
```

Also port SPARQL 1.2 triple-term query tests into `tests/sparql12_suite.rs` (all `#[ignore]` until R3):
```rust
#[test] #[ignore] // #146
fn test_sparql_triple_term_in_where() { /* SELECT ?ann WHERE { <<( :alice :knows :bob )>> :assertedBy ?ann } */ }

#[test] #[ignore] // #146
fn test_sparql_triple_term_with_graph() { /* SELECT ?g WHERE { GRAPH ?g { <<( ... )>> :ann :val } } */ }
```

---

### Phase R1 — Data model: `TripleTerm` variant in `ingress` ([#144](https://github.com/daghovland/rdf-datalog/issues/144))

**Status: complete — see PR [#148](https://github.com/daghovland/rdf-datalog/pull/148).**

Added to `ingress/src/lib.rs`:
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
    TripleTerm(TripleTermKey),   // RDF 1.2 embedded triple
}
```

`GraphElementManager` interns by structural equality, so two occurrences of the same triple term get the same `GraphElementId` automatically.

Added to `dag_rdf/src/datastore.rs`:
```rust
pub fn add_triple_term(&mut self, s: GraphElementId, p: GraphElementId, o: GraphElementId) -> GraphElementId
```
Interns the `TripleTerm` and stores one row in `reified_triples` where the "graph" slot holds the triple term's own ID (making structural lookup by ID possible).

---

### Phase R2 — Turtle 1.2 parser (`turtle` crate) ([#145](https://github.com/daghovland/rdf-datalog/issues/145))

**Depends on:** R1 (complete), R0.5 (test infrastructure).

**TDD steps:**

1. Unignore inline tests in `turtle/tests/rdf12.rs` one by one.
2. Enable `rdf-12` feature in `turtle/Cargo.toml`:
   ```toml
   oxttl = { version = "0.2.3", features = ["rdf-12"] }
   ```
3. Extend `intern_term` in `turtle/src/lib.rs`:
   ```rust
   fn intern_term(datastore: &mut Datastore, term: Term) -> Option<GraphElementId> {
       match term {
           Term::NamedNode(n)   => Some(intern_named_node(datastore, n.into_string())),
           Term::BlankNode(n)   => Some(datastore.resources.get_or_create_named_anon_resource(n.into_string())),
           Term::Literal(lit)   => Some(datastore.add_literal_resource(convert_literal(lit))),
           Term::Triple(triple) => {   // NEW
               let s = intern_subject(datastore, triple.subject);
               let p = intern_named_node(datastore, triple.predicate.into_string());
               intern_term(datastore, triple.object).map(|o| datastore.add_triple_term(s, p, o))
           }
       }
   }
   ```
4. Once inline tests pass, unignore W3C RDF 1.2 Turtle conformance tests from `tests/w3c_rdf12_conformance.rs` (positive syntax first, then eval).

**Subject-position blocker:** `oxrdf 0.3.3` defines `Triple.subject: NamedOrBlankNode`, which cannot represent a triple in subject position. Two options:
- **Option A:** Handle only object-position triple terms; return an error for subject-position until oxrdf is updated.
- **Option B:** Check what `oxttl` with `rdf-12` actually emits for `<<( :s :p :o )>> :q :r` — it may already convert these to blank nodes + reification quads, in which case it works transparently.

Determine which option applies by running the first subject-position test and observing oxttl's output.

**N-Triples 1.2 / N-Quads 1.2 / TriG 1.2:** the same `intern_term` extension covers all formats since `NTriplesParser`, `NQuadsParser`, and `TriGParser` also come from oxttl.

---

### Phase R3 — SPARQL 1.2 parser + executor ([#146](https://github.com/daghovland/rdf-datalog/issues/146))

**Depends on:** R1 (complete), R0.5 (test infrastructure).

**TDD steps:** Unignore SPARQL triple-term tests from `tests/sparql12_suite.rs` one by one.

**AST change** (`sparql_parser/src/ast.rs`):
```rust
pub enum Term {
    Resource(GraphElementId),
    Variable(String),
    TripleTerm(Box<TriplePattern>),   // NEW — <<( s p o )>>
}
```

**Parser change** (`sparql_parser/src/sparql_grammar.rs` — nom-based):
Add a nom rule for `<<(` ... `)>>` that recursively parses a triple pattern. Grammar production:
```
TripleTerm ::= '<<(' subject predicate object ')>>'
```

**Executor change** (`sparql_parser/src/execute.rs`):

For a fully-ground triple term subject `<<( :s :p :o )>>`:
1. Resolve the triple term to its `GraphElementId` via `reified_triples` lookup by (s, p, o)
2. Use that ID as the subject in the outer pattern match against `named_graphs`

For a partially-variable triple term `<<( ?s :p ?o )>>` as subject:
1. Query `reified_triples.get_quads_with_predicate(p_id)` to enumerate candidate triple terms
2. For each candidate triple term ID, check it appears as subject in `named_graphs` (with the outer predicate/object binding)
3. Bind `?s` and `?o` from `reified_triples`; bind outer variables from `named_graphs`

For `GRAPH ?g { <<( ?s :p ?o )>> :ann ?v }` (cross-index join — see [Named-graph semantics](#named-graph-semantics-for-triple-terms) above):
1. Query `named_graphs` filtered by predicate `:ann`, binding `?v` and `?g`; subject must be a `GraphElement::TripleTerm`
2. For each matching triple-term subject, look up in `reified_triples` to bind `?s` and `?o`

---

### Phase R4 — Serialisation ([#147](https://github.com/daghovland/rdf-datalog/issues/147))

**Depends on:** R1 (complete), R3.

When query results or serialised graphs contain triple terms, emit them with `<<( ... )>>` syntax.

- `turtle/src/serialize.rs`: add `GraphElement::TripleTerm` arm → emit `<<( s p o )>>`
- SPARQL result formats: SPARQL 1.2 XML/JSON result formats have an encoding for triple terms; check the current spec draft and implement
- Content-Type negotiation: advertise Turtle 1.2 / N-Triples 1.2 support

---

### Phase R5 — JSON-LD 1.1 (no change required)

JSON-LD 1.1 does not include triple terms. No changes needed in `jsonld_parser`.

---

### Phase R6 — OWL / Datalog reasoning over triple terms

Triple terms interact with Datalog rules:
- A rule head `[<<( ?s ?p ?o )>>, :assertedBy, ?source]` should produce annotations
- The datalog evaluator (`datalog/src/`) needs to support triple term variables in rule heads and bodies

Defer to a future issue — basic RDF 1.2 parsing + SPARQL querying should ship first.

---

## Implementation order

| Phase | Issue | Effort | Blocker? |
|---|---|---|---|
| R0 | [#143](https://github.com/daghovland/rdf-datalog/issues/143) Create epic | ✅ done | — |
| R1 | [#144](https://github.com/daghovland/rdf-datalog/issues/144) Data model | ✅ done (PR [#148](https://github.com/daghovland/rdf-datalog/pull/148)) | — |
| R0.5 | [#149](https://github.com/daghovland/rdf-datalog/issues/149) W3C test suite + TDD test files | small | R1 |
| R2 | [#145](https://github.com/daghovland/rdf-datalog/issues/145) Turtle 1.2 parser | small–medium | R1, R0.5 |
| R3 | [#146](https://github.com/daghovland/rdf-datalog/issues/146) SPARQL 1.2 parser + executor | medium | R1, R0.5 |
| R4 | [#147](https://github.com/daghovland/rdf-datalog/issues/147) Serialisation | small | R1, R3 |
| R5 | (JSON-LD — skip, not in JSON-LD 1.1) | — | — |
| R6 | Reasoning (future issue) | large | R1, R3 |

---

## Open questions

1. **oxrdf subject-position triple terms** — does `oxrdf 0.3.3` with `rdf-12` feature extend `NamedOrBlankNode` to include `Triple`? Determine this in Phase R2 by testing oxttl's actual output for subject-position triple terms.

2. **SPARQL result XML/JSON encoding** — the SPARQL 1.2 spec adds an encoding for triple terms in result documents. Check the current draft and implement in Phase R4.

3. **DagSemTools reference** — DagSemTools (the F# ancestor) has `reified_triples` support and RDF 1.2 tests. Port those tests into `turtle/tests/rdf12.rs` and `tests/sparql12_suite.rs` during Phase R0.5.

---

## References

- [RDF 1.2 Concepts](https://www.w3.org/TR/rdf12-concepts/)
- [Turtle 1.2](https://www.w3.org/TR/turtle12/)
- [SPARQL 1.2](https://www.w3.org/TR/sparql12-query/)
- [W3C RDF 1.2 test suites](https://w3c.github.io/rdf-tests/rdf/rdf12/)
- [oxttl rdf-12 feature](https://crates.io/crates/oxttl)
- Epic issue: [#143](https://github.com/daghovland/rdf-datalog/issues/143)
