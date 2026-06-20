# RML Core Plan: CSV Ingestion + RDF Mapping

## Goal

Add a new `rml` crate that reads CSV files and maps them to RDF triples using
RML (RDF Mapping Language) mapping documents. This gives data engineers a
standard declarative way to bring structured tabular data into dagalog without
writing Rust code.

The execution model follows the relational algebra approach described in Freund
et al., "Efficient Knowledge Graph Construction Based on Optimized Plans"
(SEMANTICS 2025). An RML mapping is compiled to a **logical plan** (a tree of
relational algebra operators), which is then optimized (constant folding) and
translated to a **physical plan** (a Volcano-style pull pipeline). This
separates concerns cleanly and enables optimizations without touching the
execution core.

---

## Spec and paper references

- RML 1.0 (W3C) — <https://www.w3.org/TR/rml/>
- R2RML (predecessor, SQL sources) — <https://www.w3.org/TR/r2rml/>
- RML test cases — <https://github.com/kg-construct/rml-test-cases>
- Percent-encoding — <https://www.rfc-editor.org/rfc/rfc3986#section-2.1>
- Freund et al., ESWC 2025 — relational algebra execution model, constant
  folding, heuristic scheduling https://ebooks.iospress.nl/doi/10.3233/SSW62 (reference implementation: konverter)

The target namespace is the new W3C RML 1.0: `http://w3id.org/rml/`
(abbreviated `rml:` throughout this document). The older Dimou-lab namespace
(`http://semweb.mmlab.be/ns/rml#`) is noted in the compatibility section below.

---

## Scope of "RML Core" (this plan)

**In scope:**
- `rml:TriplesMap`, `rml:LogicalSource`, `rml:SubjectMap`, `rml:PredicateObjectMap`
- `rml:PredicateMap`, `rml:ObjectMap`, `rml:GraphMap`
- Term map types: `rml:template`, `rml:constant`, `rml:reference`
- Term types: `rml:IRI`, `rml:BlankNode`, `rml:Literal`
- `rml:language`, `rml:datatype` on ObjectMaps
- Shorthand properties: `rml:subject`, `rml:predicate`, `rml:object`, `rml:graph`
- CSV `LogicalSource` (`rml:referenceFormulation rml:CSV`)
- Class shorthand: `rml:class` on SubjectMap
- Logical plan construction from AST
- Constant folding optimization
- Volcano-style physical execution

**Deferred to later phases:**
- JSON source (JSONPath references) → see `PIPELINE_BACKLOG.md`
- XML source (XPath references)
- SQL/JDBC sources
- `rml:JoinCondition` (cross-source joins — modeled in the plan but not executed)
- Nested iteration (for JSON arrays)
- FunctionMap (FNML function calls)
- Partitioning and heuristic scheduling (parallelism optimization)
- Old Dimou-lab namespace compatibility shim

---

## Crate: `rml`

New workspace member. Depends on: `ingress`, `dag_rdf`, `turtle` (to parse the
mapping file), and the `csv` crate (new external dependency).

```
rml/
├── Cargo.toml
└── src/
    ├── lib.rs          — pub API
    ├── ast.rs          — RML AST (TriplesMap, SubjectMap, TermMap, …)
    ├── loader.rs       — Turtle mapping file → MappingDocument (AST)
    ├── plan.rs         — LogicalPlan, GenerationLogic, FormatFunction types
    ├── translate.rs    — MappingDocument → Vec<LogicalPlan>
    ├── optimizer.rs    — constant_fold: LogicalPlan → LogicalPlan
    ├── engine.rs       — physical execution: Volcano-style iterator pipeline
    └── sources/
        ├── mod.rs      — RawRow type
        └── csv.rs      — CsvSource (csv crate)
```

---

## Pipeline overview

```
Turtle mapping file
      │
      ▼ loader.rs
MappingDocument (AST: TriplesMap, SubjectMap, …)
      │
      ▼ translate.rs
Vec<LogicalPlan>   (Scan → Projection trees; Join trees for rml:JoinCondition)
      │
      ▼ optimizer.rs  (constant_fold)
Vec<LogicalPlan>   (constant GenerationLogic entries pre-evaluated)
      │
      ▼ engine.rs
Physical pipeline  (nested Rust iterators, Volcano-style)
      │
      ▼ Datastore::add_quad
Quads in the Datastore
```

---

## RML AST types (`ast.rs`)

The AST is a faithful representation of what the RML mapping document says.
It is not executed directly; `translate.rs` converts it to a `LogicalPlan`.

