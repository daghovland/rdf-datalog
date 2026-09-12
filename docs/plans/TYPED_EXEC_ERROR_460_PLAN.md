# Typed error enum for the SPARQL executor and notebook kernel (#460)

Part of epic #453.

## Problem

`sparql_parser::execute` (the query evaluator: `execute`/`execute_with_base`
and all its internal helpers across `sparql_parser/src/execute/*.rs` and
`sparql_parser/src/deadline.rs`) and `dagalog-kernel::sockets::dispatch_cell`
both use `Result<_, String>`. Callers that need to distinguish failure modes
(e.g. `sparql_endpoint::query::query_execution_error_response`, which
special-cases the #372 cooperative timeout to return HTTP 503 instead of
500) currently do it by `message.contains("exceeded the configured
timeout")` — a fragile string match instead of a `match` on a variant.

## Scope

Inventory of every place `sparql_parser`'s executor actually *constructs* an
error (as opposed to merely propagating one via `?`):

- `sparql_parser/src/execute/mod.rs::execute_inner` — non-SILENT `SERVICE`
  under `NetworkPolicy::Deny` (endpoint rejected) and under
  `NetworkPolicy::Allow`/`AllowList` (not yet implemented).
- `sparql_parser/src/deadline.rs::Deadline::check` — the #372 cooperative
  query timeout.

Every other `Result<_, String>` return type across `execute/*.rs` exists
purely to propagate one of the above via `?` (mainly `deadline.check()?` at
loop-iteration boundaries). So the fix is: introduce one small
`sparql_parser::ExecError` enum with three variants (`ServiceDenied`,
`ServiceNotImplemented`, `Timeout`), thread it through every
`Result<_, String>` in the executor, and update in-workspace callers.

For the notebook kernel, `dispatch_cell` itself constructs errors in three
distinguishable shapes: a rejected unsafe path (#85 path-traversal check), a
filesystem I/O failure opening a referenced file, and a message forwarded
from whichever cell-type subsystem ran (turtle/rml/manchester/shacl/ottr/
datalog/reasoning/sparql — each already collapses to its own `String`
inside `dagalog-kernel/src/cell/*.rs`, which is out of scope here: fully
typing every one of those subsystems' errors is a much larger, separate
effort). `CellError` gets three matching variants: `UnsafePath`, `Io`,
`Execution(String)` (a deliberate, intentionally-scoped catch-all for the
subsystem message).

## Non-goals (follow-ups filed separately if found)

- Typing the individual `dagalog-kernel/src/cell/*.rs` subsystem errors
  (turtle/rml/manchester/shacl/ottr/datalog) themselves — out of scope per
  #460's stated scope ("sparql_parser's executor and the kernel's
  cell-dispatch path").
- Typing errors in `sparql_parser`'s *parser* (`nom`-based, `IResult`) —
  #460 is about the *executor*, not the parser.

## Plan

1. `sparql_parser/src/error.rs`: `ExecError` enum + `Display` + `std::error::Error`.
2. Thread `ExecError` through `deadline.rs` and every `execute/*.rs` module
   (mechanical: `Result<_, String>` → `Result<_, ExecError>`).
3. Update in-workspace callers: `shacl`, `sparql_endpoint`, `src/lib.rs`,
   `vqs_index`, tests. `sparql_endpoint::query::query_execution_error_response`
   changes from `message.contains(...)` to a `match` on `ExecError::Timeout`.
4. `dagalog-kernel/src/cell/mod.rs`: `CellError` enum (`UnsafePath`, `Io`,
   `Execution`) + `Display` + `std::error::Error`. `dispatch_cell` and
   `handle_execute` updated.
5. Tests: variant-level assertions replacing `.contains(...)` string checks
   in `tests/network_policy.rs`, `sparql_parser/tests/query_timeout_tests.rs`,
   and new unit tests for `ExecError`/`CellError` Display text.

Refactor with an observable type-signature change but no behavior change —
one-pass TDD (tests alongside implementation) per this repo's CLAUDE.md
"pure refactors" exception.
