# OWL 2 Manchester Syntax Parser Plan

Tracked in issue: [#139](https://github.com/daghovland/rdf-datalog/issues/139) — "OWL Manchester Syntax parser".
Deferred-features follow-up: [#157](https://github.com/daghovland/rdf-datalog/issues/157).

This document plans a real implementation of `manchester_parser` (currently a
16-line stub that always returns `Err`). The target is a nom-based parser,
in the style of `sparql_parser` and `datalog_parser`, that reads
[OWL 2 Manchester Syntax](https://www.w3.org/TR/owl2-manchester-syntax/)
(`.omn` files) and produces an `owl_ontology::Ontology`.

---

## What is Manchester Syntax?

Manchester Syntax is a frame-based, human-readable syntax for OWL 2
ontologies. Instead of one axiom per line (functional syntax) or a triple
store (RDF/XML, Turtle), it groups all axioms about one entity into a
*frame*:

```omn
Prefix: : <http://example.org/pizza#>
Prefix: owl: <http://www.w3.org/2002/07/owl#>

Ontology: <http://example.org/pizza>

Class: Pizza
    SubClassOf: Food
    EquivalentTo: Food and (hasTopping some Topping)

ObjectProperty: hasTopping
    Domain: Pizza
    Range: Topping
    Characteristics: InverseFunctional
```

A single frame (`Class: Pizza ...`) expands to *multiple* OWL axioms: one
`SubClassOf` axiom and one `EquivalentClasses` axiom, both about `Pizza`.
This is the central design fact that shapes the parser: **a frame parser
returns `Vec<Axiom>`, not one `Axiom`.**

---

## Target data model (do not invent new types)

The parser must produce `owl_ontology::Ontology` (`owl_ontology/src/ontology.rs`)
built from `owl_ontology::axioms` types: `Axiom`, `ClassAxiom`,
`ObjectPropertyAxiom`, `DataPropertyAxiom`, `Assertion`, `AnnotationAxiom`,
`Entity`, `Declaration`, `ClassExpression`, `ObjectPropertyExpression`,
`DataRange`, `Individual`, `Annotation`, `AnnotationValue`. IRIs are
`owl_ontology::FullIri(ingress::IriReference(String))` — every Manchester IRI
form (`<full>`, `prefix:local`, bare `simpleName`, default `:name`) must be
resolved to a `FullIri` before it reaches the AST; there is no partial/lazy
IRI type in the target model.

`Ontology::new` takes `directly_imports_documents: Vec<IriReference>`,
`version: OntologyVersion`, `annotations: Vec<Annotation>`,
`axioms: Vec<Axiom>`. `Ontology` itself has **no prefix field** — prefixes are
consumed during parsing (to expand IRIs) and then discarded. (`OntologyDocument`
carries prefixes alongside an `Ontology`, but `manchester_parser::parse` returns
a bare `Ontology` per the existing stub signature, so we do not surface
`OntologyDocument` unless a concrete need arises.)

---

## Grammar productions in scope

Quoted (lightly reformatted) from the W3C spec, https://www.w3.org/TR/owl2-manchester-syntax/,
sections 2.1–2.5. These are the productions this implementation targets.

**Ontology structure (§2.2):**
```
prefixDeclaration ::= 'Prefix:' prefixName fullIRI
import           ::= 'Import:' IRI
annotations      ::= 'Annotations:' annotationAnnotatedList
ontology         ::= 'Ontology:' [ ontologyIRI [ versionIRI ] ]
                       { import } { annotations } { frame }
```

**IRIs, literals, entities (§2.1):**
```
IRI       ::= fullIRI | abbreviatedIRI | simpleIRI
classIRI  ::= IRI
individual ::= individualIRI | nodeID
literal   ::= typedLiteral | stringLiteralNoLanguage
            | stringLiteralWithLanguage | integerLiteral
            | decimalLiteral | floatingPointLiteral
typedLiteral ::= lexicalValue '^^' Datatype
```

**Class-expression precedence ladder (§2.4) — the hard core:**
```
description ::= conjunction 'or' conjunction { 'or' conjunction } | conjunction
conjunction ::= classIRI 'that' [ 'not' ] restriction { 'and' [ 'not' ] restriction }
              | primary 'and' primary { 'and' primary }
              | primary
primary     ::= [ 'not' ] ( restriction | atomic )
restriction ::= objectPropertyExpression 'some' primary
              | objectPropertyExpression 'only' primary
              | objectPropertyExpression 'value' individual
              | objectPropertyExpression 'Self'
              | objectPropertyExpression 'min' nonNegativeInteger [ primary ]
              | objectPropertyExpression 'max' nonNegativeInteger [ primary ]
              | objectPropertyExpression 'exactly' nonNegativeInteger [ primary ]
              | dataPropertyExpression 'some' dataPrimary
              | dataPropertyExpression 'only' dataPrimary
              | dataPropertyExpression 'value' literal
              | dataPropertyExpression 'min' nonNegativeInteger [ dataPrimary ]
              | dataPropertyExpression 'max' nonNegativeInteger [ dataPrimary ]
              | dataPropertyExpression 'exactly' nonNegativeInteger [ dataPrimary ]
atomic      ::= classIRI | '{' individualList '}' | '(' description ')'
```
Note: `conjunction`'s first alternative (`classIRI 'that' ...`) is the
"`X that P some Y`" sugar. It is low-value relative to its parsing
complexity (it re-enters `classIRI` ambiguously against `primary`) and is
**deferred** — see "Deferred" below. The `primary 'and' primary` and plain
`primary` alternatives are in scope.

**Property/data-range expressions (§2.3):**
```
objectPropertyExpression ::= objectPropertyIRI | inverseObjectProperty
dataPropertyExpression   ::= dataPropertyIRI
dataRange       ::= dataConjunction 'or' dataConjunction { 'or' dataConjunction } | dataConjunction
dataConjunction ::= dataPrimary 'and' dataPrimary { 'and' dataPrimary } | dataPrimary
dataPrimary     ::= [ 'not' ] dataAtomic
dataAtomic      ::= Datatype | '{' literalList '}' | datatypeRestriction | '(' dataRange ')'
```
Only the `Datatype` (bare named datatype) alternative of `dataAtomic` is
implemented; `dataConjunction`/`dataRange`'s `and`/`or`, `'{' literalList '}'`
(`DataOneOf`), and `datatypeRestriction` (facets) are **deferred** (#157) —
`DataRange` parsing always yields `DataRange::NamedDataRange`.

**Frames (§2.5):**
```
classFrame ::= 'Class:' classIRI
    { 'Annotations:' annotationAnnotatedList
    | 'SubClassOf:' descriptionAnnotatedList
    | 'EquivalentTo:' descriptionAnnotatedList
    | 'DisjointWith:' descriptionAnnotatedList
    | 'DisjointUnionOf:' annotations description2List }        -- deferred (#157)
    | 'HasKey:' annotations (...)                                -- deferred (#157)

objectPropertyFrame ::= 'ObjectProperty:' objectPropertyIRI
    { 'Annotations:' annotationAnnotatedList
    | 'Domain:' descriptionAnnotatedList
    | 'Range:' descriptionAnnotatedList
    | 'Characteristics:' objectPropertyCharacteristicAnnotatedList
    | 'SubPropertyOf:' objectPropertyExpressionAnnotatedList
    | 'EquivalentTo:' objectPropertyExpressionAnnotatedList
    | 'DisjointWith:' objectPropertyExpressionAnnotatedList
    | 'InverseOf:' objectPropertyExpressionAnnotatedList
    | 'SubPropertyChain:' annotations objectPropertyExpression 'o' ... }  -- deferred (#157)

dataPropertyFrame ::= 'DataProperty:' dataPropertyIRI
    { 'Annotations:' annotationAnnotatedList
    | 'Domain:' descriptionAnnotatedList
    | 'Range:' dataRangeAnnotatedList
    | 'Characteristics:' annotations 'Functional'
    | 'SubPropertyOf:' dataPropertyExpressionAnnotatedList
    | 'EquivalentTo:' dataPropertyExpressionAnnotatedList
    | 'DisjointWith:' dataPropertyExpressionAnnotatedList }

annotationPropertyFrame ::= 'AnnotationProperty:' annotationPropertyIRI
    { 'Annotations:' annotationAnnotatedList }
    | 'Domain:' IRIAnnotatedList | 'Range:' IRIAnnotatedList
    | 'SubPropertyOf:' annotationPropertyIRIAnnotatedList

individualFrame ::= 'Individual:' individual
    { 'Annotations:' annotationAnnotatedList
    | 'Types:' descriptionAnnotatedList
    | 'Facts:' factAnnotatedList
    | 'SameAs:' individualAnnotatedList
    | 'DifferentFrom:' individualAnnotatedList }

misc ::= 'EquivalentClasses:' annotations description2List
       | 'DisjointClasses:' annotations description2List
       | 'EquivalentProperties:' annotations objectProperty2List
       | 'DisjointProperties:' annotations objectProperty2List
       | 'EquivalentProperties:' annotations dataProperty2List
       | 'DisjointProperties:' annotations dataProperty2List
       | 'SameIndividual:' annotations individual2List
       | 'DifferentIndividuals:' annotations individual2List
```

---

## Scope (in / out) — summary table

| Feature | In scope | Notes |
|---|---|---|
| `Prefix:` (incl. default `:`), `Ontology:`, `Import:`, ontology `Annotations:` | Yes | |
| `Class:` frame: `Annotations:`, `SubClassOf:`, `EquivalentTo:`, `DisjointWith:` | Yes | |
| `Class:` frame: `DisjointUnionOf:`, `HasKey:` | No | #157 |
| `ObjectProperty:` frame: `Annotations:`, `Domain:`, `Range:`, `Characteristics:`, `SubPropertyOf:`, `EquivalentTo:`, `DisjointWith:`, `InverseOf:` | Yes | |
| `ObjectProperty:` frame: `SubPropertyChain:` | No | #157 |
| `DataProperty:` frame: all sections (Characteristics limited to `Functional`, per spec) | Yes | |
| `Individual:` frame: `Annotations:`, `Types:`, `Facts:` (incl. negative `not` facts), `SameAs:`, `DifferentFrom:` | Yes | anonymous individuals via `_:id` node IDs supported |
| `AnnotationProperty:` frame: `Annotations:`, `Domain:`, `Range:`, `SubPropertyOf:` | Yes | |
| Top-level `misc`: `EquivalentClasses:`, `DisjointClasses:`, `EquivalentProperties:`/`DisjointProperties:` (object + data), `SameIndividual:`, `DifferentIndividuals:` | Yes | |
| Class expressions: atomic class, `(desc)`, `{ind, ind}` (`ObjectOneOf`), `not`/`and`/`or`, restrictions (`some`/`only`/`value`/`Self`/`min`/`max`/`exactly`, qualified and unqualified) | Yes | |
| `conjunction`'s `classIRI 'that' ...` sugar | No | #157 |
| Data ranges beyond a bare named datatype (`and`/`or`/`not`/`{lit,...}`/facet restrictions) | No | #157 |
| `Datatype:` frame | No | #157 (depends on compound data ranges) |
| `Rule:` (SWRL) frames | No | #157 |
| Literals: typed, plain string, lang string, integer, decimal, float | Yes | |

---

## Intermediate design

- **`ParserContext`**: `{ prefixes: HashMap<String, IriReference>, base: Option<IriReference>, next_anon_individual: Cell<u32>, blank_node_labels: RefCell<HashMap<String, u32>> }`,
  modeled on `datalog_parser::ParserContext` / `sparql_parser::ParserContext`
  (prefix map keyed by prefix name including `""` for the default `:` prefix).
  IRI resolution (full/prefixed/simple) happens against this context and
  always yields `owl_ontology::FullIri`.
- **Frame parsers return `Vec<Axiom>`.** E.g. `class_frame` parses
  `Class: C { section }*` and, for each section, emits axioms with that
  section's own `Annotations:`-collected `Vec<Annotation>` — matching the
  `Vec<Annotation>` slot each `ClassAxiom`/`ObjectPropertyAxiom`/etc. variant
  carries. A frame also always emits one `Axiom::AxiomDeclaration` for its
  own entity.
- **Cardinality restrictions**: `min N P` → `ObjectMinCardinality(N, P)`;
  `min N P C` (filler present) → `ObjectMinQualifiedCardinality(N, P, C)`.
  Same pattern for `max`/`exactly`, and the data-property equivalents.
- **Module layout** (mirrors `sparql_parser`'s `ast.rs`/`lib.rs` split and
  `datalog_parser`'s single-file nom style, adapted to this grammar's size):
  - `src/lib.rs` — public `parse(&str) -> Result<Ontology, String>` entry
    point; top-level `ontologyDocument`/`ontology` parsing; re-exports.
  - `src/tokens.rs` — whitespace/comment skipping, case-sensitive keyword
    matching with word-boundary checks (so `and`/`or`/`not`/`some`/`only`
    don't match inside a longer identifier), delimited-list helpers.
  - `src/iri.rs` — `fullIRI`, prefixed name, simple name; `ParserContext`
    and IRI resolution.
  - `src/literal.rs` — literal parsing → `ingress::GraphElement`/`RdfLiteral`.
  - `src/individual.rs` — `individual` (named IRI or `_:nodeID` anonymous).
  - `src/class_expr.rs` — the `description`/`conjunction`/`primary`/
    `restriction`/`atomic` ladder → `ClassExpression`.
  - `src/property_expr.rs` — `objectPropertyExpression` (incl. `inverse`),
    `dataPropertyExpression`.
  - `src/data_range.rs` — named-datatype-only `dataRange` → `DataRange`.
  - `src/annotation.rs` — `Annotations:` section → `Vec<Annotation>`.
  - `src/frame.rs` — all entity frames + top-level `misc`, each → `Vec<Axiom>`.
  - `tests/manchester_syntax.rs` — integration tests, one `.omn` snippet per
    test, following `turtle/tests/rdf12.rs`'s pattern (doc comments
    explaining what's asserted, `#[ignore] // #139` pending implementation,
    `#[ignore] // #157` for deferred-feature placeholders).

---

## Phases

Each phase is implemented fully (all its tests green, `cargo clippy -p
manchester-parser --all-targets -- -D warnings` clean) before moving to the
next, per CLAUDE.md's TDD protocol.

1. **Ontology header** — `Prefix:` (incl. default `:`), `Ontology:` with
   optional IRI/version IRI, `Import:`, ontology-level `Annotations:`, empty
   ontology body. Establishes `ParserContext`, `tokens.rs`, `iri.rs`.
2. **Literals & individuals** — `literal.rs`, `individual.rs`. Exercised
   indirectly through minimal frames (e.g. a `Facts:` section) since these
   are internal combinators, not part of the public API.
3. **Class expressions** — `class_expr.rs`'s full ladder: atomic, `not`,
   `and`, `or`, parenthesized, `{a b c}` one-of, all restriction forms
   (qualified/unqualified cardinalities). Tested via minimal `Class: C
   SubClassOf: <expr>` frames so results can be asserted against
   `ClassAxiom::SubClassOf`.
4. **Class frames** — `Annotations:`, `SubClassOf:`, `EquivalentTo:`,
   `DisjointWith:`, each accepting an annotated list of expressions and
   emitting one axiom per list element (or one `EquivalentClasses`/
   `DisjointClasses` per whole list, matching the `Vec<ClassExpression>`
   variant shape).
5. **Object/data property frames** — `property_expr.rs`, `data_range.rs`
   (named datatype only), full `ObjectProperty:` and `DataProperty:` frame
   sections per the scope table.
6. **Individual frames** — `Types:`, `Facts:` (positive and `not`-negated),
   `SameAs:`, `DifferentFrom:`; anonymous individuals via `_:id`.
7. **Annotation properties + annotations-on-axioms** — `AnnotationProperty:`
   frame; verify `Annotations:` sections attach correctly to axioms from
   earlier phases (re-test a `Class:`/`ObjectProperty:` frame with an
   `Annotations:` section preceding a `SubClassOf:` entry).
8. **Top-level misc + full-document integration** — `EquivalentClasses:`,
   `DisjointClasses:`, `EquivalentProperties:`/`DisjointProperties:`,
   `SameIndividual:`/`DifferentIndividuals:`; then a larger end-to-end test
   assembling a multi-frame ontology (adapted from the spec's §3 "Quick
   Reference" example / pizza-style example above) and checking the full
   `Vec<Axiom>`.

---

## TDD protocol (per CLAUDE.md)

1. This plan document is committed on its own first.
2. All tests for all 8 phases (plus `#[ignore] // #157` placeholders for
   deferred grammar) are written next, in `manchester_parser/tests/
   manchester_syntax.rs`, with just enough type stubs in `src/` for the test
   file to compile. No implementation logic yet. Tests are `#[ignore]`.
   Committed as its own commit.
3. Implementation proceeds phase by phase. For each test: unignore it,
   implement just enough to pass, run `cargo clippy -p manchester-parser
   --all-targets -- -D warnings` and re-read the diff for smells, then move
   to the next test. All tests in a phase are green before starting the
   next phase.
4. End-of-task quality gate (full workspace, per root `CLAUDE.md`):
   ```bash
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```

---

## Serialiser (`manchester_parser::serialize`)

Tracked in issue [#160](https://github.com/daghovland/rdf-datalog/issues/160),
follow-up to this parser and to [#147](https://github.com/daghovland/rdf-datalog/issues/147)
(Turtle 1.2 serialisation, the pattern this mirrors). Lives in
`manchester_parser/src/serialize.rs`, exported as `manchester_parser::serialize`.

**Scope mirrors the parser's** (the table above): the same entity frames and
sections, the same class-expression/restriction forms, the same "named
datatype only" data ranges. Constructs deferred by [#157](https://github.com/daghovland/rdf-datalog/issues/157)
are out of scope here too.

Design points:

- **No `Prefix:` declarations are emitted.** `Ontology` carries no prefix map
  (see "Target data model" above — prefixes are consumed and discarded during
  parsing), so every IRI is serialized in full `<...>` form. The parser's
  `iri` production accepts a full IRI in every position a Manchester IRI can
  appear, so this is always valid and sidesteps inventing a prefix-shortening
  scheme.
- **One frame per entity, not per axiom.** Axioms are grouped by their frame
  subject (the class/property/individual a section is about) in
  first-occurrence order, then emitted as a single frame with one section
  line per axiom (`many0` in the parser's frame grammar allows a section
  keyword like `SubClassOf:` to repeat, so no comma-joining of same-keyword
  items is needed). Grouping — rather than one frame per axiom — matters
  specifically for declaration annotations: the parser folds every
  `Annotations:` section inside a frame into that entity's single
  `AxiomDeclaration`, so splitting an entity's declaration annotations and its
  other axioms across separate same-named frames would reparse into two
  distinct (and non-equal) declaration axioms.
- **Top-level `misc` forms** (`EquivalentClasses:`, `DisjointClasses:`,
  `SameIndividual:`, `DifferentIndividuals:`, `EquivalentProperties:`,
  `DisjointProperties:`) are used for genuinely n-ary axioms (more than two
  members), and as a fallback for binary axioms whose members can't serve as
  an atomic frame subject (e.g. one side of an `EquivalentObjectProperties`
  pair is `inverse P`, which has no `ObjectProperty:` frame header of its
  own — the other side is tried instead, since the relation is symmetric).
- **Out-of-scope or unsupported constructs are skipped with a `log::warn!`,
  never silently emitted as invalid syntax.** This covers everything
  deferred by #157, plus two serialisation-specific gaps: a standalone
  `AnnotationAssertion` about an arbitrary subject (the frame grammar only
  lets `Annotations:` attach to a frame's own entity declaration, so there's
  no frame form for an assertion about an unrelated subject), and any
  `ClassExpression`/`ObjectPropertyExpression` variant the parser itself
  never produces (`AnonymousClass`, `AnonymousObjectProperty`,
  `ObjectPropertyChain`, nested `inverse (inverse ...)`).
- **Anonymous individuals** (`Individual::AnonymousIndividual(u32)`) serialize
  as `_:b<id>`. Ids are assigned by first-occurrence order during parsing;
  since the serializer walks axioms in their original order when building
  frame groups, a document with a single anonymous individual round-trips to
  the same id. Multiple anonymous individuals are not guaranteed to keep
  their relative ids stable across a round-trip once entity-grouping can
  reorder which `_:bN` label is written first — the round-trip test suite
  (`manchester_parser/tests/serialize_roundtrip.rs`) accordingly limits itself
  to single-anonymous-individual fixtures.

Round-trip tests (parse → serialize → re-parse → compare axiom sets via
`HashSet<owl_ontology::Axiom>`, per this document's original TDD-protocol
ask) live in `manchester_parser/tests/serialize_roundtrip.rs`, covering the
ontology header, each entity frame's sections, class-expression nesting,
top-level `misc` forms, and a multi-frame integration fixture.

## References

- [OWL 2 Manchester Syntax, W3C](https://www.w3.org/TR/owl2-manchester-syntax/)
- [OWL 2 Structural Specification](https://www.w3.org/TR/2012/REC-owl2-syntax-20121211) — `owl_ontology`'s type model is based on this
- Issue [#139](https://github.com/daghovland/rdf-datalog/issues/139) — this feature
- Issue [#157](https://github.com/daghovland/rdf-datalog/issues/157) — deferred grammar follow-up
- `sparql_parser/` and `datalog_parser/` — nom parser conventions this crate follows
- `turtle/tests/rdf12.rs` — test-file structure/pattern this crate follows
- `docs/plans/RDF12_PLAN.md` — plan-document template this document follows