```rust
pub struct MappingDocument {
    pub triples_maps: Vec<TriplesMap>,
}

pub struct TriplesMap {
    pub id: IriReference,
    pub logical_source: LogicalSource,
    pub subject_map: SubjectMap,
    pub predicate_object_maps: Vec<PredicateObjectMap>,
}

pub struct LogicalSource {
    pub source: LogicalSourceRef,
    pub reference_formulation: ReferenceFormulation,
    pub iterator: Option<String>,  // for JSON/XML; unused for CSV
}

pub enum LogicalSourceRef {
    File(PathBuf),
    // Url(Url),  — deferred
}

pub enum ReferenceFormulation {
    Csv,
    // JsonPath,  — deferred
}

pub struct SubjectMap {
    pub term_map: TermMap,
    pub term_type: TermType,       // resolved effective type (default: Iri)
    pub classes: Vec<IriReference>,
    pub graph_maps: Vec<GraphMap>,
}

pub struct PredicateObjectMap {
    pub predicate_maps: Vec<(TermMap, TermType)>,
    pub object_maps: Vec<ObjectMap>,
    pub graph_maps: Vec<GraphMap>,
}

pub struct ObjectMap {
    pub term_map: TermMap,
    pub term_type: TermType,       // resolved: Iri unless language/datatype → Literal
    pub language: Option<String>,
    pub datatype: Option<IriReference>,
    pub parent_triples_map: Option<IriReference>,  // joins — deferred
}

pub struct GraphMap {
    pub term_map: TermMap,
}

pub enum TermMap {
    Template(String),         // rml:template "http://example.com/{id}"
    Constant(GraphElement),   // rml:constant <iri> or "literal"
    Reference(String),        // rml:reference "column_name"
}

pub enum TermType {
    Iri,
    BlankNode,
    Literal,
}
// TermType defaults (RML spec §TermMap):
//   SubjectMap   → Iri
//   PredicateMap → Iri
//   ObjectMap    → Iri, but Literal if rml:language or rml:datatype is present
//   GraphMap     → Iri
// Explicit rml:termType overrides the default.
// loader.rs resolves the effective TermType so downstream code never re-derives it.
```

---

## Logical plan types (`plan.rs`)

The logical plan is the relational algebra representation of the mapping.
Each `TriplesMap` in the AST becomes one `Projection(Scan(...))` tree (or
`Projection(Join(Scan, Scan))` when a `JoinCondition` is present).

```rust
pub enum LogicalPlan {
    Scan(LogicalScan),
    Projection(LogicalProjection),
    Join(LogicalJoin),
}

pub struct LogicalScan {
    pub source: LogicalSourceRef,
    pub reference_formulation: ReferenceFormulation,
    pub iterator: Option<String>,
}

pub struct LogicalProjection {
    pub input: Box<LogicalPlan>,
    // Spec list A from the paper: [(output attribute, how to generate the term)]
    // Before constant folding: may include Dynamic for constant-valued TermMaps.
    // After constant folding:  all constant terms are Constant(GraphElementId).
    pub attrs: Vec<(OutputAttr, GenerationLogic)>,
}

pub struct LogicalJoin {
    pub left: Box<LogicalPlan>,
    pub right: Box<LogicalPlan>,
    pub condition: JoinCondition,
    pub algorithm: JoinAlgorithm,
}

pub struct JoinCondition {
    pub left_column: String,
    pub right_column: String,
}

pub enum JoinAlgorithm {
    HashJoin,  // default; nested-loop deferred
}

pub enum OutputAttr {
    Subject,
    Predicate,
    Object,
    Graph,
}
```

### GenerationLogic and FormatFunction

This is the key type that bridges the logical plan to per-row execution.
After constant folding, the `Constant` variant requires no per-row work.

```rust
pub enum GenerationLogic {
    /// Pre-evaluated constant: same GraphElement for every row.
    /// Produced by constant_fold() for any TermMap that has no column references.
    Constant(GraphElement),
    /// Must be evaluated per row using the format function.
    Dynamic(FormatFunction),
}

pub struct FormatFunction {
    pub pattern: TermPattern,
    pub term_type: TermType,
    // encode is not stored — it is always `matches!(term_type, TermType::Iri)`
}

pub enum TermPattern {
    /// "{column}" template string
    Template(String),
    /// Direct column reference (the column value becomes the whole term lexical form)
    Reference(String),
}
```

Applying a `GenerationLogic` to a row:
- `Constant(g)` → `Some(g)` (ignores row)
- `Dynamic(FormatFunction { Template(t), Iri })` → expand template with
  percent-encoded column values → intern as IRI in `GraphElementManager`
- `Dynamic(FormatFunction { Template(t), Literal })` → expand template
  verbatim (no encoding) → create `RdfLiteral`
- `Dynamic(FormatFunction { Reference(col), Iri })` → percent-encode
  `row[col]` → intern as IRI
- `Dynamic(FormatFunction { Reference(col), Literal })` → use `row[col]`
  verbatim as literal lexical form
