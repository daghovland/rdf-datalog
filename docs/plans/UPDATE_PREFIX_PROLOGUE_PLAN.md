# Plan: honour SPARQL Update `PREFIX`/`BASE` prologue (issue #392)

Epic/issue: [#392](https://github.com/daghovland/rdf-datalog/issues/392)

## Bug

`sparql_endpoint`'s hand-rolled SPARQL Update parser (`sparql_update.rs`)
recognises `PREFIX`/`BASE` declarations only well enough to skip past them
(`skip_prologue`) — the prefix→IRI mappings are discarded, never attached to
the operation that follows. Any prefixed name used inside `INSERT DATA` /
`DELETE DATA` / a WHERE-form update's template or pattern then fails to
resolve, even when the very same request declared it:

```
PREFIX ex: <http://example.com/ns/> INSERT DATA { <urn:testpkg> a ex:Thing . }
→ 400: "The prefix ex: has not been declared"
```

An equivalent `SELECT` with the same `PREFIX` works, because
`sparql_parser::parse_query` records prologue prefixes into its
`ParserContext` as it parses them and resolves prefixed names against that
context. SPARQL Update's parser has no equivalent plumbing.

## Root cause detail

- `skip_prologue` (~line 126) scans past `PREFIX prefix: <iri>` / `BASE <iri>`
  text and returns only the remainder — no mapping is recorded anywhere.
- `parse_update` calls `skip_prologue` before parsing each operation, so
  `UpdateOp::InsertData { content }` etc. only ever hold the raw text
  *inside* `{ ... }` — prefix declarations never reach the operation.
- `prepare_update` passes that raw `content` to `parse_turtle_content`, which
  calls `turtle::parse_turtle` (rio_turtle-backed). Turtle's own prefix
  syntax is `@prefix p: <iri> .` (trailing dot), not SPARQL's `PREFIX p: <iri>`
  (no dot) — so even surviving prologue text wouldn't be recognised as-is.
- WHERE-form ops (`InsertWhere`/`DeleteWhere`/`DeleteInsertWhere`, and the
  `PatternUpdate` variant applied lazily in `apply_prepared_update`) go
  through `sparql_parser::parse_query` via a synthesized
  `SELECT * WHERE { ... }` string, each with a **fresh, empty**
  `ParserContext { prefixes: HashMap::new(), .. }` (`eval_where_pattern`,
  `parse_template`) — so the same gap exists there too.

## Fix

1. `skip_prologue` returns the captured `(prefix, iri)` pairs alongside the
   remaining text instead of discarding them.
2. `parse_update` accumulates prefixes into a running `HashMap<String, String>`
   across the whole request — SPARQL 1.1 Update §29's `Prologue` production
   allows (and real-world implementations treat) a prologue before each
   `;`-separated operation, but declarations remain in scope for the rest of
   the request (later declarations of the same prefix name shadow earlier
   ones for subsequent operations; earlier ones stay visible to operations
   that don't redeclare them). Each `UpdateOp` variant that carries raw
   content/template/pattern text gains a `prefixes: HashMap<String, String>`
   field snapshotting the map as of that point in the request.
3. `prepare_update`, for `InsertData`/`DeleteData`, prepends synthesized
   `@prefix name: <iri> .` lines to the Turtle content before calling
   `turtle::parse_turtle` (Turtle's own directive syntax, so no parser change
   needed there).
4. For WHERE-form ops, `eval_where_pattern` and `parse_template` take a
   `&HashMap<String, String>` and seed `ParserContext::prefixes` with it
   before calling `sparql_parser::parse_query`, instead of always starting
   from an empty map. `PreparedOp::PatternUpdate` gains a `prefixes` field so
   the map survives from `prepare_update` time to the lazy WHERE evaluation
   in `apply_prepared_update`.
5. `BASE` declarations are captured the same way, for completeness, but this
   issue is scoped to `PREFIX` resolution; `BASE` handling is not otherwise
   changed here (existing relative-IRI behaviour is unaffected since none of
   the fixed test cases use relative IRIs).

## Test plan (red phase — all `#[ignore]`d until implementation)

Added to `sparql_endpoint/tests/sparql_update_where.rs` (existing file for
SPARQL-Update-parsing-adjacent gaps) or a new file
`sparql_endpoint/tests/sparql_update_prefix.rs`:

1. `update_insert_data_resolves_prologue_prefix` — the exact repro from the
   issue: `PREFIX ex: <...> INSERT DATA { <urn:testpkg> a ex:Thing . }`
   against a writable server, then `ASK` confirms the triple exists.
2. `update_delete_data_resolves_prologue_prefix` — pre-load the triple via
   Turtle, then `PREFIX ex: <...> DELETE DATA { <urn:testpkg> a ex:Thing . }`,
   then `ASK` confirms it's gone.
3. `update_multi_op_prologue_prefix_carries_forward` — two `;`-separated ops,
   `PREFIX ex: <...> INSERT DATA { ... ex:Thing ... } ; PREFIX ex2: <...>
   INSERT DATA { ... ex2:Other ... }`, second op only declares `ex2` — verify
   `ex:` from the first prologue is still resolvable in the *second*
   operation (forward-carry scoping) and both triples land.
4. `update_insert_where_resolves_prologue_prefix` — WHERE-form
   (`INSERT { ?s ex:label ?o } WHERE { ?s ex:name ?o }`) with a prologue
   `PREFIX ex: <...>`, confirming the same fix covers `PatternUpdate`.

Unit-level parser tests in `sparql_endpoint/src/sparql_update.rs`'s existing
`#[cfg(test)] mod tests` verify `skip_prologue`/`parse_update` actually
capture the prefix map (not just end-to-end behaviour).

## Implementation order

1. `skip_prologue` returns captured prefixes (smallest, testable in
   isolation).
2. Thread the accumulated map through `parse_update` into each `UpdateOp`
   variant that needs it (compiles once all variants + call sites are
   updated — no behaviour change yet since nothing consumes the field).
3. `prepare_update`: synthesize `@prefix` lines for `InsertData`/`DeleteData`.
   Unignore repro tests 1–3.
4. `eval_where_pattern`/`parse_template`/`PreparedOp::PatternUpdate`: seed
   `ParserContext::prefixes`. Unignore test 4.
5. Code-smell pass, quality gate, commit/push/PR.
