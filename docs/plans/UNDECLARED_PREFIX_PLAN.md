# Undeclared prefix in SELECT/ASK/CONSTRUCT/DESCRIBE silently matches nothing — plan (issue #389)

Related: [#389](https://github.com/daghovland/rdf-datalog/issues/389) (this issue).

## The bug

`sparql_parser::lib::parse_prefixed_name` (the single shared parser for every
`prefix:local` occurrence in a SPARQL query — triple-pattern subject/predicate/
object, property-path IRIs, `^^`-datatype IRIs on typed literals, and function
names in `FILTER`/`BIND` expressions) falls back to treating an *undeclared*
prefix as a literal string when the prefix isn't in `ctx.prefixes`:

```rust
let base = ctx
    .prefixes
    .get(prefix)
    .cloned()
    .unwrap_or_else(|| prefix.to_string() + ":");
Ok((after_local, IriReference(base + &local)))
```

So `SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }` "parses" into a
pattern with predicate IRI `totallyundeclaredprefix:foo`, which is syntactically
valid but semantically nonsense — it can never match real data, so the query
returns HTTP 200 with an empty result set instead of an error. SPARQL Update
(`sparql_endpoint::sparql_update`) doesn't have this problem because
`INSERT DATA`/`DELETE DATA` bodies are parsed with `turtle_parser::parse_turtle`
(the shared Turtle/TriG parser via `rio_turtle`), which already rejects
undeclared prefixes as a genuine parse error — the two code paths (query vs.
update) just use entirely different parsers, and only the hand-rolled query
parser has the permissive fallback. There's no reusable "prefix X not declared"
message to borrow: the Update path's rejection is `rio_turtle`'s own generic
parse-error text, not a first-class message we construct. This plan makes the
query parser fail loudly at the same call site instead, using nom's own
`Err::Failure` mechanism so the failure isn't silently swallowed by `alt()`
backtracking elsewhere in the parser.

## Fix shape

- In `parse_prefixed_name` (`sparql_parser/src/lib.rs`, ~line 1521-1583): once
  the input has matched the *shape* of a prefixed name (`take_while` prefix +
  literal `:` + non-empty local-or-prefix, and it's not one of the reserved
  keyword-prefixes already rejected above), looking up `ctx.prefixes` and
  failing to find `prefix` is no longer ambiguous — nothing else in any `alt()`
  this function participates in could plausibly reinterpret `prefix:local` as
  something other than a prefixed name once we're this far in. Return
  `Err(nom::Err::Failure(...))` instead of silently falling back, so `alt()`
  callers (`parse_term`, `parse_path_iri`, `parse_function_call`, the `^^`
  datatype-IRI parser, the `FILTER`/`BIND` literal-constant `alt`) stop trying
  further alternatives instead of masking the error with a different, more
  confusing parse failure (or, as today, no failure at all).
  - Checked call sites (grep for `parse_prefixed_name` across
    `sparql_parser/src/lib.rs`): `parse_path_iri` (property-path IRIs),
    `parse_term` (triple-pattern subject/predicate/object), the `^^`-datatype
    IRI alt in `parse_string_literal`, the `FILTER`/`BIND` constant alt, and
    `parse_function_call`'s function-name alt. In every one, `parse_prefixed_name`
    is the last alternative tried for anything containing a bare `word:` shape,
    or (in `parse_function_call`) the remaining bare-word fallback couldn't
    have matched a colon-containing name anyway (`take_while1` alphanumeric/`_`
    stops at `:`, then the required `(` wouldn't be next) — so switching to
    `Failure` here doesn't change behavior for any query that would otherwise
    have parsed successfully; it only turns a previously-silent semantic bug
    into a hard parse error.
  - `nom::error::Error<&str>` (the error type used throughout this parser)
    carries no custom message field, only the offending remaining input slice
    and an `ErrorKind`. The existing top-level handling in
    `sparql_endpoint::query` already formats parse errors as
    `format!("Parse error: {:?}", e)`, which includes the remaining input —
    for an undeclared prefix that remaining input starts at the offending
    `prefix:local` token itself (e.g. `"totallyundeclaredprefix:foo ?o }"`),
    so the resulting 400 response already names the culprit without needing a
    new error-type plumbing change. Use `ErrorKind::Verify` (closest existing
    variant semantically — "the prefix did not verify against declared
    prefixes").
- No changes needed in `sparql_endpoint` itself: `run_select_query`/
  `run_ask_query`/... already treat any `Err` from `parse_query` as
  `StatusCode::BAD_REQUEST`. Once the parser fails instead of succeeding, the
  existing error path takes over end-to-end.

## Tests (red phase, `#[ignore]`)

Parser-level unit tests in `sparql_parser/src/lib.rs` `#[cfg(test)] mod tests`
(or a new `sparql_parser/tests/undeclared_prefix_tests.rs` integration test —
using the latter since it mirrors existing sibling test files like
`base_iri_tests.rs`):

- `test_select_with_undeclared_prefix_is_parse_error` — the exact repro from
  the issue: `SELECT * WHERE { ?s totallyundeclaredprefix:foo ?o }` (no
  `PREFIX` declared for `totallyundeclaredprefix`) must return `Err(_)` from
  `parse_query`, not `Ok` with an empty-matching pattern.
- `test_ask_with_undeclared_prefix_is_parse_error` — same shape, `ASK` form.
- `test_construct_with_undeclared_prefix_is_parse_error` — undeclared prefix
  in the `CONSTRUCT` template.
- `test_describe_with_undeclared_prefix_is_parse_error` — `DESCRIBE
  undeclaredprefix:foo`.
- `test_undeclared_prefix_in_filter_datatype_is_parse_error` — undeclared
  prefix used as a `^^`-datatype IRI inside a `FILTER`, e.g.
  `FILTER(?o = "1"^^undeclaredprefix:integer)`.
- `test_undeclared_prefix_in_values_is_parse_error` — undeclared prefix as a
  constant inside `VALUES ?o { undeclaredprefix:foo }`.
- `test_declared_prefix_still_works` (control/non-regression) — the same
  query shape but with the prefix properly declared must still parse and
  execute correctly (guards against the fix being over-eager and rejecting
  legitimate declared prefixes).

Integration test in `sparql_endpoint`
(`sparql_endpoint/tests/undeclared_prefix_http_tests.rs`, mirroring existing
endpoint test files) proving the end-to-end HTTP behavior:

- `test_select_with_undeclared_prefix_returns_400` — POST the exact repro
  query from the issue to `/sparql` and assert `StatusCode::BAD_REQUEST`
  (not 200 with an empty binding set).

## Existing-test audit

Grepped `sparql_parser/tests/*.rs`, `tests/sparql12_suite.rs`, and
`tests/w3c_sparql11_suite.rs` for any query that uses a prefixed name without a
corresponding `PREFIX` declaration in the same query — found none relying on
the old permissive fallback; every existing use of `xsd:`/`rdfs:`/etc. in test
fixtures declares the prefix it uses. `cargo test --workspace` after the fix
is the final confirmation.

## Implementation order

1. `test_declared_prefix_still_works` (should already be green — pure
   non-regression baseline, added first to lock in current behavior before
   touching the parser).
2. `test_select_with_undeclared_prefix_is_parse_error` (the core repro).
3. `test_ask_with_undeclared_prefix_is_parse_error`,
   `test_construct_with_undeclared_prefix_is_parse_error`,
   `test_describe_with_undeclared_prefix_is_parse_error` (same fix, more
   surface coverage).
4. `test_undeclared_prefix_in_filter_datatype_is_parse_error`,
   `test_undeclared_prefix_in_values_is_parse_error` (confirm the fix is
   really centralized in `parse_prefixed_name`, not per-call-site).
5. `test_select_with_undeclared_prefix_returns_400` (end-to-end HTTP).
6. Implement the `Err::Failure` change in `parse_prefixed_name`, un-ignoring
   tests as they go green.
7. Full quality gate (`cargo fmt`, `cargo clippy -D warnings`,
   `cargo test --workspace`).
