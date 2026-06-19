---
name: project-sparql-aggregates-paths
description: SPARQL aggregate functions (COUNT/SUM/AVG/MIN/MAX/GROUP_CONCAT) and extended property paths (|/^/*/+/?/!) — both implemented and all tests green
metadata:
  type: project
---

Both tracks implemented 2026-06-18.

**Track A — Aggregates**
- AST: added `Aggregate::CountStar` variant
- Parser: `parse_projection_element` now handles `(?expr AS ?alias)`; `parse_function_call` produces `Expression::Aggregate(...)` for COUNT/SUM/AVG/MIN/MAX/SAMPLE/GROUP_CONCAT; COUNT(*) gives CountStar; GROUP_CONCAT parses `; separator="sep"`
- Executor: `group_by_solutions`, `eval_aggregate_value`, `eval_having_expr`, `project_aggregate_row` in `execute.rs`; implicit group when no GROUP BY but aggregates present
- Key fix: `multispace0` (not `multispace1`) before `AS` in projection alias because the expression parser eagerly consumes trailing whitespace
- Tests: 9 `spec_s11_*` tests in `tests/sparql12_suite.rs`, all green

**Track B — Extended Property Paths**
- AST: `PropertyPath` enum (Iri/Sequence/Alternative/Inverse/ZeroOrMore/OneOrMore/ZeroOrOne/NegatedSet) + `QueryComponent::PathPattern(Term, Box<PropertyPath>, Term)`
- Parser: full SPARQL path grammar (`parse_path_alternative` etc.) replacing old parse-time `__path_*` bridge variable expansion; variable predicates (`?p`) handled separately before path grammar
- Executor: `eval_path_pattern` + `transitive_closure` with BFS; include_zero flag distinguishes `*` from `+`
- Key fix: no trailing `multispace0` in `parse_path_alternative` (would eat space before object); backward BFS for `+` must not include starting node in results
- Tests: 10 `spec_s9_*` tests in `tests/sparql12_suite.rs`, all green (including 3 pre-existing sequence path tests)

**Why:** `HAVING` needs group-aware bool evaluator because `HAVING (MIN(?x) > 15)` contains aggregate sub-expressions that can't go through the regular `eval_filter` path.
