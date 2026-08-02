# wOTTR support plan

Epic/issue: [#246](https://github.com/daghovland/rdf-datalog/issues/246) (part of the OTTR epic
[#13](https://github.com/daghovland/rdf-datalog/issues/13)). Working branch:
`feat/246-ottr-wottr`.

wOTTR spec consulted directly: <https://spec.ottr.xyz/wOTTR/0.4.5/> (vocabulary:
`core-vocabulary.owl.ttl`, grammar/examples: `index.html`). Namespace `ottr:` =
`http://ns.ottr.xyz/0.4/` (same as stOTTR's, matching `docs/plans/OTTR_PLAN.md`).

## Goal

Add a second, format-agnostic front end alongside `parser::parse_stottr`: read
`ottr:Template`/`ottr:Instance`-shaped RDF triples out of an already-populated
`dag_rdf::Datastore` and build the *same* `ast::StottrDocument` that
`parse_stottr` builds from stOTTR text. `expander::expand`/`expand_documents`
need zero changes — this is purely a new front end.

## Vocabulary → AST mapping

| wOTTR triple pattern | AST target |
|---|---|
| `?t a ottr:Template` | one `TemplateDef` per such subject `?t` (must resolve to an IRI, non-IRI/blank-node template ids are skipped with `log::warn!`) |
| `?t ottr:parameters (?p1 ?p2 ...)` (RDF list) | `TemplateDef.parameters: Vec<Parameter>`, one per list element, in list order |
| `?p ottr:variable ?varNode` (`?varNode` is a blank node) | `Parameter.variable`: a `String` key derived from `?varNode`'s `GraphElementId` (stable per parse, unique across the whole document since blank-node ids are never reused) — this same key is what parameter usages inside `ottr:pattern` resolve to via `Term::Variable` |
| `?p ottr:type X` | `Parameter.ottr_type`: `ottr:IRI`→`OttrType::Iri`, `ottr:BlankNode`→`OttrType::BlankNode`, `ottr:Literal`→`OttrType::Literal(None)`, other atomic IRI (e.g. `xsd:string`)→`OttrType::Literal(Some(iri))`, RDF list `(rdf:List X)`/`(rdf:NEList X)`→ `OttrType::List`/`NEList` wrapping the recursively-mapped inner type. Missing `ottr:type` → `OttrType::Iri` (matches stOTTR parser default). |
| `?p ottr:modifier ottr:optional` | `Parameter.optional = true` (`ottr:nonBlank` is read but has no AST slot yet — permissive, same stance as existing "warn don't error" policy) |
| `?p ottr:default V` | `Parameter.default = Some(Argument::Term(...))` |
| `?t ottr:pattern ?i1, ?i2, ...` (multi-valued, **not** a list — the body is an unordered *set* of pattern instances per spec) | `TemplateDef.body: Vec<Instance>` |
| `?i ottr:of <TemplateIri>` | `Instance.template` |
| `?i ottr:modifier ottr:cross` / `ottr:zipMin` | `Instance.expander = Some(Expander::Cross)` / `Some(Expander::ZipMin)`. `ottr:zipMax` has no AST variant yet ([tracked as a known gap in the original plan](../plans/OTTR_PLAN.md)) — read but dropped with `log::warn!`. |
| `?i ottr:values (v1 v2 ...)` (compact form, RDF list of terms) | `Instance.arguments: Vec<Argument>`, one per list element (primary encoding this PR targets) |
| `?i ottr:arguments (a1 a2 ...)` (canonical form, RDF list of `ottr:Argument` blank nodes, each `ottr:value V` + optional `ottr:modifier ottr:listExpand`) | same `Vec<Argument>`, resolved through the extra `ottr:value` indirection |
| a term `V` that is a blank node matching some enclosing template's parameter-variable blank node | `Argument::Term(Term::Variable(key))` |
| a term `V` that is `ottr:none` | `Argument::None` |
| a term `V` that is itself the head of an RDF list (`(a b c)`) | `Argument::List(vec![...])`, each element resolved recursively — this is what lets `expand_cross`/`expand_zip_min` (which key off `Argument::List(_)` regardless of any `listExpand` marker — a pre-existing, already-permissive trait of `expander.rs` shared with stOTTR) actually iterate it |
| any other term (IRI, literal, real anonymous blank node) | `Argument::Term(Term::Iri/Literal/BlankNode)`, resolved via `GraphElementManager::get_graph_element` |
| a top-level `ottr:Instance` that is **not** the object of any `ottr:pattern` triple | a document-level `Instance` in `StottrDocument.instances` (the wOTTR equivalent of a bare stOTTR instance-file call) |

RDF lists (`ottr:parameters`, `ottr:values`, `ottr:arguments`, and nested list
arguments) are all walked the same way: follow `rdf:first`/`rdf:rest` from the
list head to `rdf:nil`.

## Why no `expander.rs`/`ast.rs` changes are needed

Confirmed by reading `expander.rs`: it only ever inspects `ast::Instance`/
`ast::Argument`/`ast::TemplateDef` — nothing stOTTR-syntax-specific. A
`StottrDocument` built by the wOTTR front end is indistinguishable, from
`expand`'s point of view, from one built by `parse_stottr`.

## Entry points

- `wottr::parse_wottr(datastore: &Datastore) -> Result<StottrDocument, OttrError>`
  — the core deliverable, reads templates/instances already loaded into a
  `Datastore` (e.g. via `turtle::parse_turtle`).
- `wottr::parse_wottr_str(text: &str) -> Result<StottrDocument, OttrError>`
  — convenience wrapper used by tests: parses `text` as Turtle into a fresh
  `Datastore` via the `turtle` crate, then delegates to `parse_wottr`. Also the
  natural shape for later content-type-based dispatch (`text/turtle` vs the
  existing stOTTR text) in the HTTP/Jupyter surfaces mentioned in the issue,
  should those get wired up (tracked, not committed to in this PR — see
  scope note in the issue re: #247).

## Scope for this PR

In:
- Template parsing (id, parameters incl. types/optional/default, pattern body).
- Instance parsing, both `ottr:values` (compact) and `ottr:arguments`
  (canonical) encodings.
- Blank-node variables, `ottr:none`, nested RDF-list arguments (for
  cross/zipMin).
- Top-level (document) instances.
- `docs/user/ottr-templates.md` wOTTR section.

Deferred (noted inline with links, not silently dropped):
- CLI/Jupyter/HTTP wiring — stOTTR itself isn't wired into these surfaces yet
  either (issue #247); wiring wOTTR ahead of stOTTR would be inconsistent.
  Left for #247 or a follow-up once stOTTR wiring lands.
- `ottr:zipMax` expansion (no `ast::Expander` variant currently — same gap
  already noted for stOTTR in `OTTR_PLAN.md`).
- Custom `ottr:BaseTemplate`s beyond the built-in `ottr:Triple`.
- `ottr:nonBlank` enforcement (permissive/warn-only, matching existing stance).

## Test plan (fixtures under `ottr/tests/fixtures/wottr/*.ttl`)

1. No-parameter template + one `ottr:Triple` pattern instance + one top-level
   instance call.
2. Template with untyped parameters (default `ottr:IRI`).
3. Template with explicitly typed parameters (`ottr:IRI`, `ottr:BlankNode`,
   `ottr:Literal`, an `xsd:` datatype).
4. Multiple/nested user-defined templates calling each other.
5. `ottr:none` compact-value argument.
6. `ottr:cross` instance modifier with a nested-list argument value.
7. Canonical `ottr:arguments` encoding (equivalence with compact `ottr:values`
   form, per the spec's own worked example).

Each test is written first, `#[ignore]`d, with the module stubbed to compile;
unignored one at a time as the corresponding capability is implemented.
