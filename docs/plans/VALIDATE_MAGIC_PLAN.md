# Wiring `%%validate` to the `shacl` crate

> Status: RED PHASE — tests written and ignored, awaiting review before implementation.

## Problem

`dagalog-kernel`'s `%%validate <path>` magic is parsed correctly by
`detect_cell_type` (`CellType::Validate(PathBuf)`), but `dispatch_cell` in
`sockets.rs:226` stubs it out:

```rust
CellType::Validate(_path) => Err("%%validate not yet implemented".to_string()),
```

`JUPYTER_KERNEL_PLAN.md`'s status banner incorrectly claims this is done. It
isn't. The `shacl` crate itself is fully implemented (`shacl::validate(data,
shapes) -> Result<ValidationReport, String>`, used today by
`sparql_endpoint/src/shacl_endpoint.rs`'s `POST /{name}/shacl` route) but
`dagalog-kernel` doesn't depend on it at all.

## Approach

Mirror the existing `cell/rml.rs` / `cell/turtle.rs` pattern: a new
`dagalog-kernel/src/cell/shacl.rs` module with one function:

```rust
pub fn execute_validate(ds: &Datastore, shapes_path: &Path) -> Result<String, String>
```

This:
1. Opens `shapes_path` and parses it as Turtle into a fresh `Datastore`
   (same pattern as `shacl_endpoint.rs`'s `dataset_shacl_post`).
2. Calls `shacl::validate(ds, &shapes_store)`.
3. Formats the result as a plain-text status line, consistent with the
   existing `Loaded N triples.` / `Applied 1 rule.` convention:
   - Conforms: `"Conforms. 0 violation(s)."`
   - Violations: `"N violation(s)."`

Wiring:
- Add `shacl = { path = "../shacl" }` to `dagalog-kernel/Cargo.toml`.
- Add `pub mod shacl;` to `cell/mod.rs`.
- Replace the `CellType::Validate` stub arm in `sockets.rs`'s
  `dispatch_cell` with `execute_validate(ds, &path).map(CellOutput::Stream)`.

No changes to `detect_cell_type` or `CellType` — both already correct.

## Output format note

Unlike `%%load`/`%%rml`/`%%reason`, a SHACL report has a natural rich
representation (`shacl::report_to_turtle`). This plan deliberately keeps the
kernel output to the same plain `Stream` text as every other magic, to match
existing convention and avoid scope creep. A follow-up could send the full
Turtle report as a `text/turtle` rich output if a future request asks for it.

## Test plan

### Unit tests — `dagalog-kernel/src/cell/shacl.rs`

1. `test_validate_conforms` — load
   `tests/testdata/shacl_s2_target_subjects_data.ttl` inline into a
   `Datastore`, call `execute_validate` against
   `tests/testdata/shacl_s2_target_subjects_shapes.ttl`. Expect
   `Ok("Conforms. 0 violation(s).")`.
2. `test_validate_reports_violations` — load
   `tests/testdata/shacl_s1_intro_data.ttl` inline, validate against
   `tests/testdata/shacl_s1_intro_shapes.ttl`. Expect
   `Ok("4 violation(s).")` (per the existing `shacl_suite.rs` spec test
   for this exact fixture pair).
3. `test_validate_missing_shapes_file_returns_error` — nonexistent path →
   `Err`.
4. `test_validate_invalid_turtle_shapes_returns_error` — shapes file with
   malformed Turtle → `Err`.

### Integration tests — `dagalog-kernel/tests/notebook_e2e.rs`

5. `test_validate_cell_reports_violations` — via `KernelHarness`: execute
   `%%load tests/testdata/shacl_s1_intro_data.ttl`, then
   `%%validate tests/testdata/shacl_s1_intro_shapes.ttl`. Expect
   `stream == Some("4 violation(s).")`.
6. `test_validate_cell_reports_conforms` — via `KernelHarness`: execute
   `%%load tests/testdata/shacl_s2_target_subjects_data.ttl`, then
   `%%validate tests/testdata/shacl_s2_target_subjects_shapes.ttl`. Expect
   `stream == Some("Conforms. 0 violation(s).")`.

All 6 tests will be written `#[ignore]`d with just enough stub code
(`execute_validate` returning `unimplemented!()`, or in the integration
case relying on the still-stubbed dispatch) to compile, per CLAUDE.md TDD
phasing. Implementation proceeds test-by-test after review, easiest first:
3 → 4 → 1 → 2 → 5 → 6.