- If any column referenced is absent or empty → `None` (triple skipped)

---

## AST → logical plan translation (`translate.rs`)

```rust
pub fn translate(mapping: &MappingDocument) -> Vec<LogicalPlan>
```

For each `TriplesMap`:
1. Build a `LogicalScan` from its `LogicalSource`
2. Build the projection spec list `A`: one entry per (predicate, object) pair
   across all `PredicateObjectMap`s, plus entries for `rml:class` shortcuts
   (these become `(Predicate, Constant(rdf:type))` + `(Object, Constant(class_iri))`)
3. Convert each `TermMap` → `GenerationLogic::Dynamic(FormatFunction {...})`.
   Note: `TermMap::Constant` is also initially `Dynamic` here; constant folding
   converts it in the next step.
4. Wrap the scan in a `LogicalProjection` with the spec list

`rml:class` expansion: one `(Subject, …), (Predicate, rdf:type), (Object, class_iri)`
row is emitted per class per source row. This is modeled as additional projection
entries alongside the normal PredicateObjectMap entries, sharing the same Scan.

---

## Constant folding (`optimizer.rs`)

```rust
pub fn constant_fold(
    plans: Vec<LogicalPlan>,
    elements: &mut GraphElementManager,
) -> Vec<LogicalPlan>
```

Walk each `LogicalProjection`'s spec list. For each `(attr, GenerationLogic::Dynamic(ff))`:
- If `ff.pattern` is a `Template` with **no `{...}` placeholders**, or if
  the original `TermMap` was `TermMap::Constant(g)`, the value is the same
  for every row.
- Pre-evaluate it: intern the constant IRI or literal in `elements`, get back
  a `GraphElement`.
- Replace with `GenerationLogic::Constant(element)`.

This pre-evaluation happens once at plan-build time. During execution, constant
entries cost only a clone per row rather than string formatting + interning.

The paper shows ~7–10% speedup from this optimization (larger gains on bigger
datasets with more constant predicates, which is the common case — predicates
are almost always constant IRIs).

---

## CSV source (`sources/csv.rs`)

```rust
pub type RawRow = HashMap<String, String>;  // column name → cell value

pub struct CsvSource {
    path: PathBuf,
    delimiter: u8,     // default b','
}

impl CsvSource {
    pub fn rows(&self) -> impl Iterator<Item = Result<RawRow, RmlError>> + '_
}
```

Uses `csv::ReaderBuilder` with `has_headers(true)`. Empty cells are kept as
empty strings (not omitted), matching RML spec behaviour.

---

## Template expansion (`sources/mod.rs` or inline in `engine.rs`)

Percent-encoding applies only when the resulting term type is IRI:

- **IRI**: column values percent-encoded (RFC 3986 §2.1, unreserved chars
  `A-Za-z0-9-._~` pass through; everything else encoded)
- **Literal**: column values used verbatim — `3.14`, `"hello, world"`,
  `2024-01-01` must not be corrupted
- **BlankNode**: no encoding

```rust
/// Expand a template, encoding substituted values iff `encode` is true.
/// Returns None if any referenced column is absent or empty.
pub fn expand_template(template: &str, row: &RawRow, encode: bool) -> Option<String>

pub fn percent_encode(value: &str) -> String
```

The caller sets `encode = matches!(term_type, TermType::Iri)`.

---

## Physical execution (`engine.rs`)

The physical layer follows the Volcano model: each operator is a Rust
`Iterator`. Operators are composed by nesting — the terminal `Serialize`
operator drives the whole pipeline by calling `next()` on its input.

```rust
pub fn execute(
    plans: &[LogicalPlan],
    datastore: &mut Datastore,
) -> Result<(), RmlError>
```

For each `LogicalPlan`:
1. Construct the physical pipeline from the logical plan tree
2. Drive it to completion: iterate projected rows, insert each as a quad

**Physical Scan**: wraps `CsvSource::rows()`, yields `RawRow`.

**Physical Projection**: wraps the scan iterator. For each `RawRow`:
- For each `(attr, logic)` in the spec list:
  - `Constant(g)` → resolve `g` to `GraphElementId` (already interned)
  - `Dynamic(ff)` → call `expand_template` / reference lookup → intern in
    `GraphElementManager` → `GraphElementId`
- If any mandatory term produces `None`, skip the row (no quad emitted)
- Yield `ProjectedRow { subject, predicate, object, graph }`

**Physical Serialize** (terminal operator): consumes `ProjectedRow` values and
calls `datastore.add_quad(subject, predicate, object, graph)`.

```rust
struct ProjectedRow {
    subject: GraphElementId,
    predicate: GraphElementId,
    object: GraphElementId,
    graph: GraphElementId,
}
```

The graph is the default graph ID when no `GraphMap` is present.

---

## Partitioning and heuristic scheduling (deferred optimization)

