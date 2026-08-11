# Plan: turtle xsd:string collapse investigation (#360)

Issue: [#360](https://github.com/daghovland/rdf-datalog/issues/360), follow-up to
[#346](https://github.com/daghovland/rdf-datalog/issues/346) and
[#205](https://github.com/daghovland/rdf-datalog/issues/205), part of epic
[#197](https://github.com/daghovland/rdf-datalog/issues/197).

Branch: `fix/360-turtle-xsd-string`.

## What the issue assumed

The issue's premise: `turtle::convert_literal` (`turtle/src/lib.rs`, formerly
described as `turtle_parser/src/lib.rs`) has

```rust
let datatype = lit.datatype().into_owned().into_string();
if datatype == "http://www.w3.org/2001/XMLSchema#string" {
    RdfLiteral::LiteralString(lit.value().to_owned())
} else {
    RdfLiteral::TypedLiteral { literal: lit.value().to_owned(), type_iri: IriReference(datatype) }
}
```

and the ask was: stop taking the `LiteralString` branch for a literal that
was genuinely written as `"..."^^xsd:string` in Turtle source, so that
`STRDT()`/`STRLANG()` (SPARQL 1.1 §17.4.3.5/6) can correctly reject it (they
must error on anything that isn't a *simple* literal — no language tag, no
datatype, not even `xsd:string`) instead of silently accepting it because the
distinction was erased before evaluation ever saw it.

## What the investigation actually found

### Finding 1 (decisive): the collapse does not originate in `convert_literal` — it is baked into `oxrdf::Literal` itself

`turtle`/`rdf_canon`/the RML crates all parse via `oxttl` (Turtle/TriG/
N-Triples/N-Quads), which represents literals as `oxrdf::Literal`. Its
constructor:

```rust
// oxrdf-0.3.3/src/literal.rs
pub fn new_typed_literal(value: impl Into<String>, datatype: impl Into<NamedNode>) -> Self {
    let value = value.into();
    let datatype = datatype.into();
    Self(if datatype == xsd::STRING {
        LiteralContent::String(value)   // <- same variant as new_simple_literal
    } else {
        LiteralContent::TypedLiteral { value, datatype }
    })
}
```

collapses `xsd:string`-typed construction into the *same* internal variant
`new_simple_literal` produces, unconditionally — regardless of whether the
caller wrote `^^xsd:string` explicitly. `oxttl`'s Turtle grammar
(`oxttl-0.2.3/src/terse.rs:857`) calls exactly this constructor for every
`"..."^^<iri>` literal it parses, `xsd:string` included.

Verified directly (not just read — compiled and ran):

```rust
let a = Literal::new_simple_literal("foo");
let b = Literal::new_typed_literal("foo", xsd::STRING);
// a.is_plain() = true, b.is_plain() = true
// a.datatype() = xsd:string, b.datatype() = xsd:string
// a == b: true
```

So by the time `oxttl` hands a `Literal` back to `turtle::convert_literal`,
"was this written as `"foo"` or `"foo"^^xsd:string`" is **already gone** —
`convert_literal`'s `if datatype == xsd:string { collapse }` branch is dead
weight, not the actual cause. Removing it does not recover the distinction;
it would only additionally start tagging *every* bare `"foo"` (no datatype at
all) as `TypedLiteral{xsd:string, "foo"}` too (since `lit.datatype()` returns
`xsd:string` for plain literals as well under RDF 1.1's semantics), which is
strictly worse, not better.

`oxttl`'s public surface (`oxttl-0.2.3/src/lib.rs`) is only the streaming
triple/quad parsers (`TurtleParser`, `TriGParser`, `NTriplesParser`,
`NQuadsParser`, `N3Parser`) plus `TextPosition`/`TurtleSyntaxError` for error
reporting — there is no token-level or span-preserving API that would let a
caller recover which literal form was written in the source.

**Consequence:** representing "explicit `^^xsd:string`" distinctly from
"plain, no datatype at all" is not achievable through the currently-used
`oxttl`/`oxrdf` parsing stack without hand-rolling a parallel Turtle literal
tokenizer — quoted-string forms (`'`, `"`, `'''`, `"""`) with their escape
rules, comment skipping, and prefixed-vs-absolute datatype IRI resolution,
kept in permanent lockstep with `oxttl`'s own grammar, ×4 for
Turtle/TriG/N-Triples/N-Quads (`turtle/src/lib.rs` has one `parse_*` function
per format). This is a substantially bigger and more fragile undertaking than
the issue anticipated ("a real representation change in how literals are
stored in-memory after parsing" — the issue expected the risk to be in the
*storage* representation and its consumers, not in the *parser dependency*
being structurally unable to observe the input distinction at all).

This is arguably also the textbook-correct behavior for the *library*: RDF
1.1 §3.3 abolished the RDF 1.0 "plain literal without datatype" category —
every literal has a datatype, defaulting to `xsd:string` when none is
written, and `oxrdf` implements that literally (a plain literal and an
`xsd:string`-typed literal are the same RDF term, full stop, no surface-syntax
provenance kept). The SPARQL 1.1 Recommendation's `STRDT`/`STRLANG` fixtures
(`strdt03`/`strlang03`, `dawgt:approvedBy` dated 2012-01-31 — pre-dating the
RDF 1.1 Recommendation, 2014-02-25) encode the now-abolished RDF 1.0 "simple
literal" category as something functions can still observe and reject. Any
engine built on `oxrdf`'s term model is, by construction, in the same
position: this specific pair of fixtures asks for information the underlying
RDF 1.1 data model no longer carries.

### Finding 2: even if the distinction *could* be recovered at ingestion, storing it would silently break existing triple-pattern joins

Investigated in case Finding 1 turned out to have a workaround. `dag_rdf`
interns every `GraphElement` (including `GraphElement::GraphLiteral`) into an
exact-key `HashMap<GraphElement, GraphElementId>`
(`GraphElementManager::resource_map`, `dag_rdf/src/lib.rs:77-99`, derived
`PartialEq`/`Hash` on `RdfLiteral`, `ingress/src/lib.rs:50-70`). Triple-pattern
matching gates on this map directly:

```rust
// sparql_parser/src/execute.rs:1442-1448 (eval_triple_pattern_core)
for term in [&tp.subject, &tp.predicate, &tp.object] {
    if let Term::Constant(gel) = term {
        if !datastore.resources.resource_map.contains_key(gel) {
            return Ok(Vec::new());
        }
    }
}
```

If Turtle ingestion started producing `TypedLiteral{xsd:string, "foo"}` for
data written as `"foo"^^xsd:string`, that literal would intern to a
*different* `GraphElementId` than a query pattern's plain `"foo"` (or than
`"foo"` ingested elsewhere without the explicit datatype) — silently
returning zero matches for any query that pattern-matches against
`xsd:string`-typed data using a plain-literal constant, with no error. This
is a join-engine change, not just a storage-representation change: fixing it
properly would mean the interning/matching layer doing *value*-based lookup
for this one specific pair of forms instead of raw key equality — out of
scope for a "stop collapsing at ingestion" change and its own can of worms
(also affects `rdf_canon`'s canonical-form output, which is derived from
this same interned representation, and DISTINCT/GROUP BY dedup keys
downstream).

### Finding 3 (real, in-scope bug found and fixed): `sparql_parser`'s own query-text literal grammar already keeps the two forms distinct, and `FILTER` equality didn't normalize them

Unlike Turtle ingestion, `sparql_parser::parse_string_literal`
(`sparql_parser/src/lib.rs:1600-1638`) does **not** collapse `^^xsd:string`
written directly in SPARQL query text — `"foo"^^xsd:string` parses to
`RdfLiteral::TypedLiteral{ type_iri: xsd:string, literal: "foo" }`, distinct
from `RdfLiteral::LiteralString("foo")`. So the plain/`xsd:string`
value-equivalence gap was already reachable today, with zero Turtle
involvement, purely inside a `FILTER`. Confirmed by direct probe before any
fix (`FILTER("foo" = "foo"^^xsd:string)` evaluated to `false`).

`values_equal` (`sparql_parser/src/execute.rs`, used by `=`/`!=`/`IN`/`NOT
IN` per SPARQL 1.1 §17.4.1.9) already normalizes the analogous numeric and
boolean cross-representation split from #208 (computed
`IntegerLiteral`/`BooleanLiteral` vs. parsed `TypedLiteral`), but had no
equivalent case for the string/`xsd:string` split — it fell through to raw
`a == b`, which sees `LiteralString` and `TypedLiteral{xsd:string, ..}` as
unequal even for the same lexical value. This is exactly the "must not
regress" property the issue calls out (RDF 1.1's plain/`xsd:string`
value-equivalence must hold in `FILTER`/`=`/join contexts outside the
specific STRDT/STRLANG type-checking use case), except it turns out this
property didn't fully hold *before* this issue either.

## Decision

Neither of the issue's proposed options ("keep the distinction all the way
through" vs. "a narrower fix scoped just to the STRDT/STRLANG code path")
applies, because both assumed the distinction is recoverable from parsed
Turtle data at all. Finding 1 shows it is not, with the current `oxttl`/
`oxrdf` dependency. Fixing it for real would mean hand-rolling literal
tokenization outside `oxttl` (assessed and rejected above as materially
riskier than what #346 already declined to attempt as a "quick fix"), so:

- **`STRDT() TypeErrors` / `STRLANG() TypeErrors` stay skipped** in
  `tests/w3c_sparql11_suite.rs::w3c_sparql11_functions`, with the skip-list
  comment rewritten to document Finding 1 (the real blocker) instead of the
  now-known-incorrect "it's `convert_literal`'s collapse" explanation.
  `turtle::convert_literal` itself is left unchanged — editing it would not
  fix anything and would risk Finding-2-shaped regressions for zero benefit.
- **Finding 3's bug is fixed**, since it's real, in-scope (SPARQL evaluation
  correctness, not ingestion), low-risk, and squarely inside "must not
  regress the RDF 1.1 value-equivalence" from the issue's own scope note:
  `values_equal` gains a `simple_or_xsd_string_value` normalization case,
  symmetric with the existing numeric/boolean cases, restricted to exactly
  `LiteralString` vs. `TypedLiteral{xsd:string, ..}` (language-tagged and
  other-datatype literals are untouched — verified by
  `filter_eq_lang_literal_vs_xsd_string_not_equal`/
  `filter_eq_lang_literal_vs_plain_not_equal`). `SAMETERM` is deliberately
  left alone: it uses raw `==` for term-identity, not value-equality,
  purpose, and the issue's scope note is about `=`/`FILTER`/join contexts,
  not `sameTerm`. This is noted as a possible follow-up, not fixed here.
- **A follow-up issue should be filed** (unlabeled, per the repo's `ready`-
  label gate — awaiting Dag's review) for the actual structural fix implied
  by Finding 2 if #360's underlying motivation is still wanted: value-based
  literal interning/canonicalization in `dag_rdf::GraphElementManager` that
  treats `LiteralString` and `TypedLiteral{xsd:string, ..}` as the same
  interned node while *also* recovering surface-syntax provenance for
  `STRDT`/`STRLANG` — which would itself require solving Finding 1 first
  (a custom literal tokenizer, or a different Turtle parser dependency
  entirely). This is a substantially larger, riskier effort than #360 as
  filed and deserves its own scoping/plan before anyone picks it up.

## Test coverage

`sparql_parser/tests/filter_eq_string_xsd_string_normalization_tests.rs`
(new, TDD: all `=`/`!=`/`IN`/`NOT IN` cases were added `#[ignore]`d first,
confirmed red via `cargo test -- --ignored`, then unignored after the
`values_equal` fix and confirmed green):

- `filter_eq_plain_matches_explicit_xsd_string` /
  `filter_eq_explicit_xsd_string_matches_plain` — `"foo" = "foo"^^xsd:string`
  holds, both operand orders.
- `filter_ne_plain_vs_explicit_xsd_string_is_false` — `!=` symmetric case.
- `filter_in_plain_matches_xsd_string_in_list` /
  `filter_not_in_excludes_xsd_string_match` — `IN`/`NOT IN` route through the
  same `values_equal` normalization.
- `filter_eq_different_values_still_not_equal` (not ignored — baseline that
  must already pass and must keep passing) — different lexical values must
  not become trivially equal.
- `filter_eq_lang_literal_vs_xsd_string_not_equal` /
  `filter_eq_lang_literal_vs_plain_not_equal` (not ignored) — guards against
  the normalization accidentally widening to language-tagged literals.

`tests/w3c_sparql11_suite.rs::w3c_sparql11_functions` — full workspace
`cargo test --workspace` run (not just the two directly-touched crates)
confirms nothing else in `turtle_parser`/`sparql_parser`/`jsonld_parser`/
equality-comparison code regressed, since `turtle::convert_literal` itself
was left untouched.

## Blast radius grepped but found not applicable

`LiteralString` matches were grepped across `turtle_parser` (now `turtle`),
`sparql_parser`, `jsonld_parser`, `dag_rdf`, `ingress`, plus `rml`,
`rdf_canon`, `shacl`, `manchester_parser`, `ottr`, `sparql_endpoint`,
`vqs_index`, `backlog`, `datalog_parser` (`ingress::RdfLiteral` is a
workspace-wide type). Since `turtle::convert_literal` was **not** changed (no
representation change actually shipped — see Decision above), none of these
consumers needed auditing for behavior change; they are listed here only to
record that the grep was done as instructed, before Finding 1 made the
originally-planned representation change moot.
