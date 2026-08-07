# Plan: fix `is_safe_rule` panic on unsafe Datalog rules

Issue: [#363](https://github.com/daghovland/rdf-datalog/issues/363) (fifth cluster; epic [#218](https://github.com/daghovland/rdf-datalog/issues/218))

## Problem

`datalog/src/datalog.rs`'s `is_safe_rule` panics (rather than returning a
`Result`) when a rule's head variable isn't bound in its body:

```rust
pub fn is_safe_rule(rule: &Rule) -> bool {
    let unsafe_vars = get_unsafe_head_variables(rule);
    if unsafe_vars.is_empty() {
        true
    } else {
        panic!("Unsafe variables {:?} in rule: {}", unsafe_vars, rule)
    }
}
```

Called from `DatalogProgram::new` and `DatalogProgram::add_rule`
(`datalog/src/reasoner.rs`) — both currently return `Self`/`()`, not
`Result` — so any `--rules` file with a rule like
`ex:Foo[?x,?y] :- ex:Bar[?x] .` crashes the whole `--serve` process at
load time.

A second panic, `apply_substitution_resource`
(`datalog/src/datalog.rs`), panics if a substitution is missing a variable
the rule references — this is only reachable for a rule that already
slipped past `is_safe_rule`'s check, so fixing `is_safe_rule` at the
construction boundary makes this one provably unreachable in practice; see
"Out of scope" below for why it's not being Result-ified too.

## Fix

Extend the existing `ReasoningError` enum (`datalog/src/reasoner.rs`,
already carrying `Contradiction` from #301 and `NotStratifiable` from the
just-merged PR #405) with a new `UnsafeRule(String)` variant (the `String`
describing the offending rule and its unsafe variables, reusing the current
panic message content).

1. `is_safe_rule(rule: &Rule) -> bool` → `is_safe_rule(rule: &Rule) -> Result<(), ReasoningError>` (returns `Ok(())` when safe, `Err(ReasoningError::UnsafeRule(...))` instead of panicking; drop the now-redundant `bool` return since callers only ever cared about the panic/no-panic outcome, never inspected a `false`).
2. `DatalogProgram::new(rules: Vec<Rule>) -> Self` → `Result<Self, ReasoningError>` (propagate `is_safe_rule`'s `Result` with `?` in the loop).
3. `DatalogProgram::add_rule(&mut self, rule: Rule)` → `Result<(), ReasoningError>` (same).
4. Update call sites:
   - `datalog/src/reasoner.rs`'s `evaluate_rules` (around line 369, `let mut program = DatalogProgram::new(partition);`) → `?`. Already returns `Result<_, ReasoningError>`.
   - `datalog/src/incremental.rs`'s `IncrementalReasoner::new` (around line 57, `strata.into_iter().map(DatalogProgram::new).collect()`) → needs to become a fallible collect, e.g. `strata.into_iter().map(DatalogProgram::new).collect::<Result<Vec<_>, _>>()?`. Already returns `Result<Self, ReasoningError>`.
   - Test call sites in `datalog/src/reasoner.rs`'s own `#[cfg(test)]` module and `tests/performance.rs` (both call `DatalogProgram::new` directly) → `.unwrap()`, since all of them build known-safe rule sets — no behavioral change, just following the new signature.
5. Any other `add_rule` callers found by a workspace-wide grep get the same `?`/`.unwrap()` treatment depending on whether they're production or test code.

## Tests (TDD)

Unit tests in `datalog/src/datalog.rs`'s (or `reasoner.rs`'s, wherever
`is_safe_rule` already has coverage — check first) test module:

- `is_safe_rule` returns `Err(ReasoningError::UnsafeRule(_))` for a rule
  whose head references a variable not bound in its body (currently
  panics — confirm red first).
- Regression: a normal safe rule still returns `Ok(())`.

Integration-level: a test through `datalog::evaluate_rules` (the real
top-level entry point a `.datalog` rules-file load goes through) with an
unsafe rule, asserting a clean `Err` instead of a crash.

## Out of scope

- `apply_substitution_resource`'s panic is **not** being converted in this
  PR. Once `is_safe_rule` is enforced at every `DatalogProgram`
  construction/mutation point (this PR), no unsafe rule can ever reach
  `apply_substitution_resource` in production — it's an internal
  invariant, not a live external-input DoS surface, once this fix lands.
  Threading `Result` through it would require touching every call site in
  the hot substitution/materialisation path (`evaluate`,
  `materialise_seminaive`, etc.) for no remaining reachability. Leave the
  panic in place but add a doc comment recording why it's now provably
  unreachable and linking back to this PR/#363, so a future reader isn't
  confused about why it wasn't also fixed.
- The remaining #363 cluster (`rdf_owl_translator`/`axiom_parser.rs`
  cyclic-list/malformed-arity sites) stays as a further follow-up PR; #363
  remains open.
