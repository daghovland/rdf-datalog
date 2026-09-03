# OWL 2 Functional-Style Syntax Parser Plan

Tracked in issue: [#180](https://github.com/daghovland/rdf-datalog/issues/180) — "OWL 2
Functional-Style Syntax: parser (.ofn -> owl_ontology::Ontology)". Part of the
Functional-Style Syntax epic (pairing serializer issue #181, not in scope
here). Mirrors [`docs/plans/MANCHESTER_SYNTAX_PLAN.md`](MANCHESTER_SYNTAX_PLAN.md)'s
structure and level of detail, which this crate's own module layout also
mirrors.

This document plans `owl_functional_parser`, a new nom-based crate reading
[OWL 2 Functional-Style Syntax](https://www.w3.org/TR/owl2-syntax/#Functional-Style_Syntax)
(`.ofn` files) — the canonical S-expression-like concrete syntax used
throughout the OWL 2 spec itself (`SubClassOf( :Dog :Animal )`) — and
producing an `owl_ontology::Ontology`, the same target type
`manchester_parser` already produces so all downstream consumers
(`owl2datalog`, reasoning) work unchanged regardless of concrete syntax.

---

## What is Functional-Style Syntax?

Unlike Manchester Syntax's frame-based grouping (all axioms about one entity
under one `Class: Foo` header) or Turtle's triple-oriented syntax,
Functional-Style Syntax is **axiom-per-axiom, fully parenthesized, prefix
notation**: every axiom, class expression, and data range is a keyword
followed by its arguments in parentheses, nested arbitrarily deeply. There is
no operator precedence to resolve (no `and`/`or`/`not` infix ladder as in
Manchester) — every construct names its own keyword, so the grammar is a
straightforward recursive-descent match on the leading keyword token.

```ofn
Prefix(:=<http://example.org/pizza#>)
Prefix(owl:=<http://www.w3.org/2002/07/owl#>)

Ontology(<http://example.org/pizza>
    Declaration(Class(:Pizza))
    Declaration(Class(:Food))
    SubClassOf(:Pizza :Food)
    EquivalentClasses(:Pizza ObjectIntersectionOf(:Food ObjectSomeValuesFrom(:hasTopping :Topping)))
    ObjectPropertyDomain(:hasTopping :Pizza)
    ObjectPropertyRange(:hasTopping :Topping)
    InverseFunctionalObjectProperty(:hasTopping)
)
```

Because every production is unambiguously keyword-tagged, a functional-syntax
parser is structurally simpler than Manchester's precedence-climbing
`description`/`conjunction`/`primary` ladder — the main engineering cost here
is breadth (many keyword productions), not grammar ambiguity. This lets a v1
landing cover a **larger fraction of the OWL 2 type model** than Manchester's
initial landing (#139) did, since `owl_ontology`'s types (`ObjectPropertyChain`,
`HasKey`, `DataOneOf`/`DataComplementOf`/`DatatypeRestriction`, ...) already
exist and functional syntax makes parsing them mechanical rather than a fresh
grammar-design problem.

---

## Target data model (do not invent new types)

Same target as `manchester_parser`: `owl_ontology::Ontology`
(`owl_ontology/src/ontology.rs`) built from `owl_ontology::axioms` types —
`Axiom`, `ClassAxiom`, `ObjectPropertyAxiom`, `DataPropertyAxiom`, `Assertion`,
`AnnotationAxiom`, `Entity`, `Declaration`, `ClassExpression`,
`ObjectPropertyExpression`, `DataRange`, `Individual`, `Annotation`,
`AnnotationValue`, `SubPropertyExpression`. IRIs are
`owl_ontology::FullIri(ingress::IriReference(String))`; both functional-syntax
IRI forms (`<full>`, `prefix:local`) resolve to `FullIri` before reaching the
AST — there is no bare unprefixed `simpleIRI` production in Functional-Style
Syntax (unlike Manchester), so IRI resolution is simpler here.

`Ontology::new` takes `directly_imports_documents: Vec<IriReference>`,
`version: OntologyVersion`, `annotations: Vec<Annotation>`,
`axioms: Vec<Axiom>`. As with Manchester, prefixes are consumed during
parsing to expand IRIs and then discarded; `manchester_parser::parse`'s
precedent (returning a bare `Ontology`, not `OntologyDocument`) is followed
here too.

---

## Grammar productions in scope

Quoted/paraphrased from [the W3C spec](https://www.w3.org/TR/owl2-syntax/#Functional-Style_Syntax),
§3 (IRIs, literals), §5 (Declarations), §8 (Annotations), §9 (Axioms),
§Functional-Style Syntax mapping tables.

**Ontology document (§3.6, §3.2):**
```
prefixDeclaration    ::= 'Prefix' '(' prefixName '=' fullIRI ')'
ontologyDocument      ::= { prefixDeclaration } Ontology
Ontology              ::= 'Ontology' '(' [ ontologyIRI [ versionIRI ] ]
                            { directlyImportsDocuments }
                            { ontologyAnnotations }
                            { axiom } ')'
directlyImportsDocuments ::= 'Import' '(' IRI ')'
```
`ontologyIRI`/`versionIRI` are bare `fullIRI`s (not further wrapped).

**IRIs, literals (§3.5–§3.6, §5.1):**
```
IRI          ::= fullIRI | abbreviatedIRI
fullIRI      ::= '<' IRI-reference '>'
abbreviatedIRI ::= PNAME_LN   (prefix:local, incl. default ':' prefix)
Literal      ::= typedLiteral | stringLiteralNoLanguage | stringLiteralWithLanguage
typedLiteral ::= lexicalForm '^^' Datatype
lexicalForm  ::= quotedString
```

**Entities and declarations (§5.1, §5.8):**
```
Class              ::= IRI
Datatype           ::= IRI
ObjectProperty     ::= IRI
DataProperty       ::= IRI
AnnotationProperty ::= IRI
NamedIndividual    ::= IRI
Individual         ::= NamedIndividual | AnonymousIndividual
AnonymousIndividual::= nodeID                 (`_:label`)

Declaration ::= 'Declaration' '(' axiomAnnotations Entity ')'
Entity ::= 'Class' '(' Class ')' | 'Datatype' '(' Datatype ')'
         | 'ObjectProperty' '(' ObjectProperty ')'
         | 'DataProperty' '(' DataProperty ')'
         | 'AnnotationProperty' '(' AnnotationProperty ')'
         | 'NamedIndividual' '(' NamedIndividual ')'
```

**Property expressions (§6):**
```
ObjectPropertyExpression ::= ObjectProperty | InverseObjectProperty
InverseObjectProperty    ::= 'ObjectInverseOf' '(' ObjectProperty ')'
DataPropertyExpression   ::= DataProperty
```

**Data ranges (§7):**
```
DataRange ::= Datatype | DataIntersectionOf | DataUnionOf | DataComplementOf
            | DataOneOf | DatatypeRestriction
DataIntersectionOf  ::= 'DataIntersectionOf' '(' DataRange DataRange {DataRange} ')'
DataUnionOf         ::= 'DataUnionOf' '(' DataRange DataRange {DataRange} ')'
DataComplementOf    ::= 'DataComplementOf' '(' DataRange ')'
DataOneOf           ::= 'DataOneOf' '(' Literal {Literal} ')'
DatatypeRestriction ::= 'DatatypeRestriction' '(' Datatype constrainingFacet
                          restrictionValue {constrainingFacet restrictionValue} ')'
constrainingFacet ::= IRI
restrictionValue  ::= Literal
```

**Class expressions (§8):**
```
ClassExpression ::= Class
    | ObjectIntersectionOf | ObjectUnionOf | ObjectComplementOf | ObjectOneOf
    | ObjectSomeValuesFrom | ObjectAllValuesFrom | ObjectHasValue | ObjectHasSelf
    | ObjectMinCardinality | ObjectMaxCardinality | ObjectExactCardinality
    | DataSomeValuesFrom | DataAllValuesFrom | DataHasValue
    | DataMinCardinality | DataMaxCardinality | DataExactCardinality
```
Each is `'Keyword' '(' ... ')'` per the spec's mapping table; cardinality
forms take `nonNegativeInteger PropertyExpression [ Filler ]` — filler
present selects the Qualified variant, absent selects the unqualified one
(same convention `manchester_parser` uses for `min`/`max`/`exactly`).
`DataSomeValuesFrom`/`DataAllValuesFrom` take one-or-more `DataPropertyExpression`s
before the trailing `DataRange` (`owl_ontology::ClassExpression::DataSomeValuesFrom`
already models this as `Vec<DataProperty>`).

**Class axioms (§9.1):**
```
SubClassOf          ::= 'SubClassOf' '(' axiomAnnotations subClass superClass ')'
EquivalentClasses    ::= 'EquivalentClasses' '(' axiomAnnotations ClassExpression ClassExpression {ClassExpression} ')'
DisjointClasses      ::= 'DisjointClasses' '(' axiomAnnotations ClassExpression ClassExpression {ClassExpression} ')'
DisjointUnion        ::= 'DisjointUnion' '(' axiomAnnotations Class ClassExpression ClassExpression {ClassExpression} ')'
```

**Object property axioms (§9.2):**
```
SubObjectPropertyOf  ::= 'SubObjectPropertyOf' '(' axiomAnnotations subObjectPropertyExpression ObjectPropertyExpression ')'
subObjectPropertyExpression ::= ObjectPropertyExpression | propertyExpressionChain
propertyExpressionChain     ::= 'ObjectPropertyChain' '(' ObjectPropertyExpression ObjectPropertyExpression {ObjectPropertyExpression} ')'
EquivalentObjectProperties  ::= 'EquivalentObjectProperties' '(' axiomAnnotations ObjectPropertyExpression ObjectPropertyExpression {..} ')'
DisjointObjectProperties    ::= 'DisjointObjectProperties' '(' axiomAnnotations ObjectPropertyExpression ObjectPropertyExpression {..} ')'
ObjectPropertyDomain ::= 'ObjectPropertyDomain' '(' axiomAnnotations ObjectPropertyExpression ClassExpression ')'
ObjectPropertyRange  ::= 'ObjectPropertyRange' '(' axiomAnnotations ObjectPropertyExpression ClassExpression ')'
InverseObjectProperties ::= 'InverseObjectProperties' '(' axiomAnnotations ObjectPropertyExpression ObjectPropertyExpression ')'
FunctionalObjectProperty | InverseFunctionalObjectProperty
  | ReflexiveObjectProperty | IrreflexiveObjectProperty
  | SymmetricObjectProperty | AsymmetricObjectProperty | TransitiveObjectProperty
    ::= '<Keyword>' '(' axiomAnnotations ObjectPropertyExpression ')'
```

**Data property axioms (§9.3):**
```
SubDataPropertyOf        ::= 'SubDataPropertyOf' '(' axiomAnnotations DataPropertyExpression DataPropertyExpression ')'
EquivalentDataProperties ::= 'EquivalentDataProperties' '(' axiomAnnotations DataPropertyExpression DataPropertyExpression {..} ')'
DisjointDataProperties   ::= 'DisjointDataProperties' '(' axiomAnnotations DataPropertyExpression DataPropertyExpression {..} ')'
DataPropertyDomain       ::= 'DataPropertyDomain' '(' axiomAnnotations DataPropertyExpression ClassExpression ')'
DataPropertyRange        ::= 'DataPropertyRange' '(' axiomAnnotations DataPropertyExpression DataRange ')'
FunctionalDataProperty   ::= 'FunctionalDataProperty' '(' axiomAnnotations DataPropertyExpression ')'
```

**Datatype definitions and keys (§9.4, §9.5):**
```
DatatypeDefinition ::= 'DatatypeDefinition' '(' axiomAnnotations Datatype DataRange ')'
HasKey ::= 'HasKey' '(' axiomAnnotations ClassExpression
             '(' {ObjectPropertyExpression} ')' '(' {DataPropertyExpression} ')' ')'
```

**Assertions (§9.6):**
```
SameIndividual        ::= 'SameIndividual' '(' axiomAnnotations Individual Individual {Individual} ')'
DifferentIndividuals  ::= 'DifferentIndividuals' '(' axiomAnnotations Individual Individual {Individual} ')'
ClassAssertion        ::= 'ClassAssertion' '(' axiomAnnotations ClassExpression Individual ')'
ObjectPropertyAssertion ::= 'ObjectPropertyAssertion' '(' axiomAnnotations ObjectPropertyExpression Individual Individual ')'
NegativeObjectPropertyAssertion ::= 'NegativeObjectPropertyAssertion' '(' axiomAnnotations ObjectPropertyExpression Individual Individual ')'
DataPropertyAssertion ::= 'DataPropertyAssertion' '(' axiomAnnotations DataPropertyExpression Individual Literal ')'
NegativeDataPropertyAssertion ::= 'NegativeDataPropertyAssertion' '(' axiomAnnotations DataPropertyExpression Individual Literal ')'
```

**Annotations (§10):**
```
axiomAnnotations      ::= { Annotation }
Annotation            ::= 'Annotation' '(' annotationAnnotations AnnotationProperty AnnotationValue ')'
annotationAnnotations ::= { Annotation }
AnnotationValue        ::= AnonymousIndividual | IRI | Literal
AnnotationSubject       ::= IRI | AnonymousIndividual
AnnotationAssertion     ::= 'AnnotationAssertion' '(' axiomAnnotations AnnotationProperty AnnotationSubject AnnotationValue ')'
SubAnnotationPropertyOf ::= 'SubAnnotationPropertyOf' '(' axiomAnnotations AnnotationProperty AnnotationProperty ')'
AnnotationPropertyDomain ::= 'AnnotationPropertyDomain' '(' axiomAnnotations AnnotationProperty IRI ')'
AnnotationPropertyRange  ::= 'AnnotationPropertyRange' '(' axiomAnnotations AnnotationProperty IRI ')'
```
Note: `owl_ontology::AnnotationAxiom::AnnotationAssertion`'s subject/value
slots are typed `GraphElement`, not a dedicated subject/value enum — a
resolved IRI or anonymous-individual label is lowered to the matching
`GraphElement` variant (resource) and a literal to `GraphElement::Literal`,
mirroring how `manchester_parser` already bridges `Individual`/IRI values
into `GraphElement` elsewhere.

---

## Scope (in / out) — summary table

| Feature | In scope | Notes |
|---|---|---|
| `Prefix(...)`, `Ontology(...)` header incl. IRI/version IRI, `Import(...)` | Yes | |
| `Declaration(...)` for all six `Entity` variants | Yes | |
| Class expressions: all keyword forms listed above (object + data, incl. qualified/unqualified cardinalities, `ObjectOneOf`, `ObjectHasSelf`) | Yes | |
| Data ranges: `Datatype`, `DataIntersectionOf`, `DataUnionOf`, `DataComplementOf`, `DataOneOf`, `DatatypeRestriction` | Yes | full coverage — functional syntax makes these mechanical, unlike Manchester's deferral (#157); landed in phase 11, after the #180-mandated tier (phases 1–10) is complete — see "Phases" below |
| `ObjectInverseOf` | Yes | |
| `SubObjectPropertyOf` incl. `ObjectPropertyChain` LHS | Yes | |
| All class/object-property/data-property axiom keywords listed above | Yes | |
| `DatatypeDefinition`, `HasKey` | Yes | types already exist in `owl_ontology::Axiom` |
| All `Assertion` keywords (incl. negative property assertions) | Yes | |
| `AnnotationAssertion`, `SubAnnotationPropertyOf`, `AnnotationPropertyDomain`/`Range`, nested `Annotation(...)` on axioms/annotations | Yes | |
| Anonymous individuals (`_:label` nodeID) | Yes | mirrors `manchester_parser::iri::node_id` |
| SWRL `DLSafeRule` / `Rule(...)` (§11) | No | no `owl_ontology` type exists for SWRL rules at all (unlike Manchester's deferred productions, which target existing types) — filed as a follow-up issue, same rationale as Manchester's #157 split |
| `SubClassOf`/expression punning edge cases (e.g. using an undeclared entity) | No | parser does not validate global consistency (declarations vs. use), matching Manchester's precedent of trusting input structurally |

Everything in the "in scope" column above is targeted for this first
landing — a materially larger fraction of the OWL 2 model than Manchester's
initial landing, because Functional-Style Syntax's uniform
`Keyword(args...)` shape removes the grammar-ambiguity cost that justified
deferring these same constructs in Manchester.

---

## Intermediate design

- **`ParserContext`**: `{ prefixes: RefCell<HashMap<String, String>>, next_anon_individual: Cell<u32>, blank_node_labels: RefCell<HashMap<String, u32>> }` — same shape as `manchester_parser::iri::ParserContext`, minus the `data_property_iris` pre-scan set (functional syntax's `Keyword(...)` tagging means object- vs. data-property restrictions are never ambiguous — the keyword itself, e.g. `DataSomeValuesFrom` vs. `ObjectSomeValuesFrom`, already disambiguates, so no pre-scan pass is needed).
- **One function per keyword production.** Unlike Manchester's frame parsers returning `Vec<Axiom>`, each functional-syntax axiom keyword maps to exactly one `Axiom` value (or occasionally, for `n`-ary lists >2, to an axiom whose payload is a `Vec<...>` — still one `Axiom`, not several). A `Declaration(...)` likewise yields exactly one `Axiom::AxiomDeclaration`.
- **Generic parenthesized-keyword combinator**: `keyword_form(name, inner_parser)` matches `'Name' '(' inner ')'` with whitespace/comment skipping, used throughout instead of hand-rolling delimiters per production (mirrors `tokens::keyword`/`tokens::punct` from `manchester_parser` but adds the paren-wrapping convention specific to this syntax).
- **`axiomAnnotations`/`annotationAnnotations`**: both are `many0(Annotation)` immediately inside an axiom's/annotation's opening paren, before its "real" arguments — a single `axiom_annotations` combinator (`many0` of the `Annotation(...)` production) is shared by every axiom parser.
- **Cardinality restrictions**: `'ObjectMinCardinality' '(' n P [C] ')'` → filler present selects `ObjectMinQualifiedCardinality(n, P, C)`, absent selects `ObjectMinCardinality(n, P)` — same pattern `manchester_parser::class_expr` uses.
- **Module layout** (mirrors `manchester_parser`'s split):
  - `src/lib.rs` — public `parse(&str) -> Result<Ontology, String>` entry point; `Prefix`/`Ontology` header parsing; re-exports.
  - `src/tokens.rs` — whitespace/comment skipping, keyword + paren-form combinators.
  - `src/iri.rs` — `fullIRI`, `abbreviatedIRI`, `ParserContext`, IRI resolution, `nodeID`.
  - `src/literal.rs` — literal parsing → `ingress::GraphElement`/`RdfLiteral`.
  - `src/individual.rs` — `Individual` (named IRI or `_:nodeID` anonymous).
  - `src/annotation.rs` — `Annotation(...)`, `axiomAnnotations`/`annotationAnnotations`.
  - `src/property_expr.rs` — `ObjectPropertyExpression` (incl. `ObjectInverseOf`), `DataPropertyExpression`, `ObjectPropertyChain`.
  - `src/data_range.rs` — full `DataRange` grammar.
  - `src/class_expr.rs` — full `ClassExpression` grammar (all keyword forms, no precedence climbing needed).
  - `src/axiom.rs` — every axiom-keyword parser (`Declaration`, class/object-property/data-property axioms, `DatatypeDefinition`, `HasKey`, assertions, annotation axioms) → `Axiom`; top-level `axiom` dispatcher tries each in turn by leading keyword.
  - `tests/functional_syntax.rs` — integration tests, one `.ofn` snippet per test, following `manchester_parser/tests/manchester_syntax.rs`'s pattern (doc comments explaining what's asserted, `#[ignore] // #180` pending implementation).

---

## Phases

Each phase is implemented fully (all its tests green, `cargo clippy -p
owl-functional-parser --all-targets -- -D warnings` clean) before moving to
the next, per CLAUDE.md's TDD protocol. Phases 1–10 deliver exactly the
issue #180 tier (declarations, class/property/datatype axioms, ABox
assertions — the same coverage tier as Manchester's initial landing #139);
phase 11 is the extra breadth this syntax's unambiguous grammar makes cheap
(compound data ranges, property chains, `HasKey`). This ordering is
deliberate: **if work is interrupted after any phase through 10, the branch
already satisfies #180's stated scope** — the phase-3 data-range work is
trimmed to the named-datatype case only (sufficient for
`DataSomeValuesFrom`/`DataPropertyRange`/`DatatypeDefinition` to parse) so
nothing "extra" lands before the mandated tier is complete.

1. **Ontology header** — `Prefix(...)` (incl. default `:` prefix), `Ontology(...)` with optional IRI/version IRI, `Import(...)`, empty ontology body. Establishes `ParserContext`, `tokens.rs`, `iri.rs`.
2. **Literals, individuals, declarations** — `literal.rs`, `individual.rs`, `Declaration(...)` for all six `Entity` variants (tested via minimal `Ontology(... Declaration(Class(:C)) ...)` documents).
3. **Property expressions & named-datatype data ranges** — `property_expr.rs` (`ObjectInverseOf`); `data_range.rs` limited to `DataRange ::= Datatype` (bare named datatype only — `DataIntersectionOf`/`DataUnionOf`/`DataComplementOf`/`DataOneOf`/`DatatypeRestriction` moved to phase 11).
4. **Class expressions** — `class_expr.rs`'s full keyword set: boolean combinators, `ObjectOneOf`, all restriction forms (qualified/unqualified cardinalities, `ObjectHasSelf`, data restrictions over named-datatype ranges). Tested via minimal `SubClassOf(:C <expr>)` axioms so results assert against `ClassAxiom::SubClassOf`.
5. **Class axioms** — `SubClassOf`, `EquivalentClasses`, `DisjointClasses`, `DisjointUnion`.
6. **Object property axioms** — all keywords in §9.2 except `ObjectPropertyChain` as a `SubObjectPropertyOf` LHS (moved to phase 11); `SubObjectPropertyOf` in this phase only accepts a single `ObjectPropertyExpression` LHS. `ObjectPropertyDomain`/`ObjectPropertyRange` drop any `axiomAnnotations` with `log::warn!` — see "Type-model gaps" below; not a bug, a scoped limitation of the current `owl_ontology::ObjectPropertyAxiom` shape.
7. **Data property axioms, `DatatypeDefinition`** — §9.3–§9.4. `HasKey` (§9.5) moved to phase 11.
8. **Assertions** — all seven ABox assertion keywords (§9.6), incl. anonymous individuals.
9. **Annotation axioms + nested annotations** — `AnnotationAssertion`, `SubAnnotationPropertyOf`, `AnnotationPropertyDomain`/`Range`; verify `axiomAnnotations` attach correctly to axioms from earlier phases (re-test an axiom with a preceding `Annotation(...)`).
10. **Full-document integration** — a larger end-to-end test assembling a multi-axiom ontology (adapted from the pizza-style example above) and checking the full `Vec<Axiom>`, using only the phase 1–9 construct set.
11. **Extended constructs** (append-only, beyond #180's mandated tier — the "materially larger fraction of the OWL 2 model" this plan's intro describes): full `DataRange` grammar (`DataIntersectionOf`/`DataUnionOf`/`DataComplementOf`/`DataOneOf`/`DatatypeRestriction`), `ObjectPropertyChain` as a `SubObjectPropertyOf` LHS, `HasKey`.

## Type-model gaps found while parsing (not new `owl_ontology` types)

- **`ObjectPropertyAxiom::ObjectPropertyDomain`/`ObjectPropertyRange` have no
  `Vec<Annotation>` slot** (`owl_ontology/src/axioms.rs` lines 169–172),
  unlike every other `ObjectPropertyAxiom` variant. The functional-syntax
  grammar technically allows `axiomAnnotations` on both. Rather than widen
  the shared `owl_ontology` enum for this parser alone, any
  `axiomAnnotations` present on these two keywords are dropped with a
  `log::warn!` (matching this crate family's existing convention — see
  `manchester_parser`'s serializer — for constructs the target type model
  can't fully represent). Widening the enum, if wanted later, is an
  `owl_ontology`-crate change and out of scope for a parser-only PR.
- **`AnnotationAxiom::AnnotationAssertion`'s subject/value are
  `GraphElement`, not `Individual`/`Iri`.** An `_:x` anonymous-individual
  subject is lowered the same way `manchester_parser` already bridges
  anonymous individuals into `GraphElement` (see `manchester_parser/src/annotation.rs`,
  `literal.rs`) — reusing that scheme rather than inventing a second one, so
  the two parsers produce value-equal `Ontology`s for logically equivalent
  documents.
- **`Ontology` derives neither `PartialEq` nor `Debug`.** Tests assert on
  `.axioms`, `.version`, `.annotations`, `.directly_imports_documents`
  individually (as `owl_ontology/src/ontology.rs`'s own unit tests already
  do), not via a whole-struct `assert_eq!`.

---

## TDD protocol (per CLAUDE.md)

1. This plan document is committed on its own first.
2. All tests for all 10 phases are written next, in
   `owl_functional_parser/tests/functional_syntax.rs`, with just enough type
   stubs in `src/` for the test file to compile. No implementation logic yet.
   Tests are `#[ignore] // #180`. Committed as its own commit.
3. Implementation proceeds phase by phase. For each test: unignore it,
   implement just enough to pass, run `cargo clippy -p owl-functional-parser
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

SWRL `Rule(...)` / `DLSafeRule(...)` parsing (§11 of the spec) is deferred:
`owl_ontology` has no representation for SWRL rules at all (unlike
Manchester's #157 deferrals, which all target axiom/expression types that
already exist), so supporting it would require designing new
`owl_ontology` types first — out of scope for a parser-only issue. Filed as
a follow-up issue against this epic once this PR is under way.

## Serialiser (`owl_functional_parser::serialize`)

Tracked in issue [#181](https://github.com/daghovland/rdf-datalog/issues/181),
follow-up to this parser (#180 / PR #627). Lives in
`owl_functional_parser/src/serialize.rs`, exported as
`owl_functional_parser::serialize`. Mirrors `manchester_parser::serialize`'s
overall shape (see [`MANCHESTER_SYNTAX_PLAN.md`](MANCHESTER_SYNTAX_PLAN.md)'s
own "Serialiser" section) in two specific ways — no `Prefix:`/prefix
shortening (every IRI is emitted in full `<...>` form, since `Ontology`
carries no prefix map) and the `log::warn!`-and-skip policy for anything the
target syntax or `owl_ontology`'s type model can't represent — but the actual
structure is **simpler** than Manchester's, for the same reason the parser
itself was simpler to write in full: Functional-Style Syntax is
axiom-per-axiom, not frame-per-entity, so there is no grouping pass. Each
`owl_ontology::Axiom` maps to exactly one `Keyword(...)` line; the serializer
walks `ontology.axioms` in order and emits one line per axiom, with no
frame-subject bookkeeping, no "which entity does this belong to" grouping,
and no declaration-annotation-folding concern (Manchester's trickiest
serialization issue, from folding a frame's `Annotations:` section into a
`Declaration`'s own annotations — doesn't exist here, since
`Declaration(...)` is already its own one-line axiom).

Because the parser (#180) covers a materially larger fraction of the OWL 2
grammar than Manchester's initial landing (see this plan's own scope table
above — full `ClassExpression`/`DataRange`/`ObjectPropertyExpression`
grammars, `ObjectPropertyChain`, `HasKey`), the serializer's coverage is
correspondingly wider: every construct the parser accepts, the serializer can
emit, with two narrow exceptions (both true of Manchester's serializer too,
for the analogous reason — the parser never *produces* these variants, so
there is nothing to round-trip):

- `ClassExpression::AnonymousClass` and
  `ObjectPropertyExpression::AnonymousObjectProperty` — not produced by
  either syntax's parser (there is no *concrete-syntax* production for an
  anonymous class/object-property expression in OWL 2 at all; only
  individuals can be anonymous). Skipped with `log::warn!` for a
  hand-constructed `Ontology` that happens to use one.
- `ObjectPropertyDomain`/`ObjectPropertyRange` carry no `Vec<Annotation>`
  slot (see this plan's "Type-model gaps" section above), so there is
  nothing to lose on serialization — the axiom simply has no annotations to
  emit, which is not a skip, just an empty `axiomAnnotations` position.

`AnnotationAssertion`'s `GraphElement` subject/value (the same type-model gap
noted above) are formatted back into whichever Functional-Style Syntax
production matches the concrete `GraphElement` variant: `NodeOrEdge(Iri(_))`
→ a bare IRI, `NodeOrEdge(AnonymousBlankNode(_))` → `_:b<id>`,
`GraphLiteral(_)` → a `Literal`. Any other `GraphElement` variant (there
should be none reachable through this parser's own `annotation_subject`/
`annotation_value_as_graph_element` productions) is skipped with
`log::warn!` rather than panicking, so a hand-built `Ontology` with an
unexpected shape still serializes the rest of the document.

Anonymous individuals (`Individual::AnonymousIndividual(u32)`) serialize as
`_:b<id>`. Since there is no entity-grouping pass here (unlike Manchester),
axiom order — and therefore anonymous-individual first-occurrence order — is
preserved exactly, so a round-trip always reproduces the same ids
(Manchester's own limitation of testing only single-anonymous-individual
fixtures does not apply here).

Round-trip tests (parse → serialize → re-parse → compare axiom sets via
`HashSet<owl_ontology::Axiom>`, same pattern as
`manchester_parser/tests/serialize_roundtrip.rs`) live in
`owl_functional_parser/tests/serialize_roundtrip.rs`.

## References

- [OWL 2 Structural Specification and Functional-Style Syntax, W3C](https://www.w3.org/TR/owl2-syntax/)
- Issue [#180](https://github.com/daghovland/rdf-datalog/issues/180) — this feature
- Issue [#181](https://github.com/daghovland/rdf-datalog/issues/181) — the pairing serializer, see "Serialiser" section above
- `manchester_parser/` — sibling concrete-syntax parser this crate mirrors; see [`MANCHESTER_SYNTAX_PLAN.md`](MANCHESTER_SYNTAX_PLAN.md)
- `sparql_parser/` and `datalog_parser/` — nom parser conventions this crate follows
