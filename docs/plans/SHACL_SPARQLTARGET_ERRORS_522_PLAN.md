# SHACL-SPARQL: surface `sh:target` SPARQLTarget execution-time errors (#522)

See [#522](https://github.com/daghovland/rdf-datalog/issues/522), follow-up from
[#54](https://github.com/daghovland/rdf-datalog/issues/54). Design rationale for
the split between the Datalog-translated Core-constraint pipeline and the
directly-executed SHACL-AF SPARQL pipeline is in `docs/plans/SHACL_PLAN.md`'s
"SHACL-SPARQL (§5–6 of SHACL-AF)" section.

## Current behaviour

`shacl::validate` (`shacl/src/lib.rs`) already pre-flight parse-checks every
`sh:target`/`sh:sparql` embedded query up front (added in #54,
`sparql_constraints::check_query_syntax`) and hard-fails the whole `validate()`
call with `Err` on a parse error.

However, an **execution-time** failure of a `sh:target [ a sh:SPARQLTarget ;
sh:select "..." ]` query — one that parses fine but fails when actually run
(e.g. it contains a `SERVICE <...>` clause and `sparql_parser::execute` is
called with `NetworkPolicy::Deny`, which parses `SERVICE` syntax successfully
but rejects it at `execute_with_base` time) — is currently `log::warn!`-and-skip
in both places a `Target::Sparql` query executes:

- `shacl::data_targets`'s `Target::Sparql` arm (`shacl/src/lib.rs`) — the direct
  Phase-2 evaluation path, also used by `closed_violations` and
  `evaluate::eval_all` (via the `data_targets` call at the top of the per-shape
  loop) and by the `focus_nodes_of` closure `validate()` passes into
  `sparql_constraints::eval_all` for `sh:sparql` *constraint* evaluation.
- `translate::target_rules`'s `Target::Sparql` arm (`shacl/src/translate.rs`) —
  the Datalog rule-generation path, called once per shape from
  `translate::shapes_to_rules`.

Both contribute **zero focus nodes** for that target on failure rather than
propagating the error, so a shape can silently validate against fewer focus
nodes than intended — data that should have been checked against the shape
appears to conform simply because its target computation failed silently.

This was a deliberate, documented scope decision in #54 (`data_targets` had no
`Result` return, and neither did the Datalog rule-generation pipeline it's
threaded through — `translate::shapes_to_rules`, `evaluate::eval_all`,
`pre_compute_violations`, `closed_violations`, `translate::target_rules`, all
plain-`Vec`-returning at the time).

## Fix

Thread `Result<_, String>` through the whole call chain from `data_targets`/
`target_rules` up to `validate()`, mirroring the pattern
`sparql_constraints::eval_all`/`eval_one_constraint` already use for `sh:sparql`
*constraints* (module doc: "why a malformed/failing embedded query is a hard
`Err` rather than a silently-skipped constraint" — the same rationale now
extends to SPARQLTarget target computation).

Concrete signature changes (`shacl/src/lib.rs`, `shacl/src/evaluate.rs`,
`shacl/src/translate.rs`):

| Function | Before | After |
|---|---|---|
| `lib::data_targets` | `-> Vec<GraphElementId>` | `-> Result<Vec<GraphElementId>, String>` |
| `evaluate::eval_all` | `-> Vec<(GraphElementId, ViolMeta)>` | `-> Result<Vec<(GraphElementId, ViolMeta)>, String>` |
| `lib::closed_violations` | `-> Vec<(GraphElementId, ViolMeta)>` | `-> Result<Vec<(GraphElementId, ViolMeta)>, String>` |
| `lib::pre_compute_violations` | `-> Vec<(GraphElementId, ViolMeta)>` | `-> Result<Vec<(GraphElementId, ViolMeta)>, String>` |
| `translate::target_rules` | `-> Vec<Rule>` | `-> Result<Vec<Rule>, String>` |
| `translate::shapes_to_rules` | `-> (Vec<Rule>, Vec<(GraphElementId, ViolMeta)>)` | `-> Result<(Vec<Rule>, Vec<(GraphElementId, ViolMeta)>), String>` |
| `sparql_constraints::eval_all`'s `focus_nodes_of` closure param | `impl Fn(&ParsedShape) -> Vec<GraphElementId>` | `impl Fn(&ParsedShape) -> Result<Vec<GraphElementId>, String>` |

`validate()` (already `Result<ValidationReport, String>`-returning since #54)
just needs its call sites updated with `?`. Every `Target::Sparql` arm changes
from `Err(e) => { log::warn!(...); }` (contributing nothing) to `Err(e) =>
return Err(format!("sh:target SPARQLTarget query failed for shape {:?}: {e}",
shape.iri))` (or the loop equivalent via `?` once the surrounding function
itself returns `Result`).

Scope stays exactly this: no behavioural change to any other target kind
(`sh:targetNode`/`sh:targetClass`/`sh:targetSubjectsOf`/`sh:targetObjectsOf`),
no change to `sh:sparql` *constraint* error handling (already hard-`Err`),
no change to parse-time pre-flight checking (already hard-`Err` via #54).

## Tests

Added to `tests/shacl_suite.rs`, alongside the existing `spec_s5_sparql_target`
test:

1. `regression_issue_522_sparql_target_execution_error_data_targets` — a shape
   whose `sh:target [ a sh:SPARQLTarget ; sh:select "SELECT ?this WHERE {
   SERVICE <http://example.org/remote/> { ?this a ex:Person } }" ]` query
   parses successfully (valid `SERVICE` syntax) but fails at execution because
   `sparql_constraints::run_select` always runs with `NetworkPolicy::Deny`.
   Before the fix: `validate()` returns `Ok` with an empty report (target
   silently contributed 0 focus nodes, data conforms vacuously). After the
   fix: `validate()` returns `Err` mentioning both `SERVICE` (or "network") and
   the shape's IRI, so the caller can tell *why* validation didn't produce a
   trustworthy report.
2. `regression_issue_522_sparql_target_execution_error_via_datalog_rules` — the
   same failing `SERVICE` target but on a shape whose only constraint is a
   Core (Datalog-translated) one (e.g. `sh:minCount`), to exercise the
   `translate::target_rules` path (used during Datalog rule generation,
   `translate::shapes_to_rules`) independently of the `sparql_constraints`
   / `evaluate.rs` Phase-2 direct-evaluation paths that also call
   `data_targets`.

Both tests are written and `#[ignore]`d first (TDD phase 2), then unignored
once the corresponding code path is fixed (TDD phase 3).

## Non-goals / follow-ups

None identified so far — this is a self-contained plumbing fix with no
speculative follow-up scope, unlike #521's batching work.
