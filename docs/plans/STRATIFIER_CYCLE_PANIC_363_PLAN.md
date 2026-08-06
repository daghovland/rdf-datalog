# Plan: fix `RulePartitioner::dfs_cycle` panic on non-stratifiable negation cycles

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (second cluster; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

`datalog/src/stratifier.rs`'s `RulePartitioner::dfs_cycle` calls
`panic!("Datalog program has a cycle with negation and is not stratifiable!")`
when a negative dependency edge sits on a cycle, instead of returning an
error. A `.datalog` file with mutual negation
(`A(x) :- NOT B(x)`, `B(x) :- NOT A(x)`) crashes the whole `--serve` process
at load time — reached via `RulePartitioner::order_rules()`, called from
`DatalogProgram::new`'s caller `evaluate_rules` (`reasoner.rs:353-361`, eager
`--ontology`/`--rules` loading) and `IncrementalReasoner::new`
(`incremental.rs:53-62`, server startup with initial rules).

## Fix

`datalog::reasoner::ReasoningError` already exists for exactly this purpose —
issue [#301](https://github.com/daghovland/rdf-datalog/issues/301) converted
an earlier panic (genuine logical contradiction) into
`Err(ReasoningError::Contradiction(String))`, and both call sites
(`evaluate_rules`, `IncrementalReasoner::new`) already return
`Result<_, ReasoningError>` and already propagate other `ReasoningError`s via
`?`. This fix extends the same, already-proven pattern:

1. Add a new variant `ReasoningError::NotStratifiable(String)` (the `String`
   describes the offending rule, same convention as `Contradiction`).
2. Change `RulePartitioner::order_rules(self) -> Vec<Vec<Rule>>` to
   `-> Result<Vec<Vec<Rule>>, ReasoningError>`. Thread the `Result` through
   its private helpers (`find_cycle`, `handle_cycle`, `dfs_cycle`) — the
   simplest change is to have `dfs_cycle` return
   `Result<bool, ReasoningError>` (an `Err` instead of the panic, propagated
   via `?` through the recursive calls and through `find_cycle`/
   `handle_cycle`/`order_rules`).
3. Update the two production call sites to propagate with `?`:
   - `datalog/src/reasoner.rs:354-355` (`evaluate_rules`)
   - `datalog/src/incremental.rs:54-55` (`IncrementalReasoner::new`)
   Both functions already return `Result<_, ReasoningError>`, so this is a
   one-line change at each site (`stratifier.order_rules()?`).
4. Update test call sites that call `order_rules()` directly
   (`tests/datalog_integration.rs`, `tests/owl_integration.rs`,
   `tests/performance.rs`) to `.unwrap()` (they all construct rule sets that
   are known-stratifiable, so `.unwrap()` is correct there — no behavior
   change for those tests).

No signature change is needed on `RulePartitioner::new` itself — only
`order_rules`, which is the method that currently panics.

## Tests (TDD — written first, ignored, then unignored as fixed)

New tests in `datalog/src/stratifier.rs`'s existing `#[cfg(test)]` module (or
`tests/datalog_integration.rs` if stratifier has no unit tests currently —
check first):

- `order_rules_returns_err_on_negation_cycle` — build two rules forming a
  mutual-negation cycle (`A(x) :- NOT B(x)`, `B(x) :- NOT A(x)`, same shape as
  the issue's repro) and assert `order_rules()` returns
  `Err(ReasoningError::NotStratifiable(_))`, not a panic.
- `order_rules_still_succeeds_on_stratifiable_program` (regression) — a
  normal negation-using but stratifiable program (e.g. `A(x) :- B(x)`,
  `C(x) :- NOT A(x)`) still returns `Ok` with the correct strata.

Integration-level: a test exercising `evaluate_rules` (or
`IncrementalReasoner::new`) with a mutually-negating rule set and asserting
it returns `Err` cleanly instead of crashing the test process — proves the
error actually reaches the top-level API a `--rules` file load goes through.

## Out of scope

Remaining #363 clusters (`datalog.rs` unsafe-rule panics, `eli2rl.rs`
unimplemented-construct panics, `rdf_owl_translator`/`axiom_parser.rs` sites,
`rml/src/translate.rs` dangling parent panic) stay as further follow-up PRs;
#363 remains open.
