# OTTR Template Expansion Plan

> Tracked under [#13 OTTR template expansion epic](https://github.com/daghovland/rdf-datalog/issues/13).
> The grammar below targets the real stOTTR (Specialised Template Notation) syntax.
> Exact tokens for optional parameters and list expanders are marked **[verify]** below — confirm against
> <https://spec.ottr.xyz/stOTTR/> and real lutra fixtures during further implementation phases.

## Goal

Add an `ottr` crate implementing OTTR (Reasonable Ontology Templates)
template definition and expansion. OTTR is complementary to RML: RML maps raw
tabular/hierarchical data to flat RDF; OTTR templates define typed, reusable,
composable patterns for generating well-structured RDF instances, with
parameter types, nested template calls, optional parameters, and list
expansion.

Pipeline position (unchanged from backlog): data comes in via RML, is
optionally reshaped/expanded by OTTR templates, then OWL-RL reasoning and
SHACL validation run on the result.

```
RML mapping (rml crate)  →  OTTR template expansion (ottr crate)  →  reasoning (datalog crate)  →  SHACL (shacl crate)
```

## Spec references

- OTTR specification — <https://spec.ottr.xyz/>
- stOTTR concrete syntax — <https://spec.ottr.xyz/stOTTR/>
- wOTTR (RDF/Turtle representation of templates) — <https://spec.ottr.xyz/wOTTR/>
- OTTR test suite (lutra) — <https://gitlab.com/ottr/lutra/lutra-test-suite>
- OTTR vocabulary namespace: `ottr:` = `http://ns.ottr.xyz/0.4/`
- Reference implementation (Java) for behavioural ground truth: <https://gitlab.com/ottr/lutra/lutra>

## Scope

**In scope (this plan, all phases):**
- stOTTR template definitions: signature (typed parameter list) + pattern body
- stOTTR instance files (template calls with literal/IRI/blank-node arguments)
- The base template `ottr:Triple` (the only base template strictly required —
  every higher-level template ultimately bottoms out in `ottr:Triple` calls)
- Nested (user-defined) template calls in a template body
- `none` arguments and optional parameters (suppress triples that reference
  an unbound optional parameter)
- List-typed arguments and the `cross` / `zipMin` expanders
- Non-recursive template definitions only (OTTR forbids template recursion;
  detect and error rather than infinite-loop)

**Deferred / explicitly out of scope:**
- `zipMax` expander (rare in practice; add only if a lutra fixture needs it)
- wOTTR (RDF-encoded templates) — stOTTR text syntax only for now
- Annotations / custom base templates beyond `ottr:Triple`
- Template libraries distributed as packages (`ottr:` import mechanism)
- Strict type checking — permissive at this phase (warn, don't error), same
  stance as the original backlog sketch

## Crate: `ottr`

New workspace member. Depends on `ingress` and `dag_rdf` only (no `turtle`
dependency — stOTTR is not Turtle and needs its own parser, same approach as
`sparql_parser`).

```
ottr/
├── Cargo.toml
└── src/
    ├── lib.rs            — pub API: load_templates, load_instances, expand
    ├── ast.rs            — TemplateDef, Parameter, Instance, Argument, term types
    ├── types.rs          — OttrType: BasicType (Iri/BlankNode/Literal(dt)), List, NEList, None
    ├── parser.rs         — nom-based stOTTR parser (template + instance grammar)
    ├── expander.rs        — recursive instance expansion → quads
    ├── base_templates.rs — built-in handling of ottr:Triple
    └── error.rs          — OttrError (thiserror)
```

## stOTTR core syntax

Prefix declarations are Turtle-style (`@prefix ex: <...> .`), reused verbatim.

### Template definition

```
ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type, foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .
```

Grammar sketch:
```
template_def     := prefix_decl* signature "::" "{" instance_list "}" "."
signature        := IRI "[" parameter_list? "]"
parameter_list   := parameter ("," parameter)*
parameter        := type? "?"variable ("=" default_value)?     # default_value [verify]
type             := basic_type | "List<" type ">" | "NEList<" type ">"
basic_type       := "ottr:IRI" | "ottr:BlankNode" | "ottr:Literal" | xsd_type
instance_list    := instance ("," instance)*
instance         := IRI "(" argument_list? ")" expander?
expander         := "|" ("cross" | "zipMin" | "zipMax")          # [verify exact placement]
argument_list    := argument ("," argument)*
argument         := term | list_literal | "none" | "++" "?"variable   # ++ marks list-expansion arg [verify]
list_literal     := "(" argument_list? ")"
term             := IRI | "?"variable | literal | blank_node
```

### Instance file (data)

```
ex:Person(<http://example.com/Alice>, "Alice") .
ex:Person(<http://example.com/Bob>, "Bob") .
```

Same `instance` production as above, terminated with `.` per statement,
top-level (no signature, no `::`).

### Optional parameters and `none`

A parameter marked optional (exact marker **[verify]** — likely a `?` suffix
on the type, e.g. `ottr:IRI? ?org`) may be bound to `none` at call time. Any
`ottr:Triple` (or nested instance argument) in the template body that
references that parameter is silently dropped from the expansion for that
call — this is the one piece of behaviour from the original backlog sketch
that *is* spec-accurate and should carry over unchanged.

### List expanders

`cross` and `zipMin` operate over one or more list-typed arguments in an
instance call, producing one expanded instance per combination (`cross`) or
per index up to the shortest list (`zipMin`). Deferred to Phase 7 — core
single-valued expansion lands first.

## AST types (`ast.rs`)

```rust
pub struct StottrDocument {
    pub templates: Vec<TemplateDef>,
    pub instances: Vec<Instance>,
}

pub struct TemplateDef {
    pub id: IriReference,
    pub parameters: Vec<Parameter>,
    pub body: Vec<Instance>,
}

pub struct Parameter {
    pub variable: String,
    pub ottr_type: OttrType,
    pub optional: bool,
    pub default: Option<Argument>,
}

pub struct Instance {
    pub template: IriReference,   // ottr:Triple for the base case
    pub arguments: Vec<Argument>,
    pub expander: Option<Expander>,
}

pub enum Expander {
    Cross,
    ZipMin,
}

pub enum Argument {
    Term(Term),
    List(Vec<Argument>),
    None,
    ListExpand(String),   // ++?variable
}

pub enum Term {
    Iri(IriReference),
    Variable(String),
    Literal(RdfLiteral),
    BlankNode(String),
}
```

## Type system (`types.rs`)

```rust
pub enum OttrType {
    Iri,
    BlankNode,
    Literal(Option<IriReference>),  // datatype, None = plain/LUB
    List(Box<OttrType>),
    NEList(Box<OttrType>),          // non-empty list
}
```

Type checking is permissive in this phase: argument/parameter type mismatches
produce a `log::warn!`, not an `OttrError`. Matches the backlog's original
stance and avoids blocking expansion on type-inference edge cases the lutra
suite likely covers more rigorously than dagalog needs initially.

## Parser (`parser.rs`)

nom-based combinator parser, structured like `sparql_parser` (own AST, no
dependency on the `turtle` crate's grammar since stOTTR's instance-call syntax
isn't Turtle). Two top-level entry points:

```rust
pub fn parse_stottr(input: &str) -> Result<StottrDocument, OttrError>
```

A single `StottrDocument` holds both `templates` and `instances` because real
stOTTR files (and lutra fixtures) commonly mix definitions and data in one
file, though the lutra test suite also splits them across files per test
case — `parse_stottr` is called once per file and documents merged by the
caller (`expander.rs` / test harness) when a test case spans multiple files.

## Expansion algorithm (`expander.rs`)

```rust
pub fn expand(
    templates: &HashMap<IriReference, TemplateDef>,
    instances: &[Instance],
    datastore: &mut Datastore,
) -> Result<(), OttrError>
```

For each top-level `Instance` call `T(a1, a2, …)`:
1. If `T == ottr:Triple`: resolve the three arguments to `GraphElement`s,
   insert as a quad into the default graph. `none` in any position skips
   the triple silently.
2. Otherwise, look up the template definition for `T`.
   - Arity check: `arguments.len() == parameters.len()` else
     `OttrError::ArityMismatch`.
   - Build a substitution: `variable → Argument` for each parameter.
   - Recurse into each `Instance` in the template body, substituting bound
     variables before recursing (an inner instance argument that is itself a
     variable is replaced by the caller's bound `Argument`).
   - If a body instance's substituted arguments include a `none` bound to a
     non-optional parameter slot used downstream, drop that single instance
     from expansion (not the whole call).
3. Before recursing, check `T` is not already on the current call stack
   (recursion guard) → `OttrError::RecursiveTemplate` if it is. OTTR forbids
   template self-recursion by spec; this guard turns a spec violation into a
   clear error instead of a stack overflow.
4. List expanders (`cross`, `zipMin`) are resolved *before* step 1/2: an
   instance with an `Expander` is first expanded into N ordinary instances
   (no expander), then each is processed as above. Implemented as a
   pre-pass: `resolve_expanders(instance) -> Vec<Instance>`.

## Base template (`base_templates.rs`)

Only `ottr:Triple(subject, predicate, object)` is built in for this phase.
Everything else is a user-defined template that bottoms out in calls to it
(directly or transitively).

## Public API (`lib.rs`)

```rust
pub fn load_stottr_str(input: &str) -> Result<StottrDocument, OttrError>
pub fn load_stottr_file(path: &Path) -> Result<StottrDocument, OttrError>

/// Merge multiple documents (e.g. a templates file + an instances file),
/// then expand all instances into datastore.
pub fn expand_documents(
    docs: &[StottrDocument],
    datastore: &mut Datastore,
) -> Result<(), OttrError>

#[derive(Debug, thiserror::Error)]
pub enum OttrError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Unknown template: {0}")]
    UnknownTemplate(String),
    #[error("Template {template} called with {got} arguments, expected {expected}")]
    ArityMismatch { template: String, got: usize, expected: usize },
    #[error("Recursive template definition: {0}")]
    RecursiveTemplate(String),
}
```

## CLI / Jupyter integration (deferred to final phase)

```
dagalog --load base.ttl --ottr templates.stottr --instances data.stottr --reason
```

Jupyter (`dagalog-kernel`, once this crate is ready):
```
%%ottr path/to/templates.stottr
ex:Person(<http://example.com/Alice>, "Alice") .
```

This is the `%%ottr` magic already named as pending in `JUPYTER_KERNEL_PLAN.md`
and `PIPELINE_BACKLOG.md`'s dependency diagram — no change needed there beyond
marking it unblocked once `ottr` lands.

## Test plan (TDD phases)

### Phase 1 — AST + type system
Pure data types in `ast.rs`, `types.rs`, `error.rs`. No tests (matches the
convention used for `rml::ast` / `rml::plan` in `RML_PLAN.md`).

### Phase 2 — Parser: template definitions (red → green)
`ottr/tests/parser_tests.rs`, inline stOTTR string fixtures:
- Signature with a single typed parameter
- Signature with multiple parameters, mixed types
- Body with a single `ottr:Triple` instance
- Body with multiple instances
- Prefix declarations resolved into IRIs

### Phase 3 — Parser: instance files (red → green)
- Single instance call, IRI arguments
- Literal arguments (plain, typed, language-tagged)
- Blank node arguments
- Multiple instances in one file

### Phase 4 — Base expansion, no nesting (red → green)
`ottr/tests/expander_tests.rs`:
- `ottr:Triple` instance directly → one quad in `Datastore`
- User template with one `ottr:Triple` in its body, one instance call →
  correct quad
- Same template called twice with different arguments → two sets of quads
- `none` argument in a non-optional triple position → triple omitted

### Phase 5 — Nested template calls (red → green)
- Template A's body calls template B; B's body has `ottr:Triple`
- Three-level nesting
- Recursive template definition (A calls A) → `OttrError::RecursiveTemplate`

### Phase 6 — Optional parameters (red → green)
- Parameter marked optional, instance called with `none` → triples
  referencing it dropped, others kept
- Default value substitution when argument omitted **[verify against spec
  whether stOTTR even allows omitted trailing arguments, or only `none`]**

### Phase 7 — List expanders (red → green)
- `cross` over one list argument
- `cross` over two list arguments (cartesian product)
- `zipMin` over two lists of unequal length (truncate to shorter)

### Phase 8 — lutra test suite fixtures (red → green)
Copy fixture sets (`.stottr` template + instance files, expected N-Triples)
into `ottr/tests/fixtures/`. Compare actual vs. expected as sorted N-Triples
lines, same pattern as `rml/tests/fixtures` + `end_to_end.rs`. Start with the
simplest non-list fixtures; add list-expander fixtures once Phase 7 lands.

### Phase 9 — CLI + Jupyter integration (red → green)
- `dagalog --ottr ... --instances ...` end-to-end smoke test
- `%%ottr` kernel magic test in `dagalog-kernel` (depends on `ottr` crate
  being on the dependency graph — add `ottr = { path = "../ottr" }` to
  `dagalog-kernel/Cargo.toml` at this point, not before)

## Dependencies to add

`ottr/Cargo.toml`:
```toml
[package]
name = "ottr"
version = "0.1.0"
edition = "2024"

[dependencies]
ingress = { path = "../ingress" }
dag-rdf = { path = "../dag_rdf" }
nom = "7.1"
thiserror = "2"
log = "0.4"
```

Add `"ottr"` to the root `Cargo.toml` workspace `members` list once Phase 1
stubs exist.