The paper describes two further optimizations at the physical level:

**Partitioning**: divide the physical plans into groups that produce disjoint
sets of RDF triples (same Subject+Predicate can only appear in one group).
Groups can then execute in parallel without synchronizing on `Datastore` writes.
Reference: the partitioning algorithm from [Jozashoori et al. 2020].

**Heuristic scheduling**: within a concurrent executor, process the group with
the largest total input file size first. This overlaps long-running and
short-running groups. The paper shows ~15% speedup on multi-source benchmarks.

Both are deferred until the sequential implementation is passing all W3C test
cases. The plan's `LogicalPlan` and physical operator types are already shaped
to accommodate them without structural changes.

---

## Public API (`lib.rs`)

```rust
/// Parse an RML mapping, build and optimize the logical plan, and execute it,
/// inserting generated quads into `datastore`.
/// CSV source paths are resolved relative to `base_dir`.
pub fn apply_rml_mapping(
    mapping_path: &Path,
    base_dir: &Path,
    datastore: &mut Datastore,
) -> Result<(), RmlError>

#[derive(Debug, thiserror::Error)]
pub enum RmlError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Mapping parse error: {0}")]
    MappingParse(String),
    #[error("CSV error in {file}: {source}")]
    Csv { file: PathBuf, source: csv::Error },
    #[error("Missing required property {property} on {subject}")]
    MissingProperty { subject: String, property: String },
}
```

---

## CLI integration

```
dagalog --rml mapping.ttl [--rml-base /path/to/data/] [--output output.ttl]
```

Can be combined with existing flags:
```
dagalog --load ontology.ttl --rml mapping.ttl --reason --output enriched.ttl
```

HTTP endpoint: add a `POST /rml` route — deferred until the library API is solid.

---

## Test plan (TDD phases)

### Phase 1 — AST + plan types
Define all types in `ast.rs` and `plan.rs`. No tests needed (pure data types).

### Phase 2 — Loader (red → green)
Write ignored integration tests in `rml/tests/loader_tests.rs` using small
inline Turtle strings as mapping fixtures. One test per feature:
- TriplesMap with template subject
- Constant predicate (TermMap::Constant)
- Reference object
- `rml:class` shorthand
- Named graph via GraphMap
- Shorthand `rml:subject` / `rml:predicate` / `rml:object`
- TermType resolution (ObjectMap with `rml:language` → Literal default)

### Phase 3 — CSV source (red → green)
Write ignored tests in `rml/tests/csv_tests.rs`:
- Happy path: read header + rows
- Empty file
- Missing file → error
- Delimiter override

### Phase 4 — Template expansion (red → green)
Unit tests in `rml/tests/template_tests.rs`:
- Simple substitution (IRI mode) — special chars in column value are encoded
- Literal mode — same template, same value, no encoding (`3.14` stays `3.14`)
- Multiple placeholders
- Empty column → None (regardless of mode)

### Phase 5 — Translation + constant folding (red → green)
Unit tests in `rml/tests/plan_tests.rs`:
- `translate()` produces correct Projection spec list for a simple TriplesMap
- Constant predicate IRI becomes `Dynamic(Template("...", Iri))` before folding
- After `constant_fold()`, that entry is `Constant(graph_element)`
- Template with column reference stays `Dynamic` after folding

### Phase 6 — End-to-end W3C test cases (red → green)
Integration tests using fixtures from the W3C RML test cases repo (CSV subset).
Fixtures (input CSV + mapping Turtle + expected N-Triples) copied into
`rml/tests/fixtures/`. Expected output verified by sorting both actual and
expected as N-Triples lines and comparing.

Initial set:
- `RMLTC0001a` — simple mapping, one column IRI
- `RMLTC0002a` — multiple predicates
- `RMLTC0003a` — blank node subject
- `RMLTC0007a` — language tag
- `RMLTC0007b` — datatype
- `RMLTC0009a` — named graph
- `RMLTC0010a` — class shorthand

---

## Old namespace compatibility

The Dimou-lab RML namespace (`http://semweb.mmlab.be/ns/rml#`) predates the
W3C spec and is used in most existing RML tooling. Many real-world mappings
mix `rr:` (R2RML) with `rml:` (Dimou extensions).

Compatibility shim: in `loader.rs`, after loading the mapping Turtle, replace
the Dimou namespace IRIs with their W3C equivalents in the temporary
`Datastore` before walking quads. This is a string substitution on IRI text at
load time, not a schema mapping. Implement once the W3C path is green.

---

## Dependencies to add

`rml/Cargo.toml`:
```toml
[dependencies]
ingress = { path = "../ingress" }
dag_rdf = { path = "../dag_rdf" }
turtle = { path = "../turtle" }
thiserror = "2"
csv = "1"
```
