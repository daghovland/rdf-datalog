# Translation Plan: DagSemTools → Rust

This document describes the plan for translating DagSemTools (F#/.NET) into this Rust workspace.
The goal is a fast triplestore with a native Rust implementation of datalog-based OWL-RL reasoning.

---

## Crate mapping

Each F# project becomes a Rust crate. Names are kept as close as possible:

| DagSemTools project | Rust crate | Status |
|---|---|---|
| `Ingress` | `ingress` | Partially done |
| `Rdf` | `dag_rdf` | Partially done |
| `Datalog` | `datalog` | Not started |
| `OwlOntology` | `owl_ontology` | Not started |
| `OWL2RL2Datalog` | `owl2rl2datalog` | Not started |
| `ELI` | `eli` | Not started |
| `Turtle.Parser` | `turtle_parser` | Not started |
| `Manchester.Parser` | `manchester_parser` | Not started |
| `Sparql.Parser` | `sparql_parser` | Not started |
| `Datalog.Parser` | `datalog_parser` | Not started |
| `Api` | root crate `dagalog` | Stub only |
| `AlcTableau`, `OWL2ALC` | `alc_tableau` | Deferred |

---

## ANTLR4 in Rust

The official ANTLR4 tool does not target Rust natively. The recommended approach:

- Use the **`antlr4rust`** crate (by rrevenantt on GitHub) which provides:
  - A Rust runtime (`antlr4rust` on crates.io)
  - A code generator (a fork of the ANTLR4 tool that emits Rust)
- The existing `.g4` grammar files in `grammars/` can be reused **with minor modifications** (the Rust target has some syntax differences in actions/predicates, but pure grammar rules are portable).
- Each parser crate will have a `build.rs` that invokes the ANTLR4 tool to generate Rust parser/lexer source from the grammar files, similar to how the C# projects reference them.

Alternative if `antlr4rust` proves too unstable: **`pest`** (PEG-based, clean Rust, good for Turtle/SPARQL), but this would mean rewriting grammars. Try `antlr4rust` first.

---

## Phase 1 — Complete the foundation crates

### 1a. Fix `ingress` crate gaps

DagSemTools `Ingress` defines a `defaultGraphElementId = 0u` that is pre-populated in the element manager. Currently the Rust `ingress` crate has no concept of a default graph.

- Add `pub const DEFAULT_GRAPH_ELEMENT_ID: GraphElementId = 0;` in `dag_rdf/src/ingress.rs`
- Add `pub const DEFAULT_GRAPH_IRI: &str = "urn:x-default-graph"` (or use the DagSemTools sentinel)

### 1b. Complete `dag_rdf` crate

Missing from DagSemTools `Rdf` module:

**`query.rs`** — add `Term` and `QuadPattern` types (from `DagSemTools.Rdf.Query`):
```rust
pub enum Term {
    Resource(GraphElementId),
    Variable(String),
}

pub struct QuadPattern {
    pub graph: Term,
    pub subject: Term,
    pub predicate: Term,
    pub object: Term,
}
```
Also add `get_default_graph_pattern(subject, predicate, object)` helper.

**`triple_table.rs`** — add `TripleTable` (currently only `QuadTable` exists). In DagSemTools, `TripleTable` is a plain triple store without a graph ID; the Rust code may unify this with `QuadTable` using the default graph ID.

**`named_triple_table.rs`** — `NamedTripleTable` is a view over a `QuadTable` filtered to one `graph_id`. Can be a thin wrapper struct with iterator adapters.

**`datastore.rs`** — add `Datastore` struct mirroring DagSemTools:
```rust
pub struct Datastore {
    pub reified_triples: QuadTable,
    pub named_graphs: QuadTable,
    pub resources: GraphElementManager,
}
```
Add the full query API (`get_triples_with_subject`, `get_triples_with_subject_predicate`, `get_quads`, etc.) and `add_quad`, `add_triple`, `add_named_graph_triple`, `add_reified_triple`.

---

## Phase 2 — `datalog` crate

New crate `datalog/` depending on `dag_rdf`.

Translate `DagSemTools.Datalog` (files: `Library.fs`, `Reasoner.fs`, `Stratifier.fs`, `Unification.fs`, `PredicateGrounder.fs`).

Key types:
```rust
pub enum ResourceOrWildcard {
    Resource(GraphElementId),
    Wildcard,
}

pub struct QuadWildcard {
    pub graph: ResourceOrWildcard,
    pub subject: ResourceOrWildcard,
    pub predicate: ResourceOrWildcard,
    pub object: ResourceOrWildcard,
}

pub enum RuleHead {
    NormalHead(QuadPattern),
    Contradiction,
}

pub enum RuleAtom {
    PositivePattern(QuadPattern),
    NotPattern(QuadPattern),
    NotEqualsAtom(Term, Term),
}

pub struct Rule {
    pub head: RuleHead,
    pub body: Vec<RuleAtom>,
}

pub type Substitution = HashMap<String, GraphElementId>;
```

Submodules:
- `datalog.rs` — `evaluate_pattern`, `get_substitutions`, `apply_substitution_quad`, `wildcard_quad_pattern`, safety checks
- `reasoner.rs` — `DatalogProgram` (naive materialisation), `evaluate(rules, datastore)`
- `stratifier.rs` — `RulePartitioner`, `order_rules()` (topological sort with cycle detection, stratification for negation)
- `unification.rs` — `quad_patterns_unifiable`, `depending_rules`, `intentional_rules`

The `Stratifier` is the most complex piece — it implements topological sorting with a Kahn-style algorithm and handles negation stratification. Port it carefully, maintaining the `OrderedRule` struct with `successors`, `num_predecessors`, `uses_intensional_negative_edge`.

---

## Phase 3 — `owl_ontology` crate

New crate `owl_ontology/` depending on `ingress`.

Translate `DagSemTools.OwlOntology` (files: `Axioms.fs`, `Ontology.fs`, `Library.fs`).

These are pure data types — enums for OWL axioms, class expressions, property expressions, individuals, data ranges. No logic, just the full OWL 2 type hierarchy:
- `ClassExpression` (ClassName, ObjectIntersectionOf, ObjectSomeValuesFrom, …)
- `ObjectPropertyExpression` (NamedObjectProperty, InverseObjectProperty, ObjectPropertyChain)
- `DataRange` (NamedDataRange, DataIntersectionOf, …)
- `Axiom` / `ClassAxiom` / `ObjectPropertyAxiom` / `DataPropertyAxiom`
- `Ontology { iri, version_iri, axioms }`

---

## Phase 4 — `eli` crate

New crate `eli/` depending on `owl_ontology` and `dag_rdf`.

Translate `DagSemTools.ELI` (files: `ELIAxiom.fs`, `ELIExtractor.fs`, `ELI2RL.fs`, `Library.fs`).

This implements the translation from EL profile class axioms to datalog rules, inspired by https://arxiv.org/abs/2008.02232.
- `EliAxiom` enum — the EL fragment of OWL class axioms
- `eli_axiom_extractor` — pattern-matches OWL `ClassAxiom` into `Option<EliAxiom>`
- `eli2rl` — generates datalog `Rule`s from `EliAxiom`

---

## Phase 5 — `owl2rl2datalog` crate

New crate `owl2rl2datalog/` depending on `owl_ontology`, `eli`, `dag_rdf`, `datalog`.

Translate `DagSemTools.OWL2RL2Datalog` (`Library.fs`, `Equality.fs`).

Implements section 4.3 of https://www.w3.org/TR/owl2-profiles/#OWL_2_RL :
- `owl2_datalog(ontology, resources) -> Vec<Rule>` — main entry point
- `object_property_axiom2datalog`, `data_property_axiom2datalog`, `owl_axiom2datalog`
- `get_class_expression_resource`, `get_object_property_expression_resource`
- Many cases currently have `failwith "todo"` in F# — these become `todo!()` initially

---

## Phase 6 — Parser crates

Four crates, each with a `build.rs` that runs the ANTLR4 tool:

### `turtle_parser`
Depends on `dag_rdf`. Translates `DagSemTools.Turtle.Parser`.
- Grammar: reuse `grammars/turtle/TurtleDoc.g4` and `TriGDoc.g4`
- Visitors: `IriGrammarVisitor`, `ResourceVisitor`, `StringVisitor`, `PredicateObjectListVisitor`, `TurtleListener`
- Entry point: `parse(input: &str, datastore: &mut Datastore)`

### `manchester_parser`
Depends on `owl_ontology`, `dag_rdf`. Translates `DagSemTools.Manchester.Parser`.
- Grammar: reuse `grammars/manchester/Manchester.g4`, `Concept.g4`, `ManchesterCommonTokens.g4`
- Visitors: `ManchesterVisitor`, `ConceptVisitor`, `FrameVisitor`, `ClassAssertionVisitor`, etc.
- Entry point: `parse(input: &str, datastore: &mut Datastore) -> Ontology`

### `sparql_parser`
Depends on `dag_rdf`. Translates `DagSemTools.Sparql.Parser`.
- Grammar: reuse `grammars/sparql/Sparql.g4`
- Produces `SelectQuery` (in `dag_rdf::query`)

### `datalog_parser`
Depends on `datalog`, `dag_rdf`. Translates `DagSemTools.Datalog.Parser`.
- Grammar: reuse `grammars/datalog/Datalog.g4`
- Produces `Vec<Rule>`

---

## Phase 7 — `api` / root crate integration

The root crate `dagalog` becomes the integration layer (mirrors `DagSemTools.Api`):
- `Graph` / `Dataset` structs wrapping `Datastore`
- `load_trig(path) -> Dataset` calling `turtle_parser`
- `answer_select_query(query: &str) -> Results` calling `sparql_parser`
- `load_ontology(path) -> Ontology` calling `manchester_parser` or `turtle_parser`
- `reason(ontology, datastore)` — calling `owl2rl2datalog` then `datalog::reasoner::evaluate`

---

## Suggested implementation order

1. `dag_rdf`: add `query.rs` (Term, QuadPattern), `datastore.rs`, fix default graph ID
2. `datalog` crate: types + `datalog.rs` module + `reasoner.rs` (naive materialise)
3. `datalog` crate: `stratifier.rs` + `unification.rs`
4. `owl_ontology` crate: pure data types
5. `eli` crate
6. `owl2rl2datalog` crate
7. `turtle_parser` crate (needed for loading any real data)
8. Wire up `dagalog` root for end-to-end reasoning
9. `manchester_parser`, `sparql_parser`, `datalog_parser`

---

## Notes on translation idioms

- F# discriminated unions → Rust `enum`
- F# records → Rust `struct`
- F# `Map<K,V>` → `HashMap<K,V>` (or `BTreeMap` where ordering matters)
- F# `Seq` (lazy) → Rust iterators (`impl Iterator<Item=...>`)
- F# `Option.bind` / `Option.map` chains → Rust `Option::and_then` / `Option::map`
- F# `Seq.fold` → `Iterator::fold`
- F# `List.collect` → `Iterator::flat_map`
- F# mutable class fields → Rust `&mut self` methods or `Cell`/`RefCell` where needed
- F# interface implementations (e.g. `ITripleTable`) → Rust traits
- Logging (`Serilog.ILogger`) → `log` crate traits (`log::warn!`, `log::error!`)
- No equivalent to `failwith "todo"` found during review → use `todo!()` macro
