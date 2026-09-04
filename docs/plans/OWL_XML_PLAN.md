# OWL 2 XML Serialization Parser Plan

Tracked in epic: [#564](https://github.com/daghovland/rdf-datalog/issues/564) — "Epic: OWL 2 XML Serialization parser".
This issue: [#605](https://github.com/daghovland/rdf-datalog/issues/605) — plan doc + core structure.
Follow-up sub-issues (already filed, not this issue's scope):
[#606](https://github.com/daghovland/rdf-datalog/issues/606) (class expressions/axioms),
[#607](https://github.com/daghovland/rdf-datalog/issues/607) (object/data property axioms),
[#608](https://github.com/daghovland/rdf-datalog/issues/608) (ABox axioms + annotations),
[#609](https://github.com/daghovland/rdf-datalog/issues/609) (CLI/notebook wiring).

This document plans a new `owl_xml_parser` crate that reads
[OWL 2 XML Serialization](https://www.w3.org/TR/owl2-xml-serialization/)
(`.owx`/`.owl`) documents and produces an `owl_ontology::Ontology` — the
same target type `manchester_parser` and `owl_functional_parser` produce.
Per the epic, this is a **parser only** (input); no serializer is planned
unless a concrete need for one shows up later.

---

## What is OWL/XML?

OWL/XML is a distinct concrete syntax from both RDF/XML and OWL 2
Functional-Style Syntax. It represents every axiom, class expression, and
entity as its own XML element, named identically to the corresponding
Functional-Style Syntax keyword (`<SubClassOf>`, `<ObjectIntersectionOf>`,
`<ObjectSomeValuesFrom>`, ...), with arguments as child elements rather than
parenthesized text. It is the format Protégé exports by default, and what
Gene Ontology (`go.owl`, motivating this epic — see #564) ships as.

```xml
<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#"
          xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#"
          ontologyIRI="http://example.org/pizza">
    <Prefix name="" IRI="http://example.org/pizza#"/>
    <Prefix name="owl" IRI="http://www.w3.org/2002/07/owl#"/>
    <Import>http://example.org/imported</Import>
    <Annotation>
        <AnnotationProperty IRI="http://www.w3.org/2000/01/rdf-schema#comment"/>
        <Literal>An example ontology</Literal>
    </Annotation>
    <Declaration>
        <Class IRI="#Pizza"/>
    </Declaration>
    <Declaration>
        <Class IRI="#Food"/>
    </Declaration>
    <Declaration>
        <ObjectProperty IRI="#hasTopping"/>
    </Declaration>
    <SubClassOf>
        <Class IRI="#Pizza"/>
        <Class IRI="#Food"/>
    </SubClassOf>
    <ObjectPropertyDomain>
        <ObjectProperty IRI="#hasTopping"/>
        <Class IRI="#Pizza"/>
    </ObjectPropertyDomain>
</Ontology>
```

Structurally this is much closer to `owl_functional_parser`'s
`Keyword(args...)` s-expressions than to Manchester's frame/precedence-ladder
syntax — every element name unambiguously identifies the production being
parsed, so (as with Functional-Style Syntax) no precedence climbing is
needed and no lookahead disambiguation is needed either. The recursion is
just XML-tree recursion instead of parenthesis recursion.

**Two IRI forms**, on every entity/expression element that names an IRI:
- `IRI="..."` — a full or relative IRI (relative IRIs resolve against
  `xml:base` / the ontology IRI, same rule as `#Pizza` above resolving
  against `http://example.org/pizza`).
- `abbreviatedIRI="prefix:localName"` — a CURIE resolved against the
  document's `<Prefix>` declarations, the XML-element equivalent of
  Functional-Style Syntax's `abbreviatedIRI` and Manchester's `prefix:local`.

---

## Target data model (do not invent new types)

Same as `manchester_parser`/`owl_functional_parser`:
`owl_ontology::Ontology` built from `owl_ontology::axioms` types (`Axiom`,
`ClassAxiom`, `ObjectPropertyAxiom`, `DataPropertyAxiom`, `Assertion`,
`AnnotationAxiom`, `Entity`, `Declaration`, `ClassExpression`,
`ObjectPropertyExpression`, `DataRange`, `Individual`, `Annotation`,
`AnnotationValue`). IRIs are `owl_ontology::FullIri(ingress::IriReference)`;
every OWL/XML IRI form (`IRI="..."` attribute, `abbreviatedIRI="..."`
attribute) is resolved to a `FullIri` before it reaches the AST, exactly as
`manchester_parser`/`owl_functional_parser` resolve their own IRI forms up
front — there is no partial/lazy IRI type in the target model.

`Ontology::new` takes `directly_imports_documents: Vec<IriReference>`,
`version: OntologyVersion`, `annotations: Vec<Annotation>`,
`axioms: Vec<Axiom>` — `Ontology` itself carries no prefix field (prefixes
are consumed while resolving IRIs, then discarded), matching the other two
parsers.

---

## Grammar productions in scope (this issue, #605)

Quoted (lightly reformatted) from the W3C spec,
<https://www.w3.org/TR/owl2-xml-serialization/>, the `Ontology` production
and its direct header children:

```
Ontology := '<Ontology' [ 'ontologyIRI=' IRI ] [ 'versionIRI=' IRI ] '>'
            { Prefix } { Import } { Annotation } { Axiom } '</Ontology>'
Prefix   := '<Prefix' 'name=' quotedPrefixName 'IRI=' quotedIRI '/>'
Import   := '<Import>' IRI '</Import>'
Declaration := '<Declaration>' { Annotation } Entity '</Declaration>'
Entity   := Class | Datatype | ObjectProperty | DataProperty
          | AnnotationProperty | NamedIndividual
Class | Datatype | ObjectProperty | DataProperty
| AnnotationProperty | NamedIndividual
         := '<'Tag ('IRI=' quotedIRI | 'abbreviatedIRI=' quotedAbbreviatedIRI) '/>'
```

This issue covers exactly this subset: the `<Ontology>` root (with
`ontologyIRI`/`versionIRI` attributes), `<Prefix>` declarations, `<Import>`
elements, ontology-level `<Annotation>` elements (needed for the header —
axiom-level `<Annotation>` children on non-Declaration axioms are #606-608's
concern), and `<Declaration>` for all six `Entity` variants (including a
`Declaration`'s own leading `<Annotation>` children, which become the
`Vec<Annotation>` in `owl_ontology::Declaration = (Vec<Annotation>,
Entity)`).

**Deferred to later sub-issues** (already filed against epic #564, not
re-listed here as new follow-ups):
- [#606](https://github.com/daghovland/rdf-datalog/issues/606) — class
  expressions and class-level axioms (`<SubClassOf>`,
  `<EquivalentClasses>`, `<DisjointClasses>`, `<DisjointUnion>`, and every
  `ClassExpression`/restriction element).
- [#607](https://github.com/daghovland/rdf-datalog/issues/607) — object/data
  property axioms, property chains, `<HasKey>`.
- [#608](https://github.com/daghovland/rdf-datalog/issues/608) — ABox
  (individual) axioms and the remaining annotation-axiom elements
  (`<AnnotationAssertion>`, `<SubAnnotationPropertyOf>`, etc.), plus
  axiom-level `<Annotation>` children on axiom kinds other than
  `<Declaration>`.
- [#609](https://github.com/daghovland/rdf-datalog/issues/609) — CLI
  (`--ontology`/`-o`) and `dagalog-kernel` wiring.

---

## Scope (in / out) — summary table, this issue only

| Feature | In scope | Notes |
|---|---|---|
| `<Ontology>` root, `ontologyIRI`/`versionIRI` attributes | Yes | |
| `<Prefix name="..." IRI="..."/>`, incl. default `name=""` | Yes | |
| `<Import>IRI</Import>` | Yes | |
| Ontology-level `<Annotation>` (direct child of `<Ontology>`) | Yes | |
| `<Declaration>` for all six `Entity` variants, incl. leading `<Annotation>` children | Yes | |
| `IRI="..."` and `abbreviatedIRI="..."` resolution, relative-IRI resolution against ontology IRI | Yes | |
| `xml:base` attribute overriding the ontology-IRI-as-base default | No | not observed in practice ontologies checked so far (Gene Ontology, pizza); filed as follow-up if a real fixture needs it (see "Deferred follow-up" below) |
| Class/object-property/data-property axioms, class expressions | No | #606, #607 |
| ABox axioms, non-declaration axiom annotations | No | #608 |
| CLI/notebook wiring | No | #609 |

---

## Intermediate design

- **XML parsing library: `roxmltree`, not `quick-xml`.** `quick-xml`'s core
  API is a streaming pull-parser (`Reader`/`Event`), which is a good fit for
  large flat documents but pushes the caller to hand-maintain an explicit
  stack for recursive structures. OWL/XML is the opposite shape: deeply
  nested class expressions are exactly the case #606 flags as
  bug-prone ("a union containing an intersection containing a restriction").
  `roxmltree` parses once into a read-only DOM (`roxmltree::Document`,
  `Node` with `.children()`, `.attribute()`, `.tag_name()`, `.text()`) that
  recursive-descent functions can walk directly — `fn class_expression(node:
  roxmltree::Node) -> Result<ClassExpression, String>` recursing into
  `node.children()` mirrors `class_expr.rs` in the other two parsers almost
  line for line, just replacing "parse next paren-form" with "look at next
  child element". `quick-xml` is not currently a workspace dependency
  (checked: not in `Cargo.lock` before this crate); neither is `roxmltree`
  before this crate, so both are equally new — but `roxmltree` reads
  radically simpler for the tree shape this format actually has.
- **No `ParserContext` state cell.** `manchester_parser`/
  `owl_functional_parser` thread a `ParserContext` for prefix resolution
  because their input is scanned left-to-right by `nom` and prefix
  declarations must be recorded before later text is parsed. Here the whole
  document is already a tree (`roxmltree::Document::parse` returns
  everything at once), so prefixes are collected into a plain
  `HashMap<String, String>` from the root `<Ontology>`'s `<Prefix>` children
  in one pass, then threaded as a `&HashMap` (or a small `Resolver { prefixes,
  base }` struct once relative-IRI resolution is added) through the
  recursive-descent functions — no interior mutability needed since nothing
  is written mid-parse. `next_anon_individual`/`blank_node_labels` (the
  other half of the other parsers' `ParserContext`) are out of scope for
  this issue (no individuals yet) and will be added in #608 when
  `NamedIndividual`/anonymous-individual handling is needed.
- **One function per element production**, same convention as
  `owl_functional_parser`'s "one function per keyword" — `entity(node)`,
  `declaration(node)`, `prefix_decl(node)`, `import_decl(node)`. A
  `Declaration` element's `Entity` child is looked up by tag name via `alt`-
  style match on `node.tag_name().name()` rather than `nom::alt`.
- **IRI resolution**: `resolve_iri(node, resolver) -> Result<FullIri,
  String>` reads whichever of `IRI`/`abbreviatedIRI` is present on `node`
  (exactly one is expected per the spec), resolves an `abbreviatedIRI`
  against the prefix map, and returns the `IRI` attribute's value as-is
  (full-IRI-only for this issue — relative-IRI-against-`xml:base`
  resolution, matching `sparql_parser`'s `oxiri`-based approach in
  `iri.rs`, is deferred; see "Deferred follow-up").
- **Module layout** (mirrors `owl_functional_parser`'s split, adapted to a
  DOM walk instead of a `nom` combinator chain):
  - `src/lib.rs` — public `parse(&str) -> Result<Ontology, String>` entry
    point: `roxmltree::Document::parse`, root-element validation, prefix
    collection, `Import`/ontology-`Annotation`/`Declaration` dispatch over
    `<Ontology>`'s children, `OntologyVersion` construction. Re-exports.
  - `src/iri.rs` — `resolve_iri`, prefix-map collection from `<Prefix>`
    elements.
  - `src/annotation.rs` — `<Annotation>` element → `owl_ontology::Annotation`
    (property + `AnnotationValue`); ontology-level and declaration-level
    annotation-list collection. Only the `IriAnnotation`/`LiteralAnnotation`
    value shapes needed by a `<Literal>`/entity-IRI annotation value are
    covered here — `IndividualAnnotation` values need `individual.rs`
    (#608).
  - `src/declaration.rs` — `Entity` element dispatch (`<Class>`,
    `<Datatype>`, `<ObjectProperty>`, `<DataProperty>`,
    `<AnnotationProperty>`, `<NamedIndividual>`) and `<Declaration>` →
    `Axiom::AxiomDeclaration`.
  - `tests/owl_xml.rs` — integration tests, one `.owx`-flavoured XML snippet
    per test, following `owl_functional_parser/tests/functional_syntax.rs`'s
    pattern (doc comments explaining what's asserted, `#[ignore] // #605`
    pending implementation).

Later sub-issues add `src/class_expr.rs`, `src/property_expr.rs`,
`src/data_range.rs`, `src/individual.rs`, `src/axiom.rs` (#606-608),
matching the other two parsers' layout — not created as stubs here, since
CLAUDE.md's TDD protocol stubs only what the *current* issue's tests need to
compile.

---

## Phases (this issue, #605)

Each phase is implemented fully (all its tests green, `cargo clippy -p
owl-xml-parser --all-targets -- -D warnings` clean) before moving to the
next, per CLAUDE.md's TDD protocol.

1. **Crate scaffold** — `owl_xml_parser` crate added to the workspace,
   `Cargo.toml`, this plan doc.
2. **Ontology header** — `<Ontology>` root with optional `ontologyIRI`/
   `versionIRI`, empty body. Establishes `lib.rs`'s document-level parsing
   and `OntologyVersion` construction.
3. **`<Prefix>` + `<Import>`** — prefix-map collection (`iri.rs`), import
   IRI collection.
4. **`<Declaration>`** — all six `Entity` variants
   (`declaration.rs`), both `IRI=` and `abbreviatedIRI=` forms, resolved
   through the phase-3 prefix map.
5. **Ontology-level `<Annotation>` + `<Declaration>`'s own leading
   `<Annotation>` children** — `annotation.rs`; both attach a
   `Vec<Annotation>` (ontology-level to `Ontology::annotations`,
   declaration-level to the `Declaration` tuple's first field).
6. **Full-document integration** — a multi-declaration ontology header (adapted
   from the pizza-style example above, header-only — no class/property axioms
   since those are #606/#607) parsed end-to-end and checked against the full
   `Ontology` shape (imports, version, annotations, declarations).

---

## TDD protocol (per CLAUDE.md)

1. This plan document is committed on its own first.
2. All tests for all 6 phases are written next, in
   `owl_xml_parser/tests/owl_xml.rs`, with just enough type stubs in `src/`
   for the test file to compile. No implementation logic yet. Tests are
   `#[ignore] // #605`. Committed as its own commit.
3. Implementation proceeds phase by phase. For each test: unignore it,
   implement just enough to pass, run `cargo clippy -p owl-xml-parser
   --all-targets -- -D warnings` and re-read the diff for smells, then move
   to the next test. All tests in a phase are green before starting the next
   phase.
4. End-of-task quality gate (full workspace, per root `CLAUDE.md`):
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

## Deferred follow-up

- `xml:base`-driven relative-IRI resolution (an entity's `IRI="#Pizza"`
  resolving against an explicit `xml:base` attribute rather than the
  ontology IRI) is not implemented in this issue; the ontology-IRI-as-base
  fallback in `sparql_parser`'s own `BASE`-handling precedent
  (`ParserContext::base`, [#217](https://github.com/daghovland/rdf-datalog/issues/217))
  is not ported here since no OWL/XML fixture checked so far (pizza-style
  examples, Gene Ontology's `go.owl` header) exercises `xml:base` — filed as
  a real follow-up issue, unlabeled/Status `Todo`, once a concrete fixture
  needing it is found, rather than speculatively building it now.
- Class expressions, property axioms, ABox axioms, non-declaration axiom
  annotations, and CLI/notebook wiring are #606-#609, already filed against
  epic #564 — not re-filed here.

---

## References

- [OWL 2 XML Serialization, W3C](https://www.w3.org/TR/owl2-xml-serialization/)
- [OWL 2 Structural Specification](https://www.w3.org/TR/2012/REC-owl2-syntax-20121211) — `owl_ontology`'s type model is based on this
- Epic [#564](https://github.com/daghovland/rdf-datalog/issues/564) — this feature area
- Issue [#605](https://github.com/daghovland/rdf-datalog/issues/605) — this issue
- Issues [#606](https://github.com/daghovland/rdf-datalog/issues/606)-[#609](https://github.com/daghovland/rdf-datalog/issues/609) — follow-up sub-issues
- `owl_functional_parser/`, `manchester_parser/` — the two sibling
  alternate-concrete-syntax parsers this plan follows structurally
- [`docs/plans/OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md`](OWL_FUNCTIONAL_SYNTAX_PARSER_PLAN.md),
  [`docs/plans/MANCHESTER_SYNTAX_PLAN.md`](MANCHESTER_SYNTAX_PLAN.md) — plan-document templates this document follows
