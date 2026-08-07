# Plan: parse Fuseki assembler Turtle properly, warn on ignored config

Issue: [#415](https://github.com/daghovland/rdf-datalog/issues/415) (reopened — the first pass, direct-to-main documentation, wasn't
enough; the underlying implementation itself needed to improve)

## Problem with the current implementation

`sparql_endpoint::admin::admin_create_dataset`'s `text/turtle` branch calls
`extract_fuseki_name_from_assembler`, which does a **substring search** for
the literal text `fuseki:name` followed by the next `"`-quoted string — not
real Turtle parsing. This is fragile (breaks on different prefix bindings,
whitespace, quoting styles, or property ordering that are all
equally-valid Turtle) and silently ignores everything else in the
assembler document: the declared dataset type (`ja:MemoryDataset` vs.
anything else) and any `fuseki:endpoint` configuration — every dataset
created this way is always an in-memory dataset with the full default
route set, regardless of what the payload actually declares.

## Fix

1. Parse the request body as real Turtle via `turtle::parse_turtle` (already
   a dependency; same parser used everywhere else in this codebase) into a
   temporary `Datastore`, instead of string-searching the raw bytes. A
   parse failure returns `400 Bad Request` with the parser's error message
   — better diagnostics than the old "could not extract fuseki:name"
   catch-all.
2. Extract `fuseki:name` (`http://jena.apache.org/fuseki#name`) via a real
   triple-pattern lookup (`Datastore::get_triples_with_predicate`, after
   interning the predicate IRI — follow the pattern already used in
   `eli/src/eli2rl.rs`'s `get_obj_prop_pattern` for "intern an IRI just to
   get its ID for a lookup, no insertion side-effect that matters since
   it's discarded either way"). If no `fuseki:name` triple with a plain
   string literal object exists, `400 Bad Request` (same user-facing
   contract as before).
3. **New**: if the assembler declares a `fuseki:dataset` whose `rdf:type`
   is present and is NOT `ja:MemoryDataset`
   (`http://jena.hpl.hp.com/2005/11/Assembler#MemoryDataset`), emit
   `log::warn!` naming the declared type and stating that Dagalog only
   supports in-memory datasets via this API and is creating one anyway —
   don't reject the request (preserves existing behavior for downstream
   clients that don't care), just stop it being silent.
4. **New**: if the assembler declares any `fuseki:endpoint` triples on the
   service node, emit `log::warn!` that endpoint configuration in the
   assembler payload is not honored — Dagalog always exposes its full
   fixed set of dataset-scoped routes regardless of what's declared here.
5. Keep the rest of `admin_create_dataset` unchanged (dataset creation
   itself, response shape) — this PR only changes how the Turtle body is
   interpreted before that point.
6. Update `docs/user/deployment.md`'s existing Fuseki-assembler section
   (added when this issue was first — incompletely — resolved) to reflect
   the new behavior: real Turtle parsing (not substring search), and that
   unsupported dataset types / endpoint config now produce a server-side
   warning log instead of being silently dropped with no signal at all.

## Tests (TDD)

- Unit/integration test (`sparql_endpoint/tests/` or `admin.rs`'s own test
  module, whichever existing Fuseki-assembler tests already live in —
  check first) confirming:
  - the issue's exact repro payload still creates a dataset with the
    correct name (regression — must not break the existing working case).
  - a payload with different (but equally valid) Turtle formatting for the
    same triples — e.g. a different prefix binding, or `fuseki:name` and
    `rdf:type` in a different order — still extracts the name correctly
    (this is the actual proof the fix works, since the old substring
    search would likely already pass the exact-repro test but could be
    fooled by reordering).
  - a payload declaring `ja:TDBDataset` instead of `ja:MemoryDataset`
    still succeeds (creates an in-memory dataset) but a warning is logged
    (use whatever log-capturing test utility this crate already has, if
    any — check other tests that assert on `log::warn!` output; if none
    exists, it's acceptable to just verify the dataset is still created
    successfully and note in a comment that the warning itself isn't
    mechanically asserted).
  - a payload declaring a `fuseki:endpoint` block still succeeds, with a
    warning logged for the ignored endpoint config.
  - a genuinely malformed Turtle payload (not just missing `fuseki:name` —
    actually syntactically invalid) now returns `400` with a parser error
    message, not the old generic "could not extract" message.

## Out of scope

Whether to actually *honor* declared endpoint config or reject non-memory
dataset types outright is a bigger design question not asked for here —
this PR only makes the current silent-drop behavior visible via warnings,
per the user's explicit request, not change what's actually supported.
