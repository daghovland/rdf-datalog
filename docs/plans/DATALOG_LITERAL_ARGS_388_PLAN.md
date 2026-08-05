# Plan: RDF literal arguments in Datalog rule-atom terms (#388)

Issue: [#388](https://github.com/daghovland/rdf-datalog/issues/388)
Branch: `feat/388-datalog-literal-args`

## Problem

The `datalog_parser` rule-atom grammar (`predicate[subject, object]` and
`[subject, predicate, object]`) only accepts variables (`?x`), the `a` /
`rdf:type` sugar, and IRIs as terms. A quoted-string RDF literal in argument
position — e.g. matching migrated RDFox `.datalog` rules like:

```datalog
data:connectedEquipment [?node] :-
    rdfs:label [?internal, "P4712"],
    imf:connectedTo [?internal, ?node],
    dexpi:PipingOrEquipment [?node].
```

— fails to parse. The current workaround (bind to a variable, then
`FILTER(?label = "P4712")`) works but is more verbose than RDFox-dialect
users expect.

## Root cause (confirmed by reading the code, not re-derived here)

`datalog_parser/src/lib.rs`:
- `ParsedTerm` (line ~70) has only two variants: `Variable(String)`, `Iri(String)`.
- `parse_term` (line ~370) only tries `parse_variable_term`, `parse_rdf_type_abbr`,
  `parse_numeric_id_term`, then falls through to `parse_iri` — never attempts a
  quoted-string/typed/lang-tagged literal, so `"P4712"` in term position is a
  hard parse error.

## Investigation findings

1. **Reuse `sparql_parser`'s literal grammar rather than duplicating it.**
   `datalog_parser/Cargo.toml` already depends on `sparql-parser` (used today
   for `FILTER(...)` via the existing public function
   `sparql_parser::parse_filter_expression`). `sparql_parser::lib.rs` already
   has a private literal-parsing combinator used by its own `parse_term`
   (string literal with `@lang`/`^^datatype` suffix, numeric literal, boolean
   literal — `parse_string_literal`, `parse_numeric_literal`,
   `parse_boolean_literal`). None of these were `pub`. Following the exact
   precedent of `parse_filter_expression`, this plan adds one new public
   function to `sparql_parser`, `parse_rdf_literal_term`, that tries all three
   literal forms and returns an `RdfLiteral` plus bytes consumed. `datalog_parser`
   calls it from `parse_term` instead of writing a second escaping/lang-tag/
   datatype-suffix parser from scratch.

2. **Engine layer already supports literal quad-pattern arguments.**
   `dag_rdf::Datastore::resources` (`GraphElementManager`) exposes
   `add_literal_resource(RdfLiteral) -> GraphElementId` directly parallel to
   `add_node_resource(RdfResource) -> GraphElementId` (both already existed,
   `dag_rdf/src/lib.rs` lines ~101–105). `datalog::types::QuadPattern`/`Term`
   (`dag_rdf::query::Term`) only ever holds a `GraphElementId` via
   `Term::Resource(id)` — it is agnostic to whether the underlying
   `GraphElement` is `NodeOrEdge` or `GraphLiteral`. So this is a
   **parser-only gap**: once `datalog_parser::intern_term` can turn a
   `ParsedTerm::Literal(RdfLiteral)` into `Term::Resource(ds.resources.add_literal_resource(lit))`,
   the reasoner (`datalog::evaluate_rules`) and unifier need no changes —
   matching an interned literal `GraphElementId` against data is identical to
   matching an interned IRI `GraphElementId`.

3. **`parse_numeric_id_term` is unrelated — do not touch it.**
   Despite the name, it parses `_123` (leading underscore + digits) into
   `urn:x-dag-id:123`, a synthetic anonymous-individual-style IRI, *not* an
   RDF `xsd:integer` literal. It has its own dedicated leading-underscore
   syntax and does not conflict with quoted-string literal parsing. Scoped
   out of this issue: bare numeric/boolean literal shorthand (e.g. bare `42`
   or `true` in term position, with no quotes) is **not** added here, since
   `parse_numeric_id_term`'s `_`-prefixed syntax already lives in that
   grammar slot and a bare (unprefixed) numeric/boolean literal shorthand
   would need a separate disambiguation design (e.g. against IRI-local-name
   parsing) that's out of scope for the reported bug, which is specifically
   about quoted-string literals. Tracked as a possible follow-up, not filed
   as a new issue since it's speculative scope creep beyond the reporter's
   actual need.

## Design

- Add `ParsedTerm::Literal(ingress::RdfLiteral)` to `datalog_parser`'s
  intermediate AST enum.
- Add `sparql_parser::parse_rdf_literal_term(input: &str, ctx: &ParserContext) -> Result<(usize, RdfLiteral), String>`,
  mirroring the existing `parse_filter_expression` signature/pattern, trying
  (in order) string literal (with optional `@lang`/`^^datatype`), numeric
  literal, boolean literal — the same order as `sparql_parser`'s own internal
  `parse_term`.
- Wire it into `datalog_parser::parse_term`: after the `?variable`, `a`
  abbreviation, and `_123` numeric-id checks (all of which are unambiguous
  single-character lead-ins that must stay first), try the new literal parser
  before falling through to `parse_iri`. A literal always starts with `"`,
  `'`, `+`/`-`/digit, or `true`/`false` — none of which can start a
  bare/prefixed IRI term in this grammar (IRIs are `<...>` or `prefix:local`),
  so there's no ambiguity requiring lookahead tricks beyond what `alt`
  already gives us.
- Extend `intern_term` with a `ParsedTerm::Literal(lit) => Term::Resource(ds.resources.add_literal_resource(lit))` arm.
- No changes needed to `ParsedQuadPattern`, `ParsedRuleAtom`, `ParsedRuleHead`,
  `Rule`/`RuleAtom`/`QuadPattern` (engine types) — literals flow through the
  exact same `Term::Resource(GraphElementId)` slot IRIs already use, in both
  rule bodies and rule heads (the term parser is shared, so a literal in head
  position — e.g. asserting a specific literal object — works for free and is
  covered by a test even though it isn't the issue's reported case).

## Test plan (red phase — written first, `#[ignore]`d until implemented)

In `datalog_parser/src/lib.rs`'s `#[cfg(test)] mod tests`:
- `parse_term` accepts a plain double-quoted string literal → `ParsedTerm::Literal(RdfLiteral::LiteralString(_))`.
- `parse_term` accepts a lang-tagged literal (`"hello"@en`) → `RdfLiteral::LangLiteral`.
- `parse_term` accepts a datatype-tagged literal (`"42"^^xsd:integer`) → `RdfLiteral::TypedLiteral`.
- `parse_term` still parses `_123` as `ParsedTerm::Iri("urn:x-dag-id:123")`, unaffected by the new branch (regression guard).

In `tests/datalog_integration.rs`:
- `parse_literal_in_bracket_triple_body`: `[?internal, rdfs:label, "P4712"] .`-shaped body atom (three-arg bracket form) parses to a rule with a `PositivePattern` whose object is the interned literal.
- `parse_literal_in_predicate_first_body`: `rdfs:label[?internal, "P4712"] :- ...` (predicate-first two-arg form) parses successfully with a literal object.
- `parse_literal_in_rule_head`: a literal in head object position parses and interns correctly.
- `parsed_literal_rule_end_to_end`: full pipeline — load Turtle data containing `rdfs:label "P4712"` alongside other labeled resources, parse a rule matching the issue's exact repro shape (label match + a second body atom + a type atom, chained through a shared variable), run `evaluate_rules`, then a SPARQL query proving only the node with the matching literal was derived — not just that parsing succeeded.

All new tests are added with `#[ignore]` in the initial commit (red phase, no
implementation yet); each is un-ignored as its corresponding implementation
piece lands, per this repo's TDD workflow.

## Out of scope

- Bare (unquoted) numeric/boolean literal shorthand in term position (see
  investigation point 3).
- Any change to `parse_numeric_id_term`'s existing `_123` syntax.
