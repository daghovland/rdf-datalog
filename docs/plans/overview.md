# Known Lacking Features and Bugs

This document tracks known limitations, bugs, and planned features that have not
yet been implemented.  Each item links to a more detailed plan document where one
exists.

Last updated: 2026-06-15.

---

## Bugs

### CONSTRUCT WHERE does not recurse into OPTIONAL / UNION
**File**: `sparql_parser/src/execute.rs` — `collect_bgps_from_components`  
**Plan**: [`docs/plans/construct-where-recursion.md`](construct-where-recursion.md)  
**Tests**: `sparql_parser/tests/parser_tests.rs` — `construct_where_with_optional_*` (ignored)  
**Impact**: CONSTRUCT WHERE queries that use OPTIONAL or UNION in the WHERE clause
produce an empty result instead of the expected triples.  CONSTRUCT with an explicit
template is unaffected.

### sh:and only handles sh:minCount constraints
**File**: `shacl/src/translate.rs` — `translate_shape`  
**Impact**: SHACL `sh:and` with inner shapes containing constraints other than
`sh:minCount` (e.g. `sh:datatype`, `sh:pattern`, `sh:nodeKind`) generates no
validation rules for those constraints.  Validation silently under-reports violations.

---

## Lacking features

### Changelog compaction
**Plan**: [`docs/plans/changelog-compaction.md`](changelog-compaction.md)  
**Impact**: The redb changelog grows without bound; startup replay time grows
linearly with total historical mutations.  A `POST /$/compact` admin endpoint is
planned to atomically replace the log with a minimal snapshot.

### SPARQL Aggregates (GROUP BY, COUNT, SUM, AVG, …)
**Status**: ✓ Implemented  
**Files**: `sparql_parser/src/ast.rs`, `sparql_parser/src/lib.rs`, `sparql_parser/src/execute.rs`  
**What was added**: `CountStar` AST variant; `(?expr AS ?alias)` projection parsing; aggregate function parsing (COUNT/SUM/AVG/MIN/MAX/SAMPLE/GROUP_CONCAT) in `parse_function_call`; `group_by_solutions`, `eval_aggregate_value`, `eval_having_expr`, `project_aggregate_row` in executor  
**Tests**: `tests/sparql12_suite.rs` — `spec_s11_*` (9 tests, all green); `tests/api_integration.rs` — `sparql_aggregate_sum_group_by` (green); `tests/w3c_sparql11_suite.rs` — `w3c_sparql11_aggregates`, `w3c_sparql11_grouping` (still ignored — require W3C test data download)

### SPARQL Property Paths (beyond `/`)
**Status**: ✓ Implemented  
**Files**: `sparql_parser/src/ast.rs` (`PropertyPath` enum, `QueryComponent::PathPattern`), `sparql_parser/src/lib.rs` (full path grammar parser), `sparql_parser/src/execute.rs` (`eval_path_pattern`, `transitive_closure`)  
**What was added**: `PropertyPath` AST enum with Iri/Sequence/Alternative/Inverse/ZeroOrMore/OneOrMore/ZeroOrOne/NegatedSet; full SPARQL path grammar parser; runtime path evaluation with BFS transitive closure for `*` and `+`  
**Tests**: `tests/sparql12_suite.rs` — all 10 `spec_s9_*` tests pass including the 7 new ones; `tests/w3c_sparql11_suite.rs` — `w3c_sparql11_property_path` (still ignored — requires W3C test data download)

### SPARQL SERVICE (federated queries)
**Impact**: `SERVICE <endpoint> { … }` is not parsed.

### SPARQL INSERT/DELETE with WHERE clause (non-DATA forms)
**Files**: `sparql_endpoint/src/sparql_update.rs`  
**Impact**: Only `INSERT DATA` and `DELETE DATA` are implemented.  The
pattern-matching `INSERT { … } WHERE { … }` and `DELETE { … } WHERE { … }`
forms are not supported.

### JSON-LD External Context Fetching (`@import`)
**Files**: `jsonld_parser/src/`  
**Impact**: `@import` in a JSON-LD context is not implemented.  Contexts that
reference external URLs via `@import` will fail silently.

### OWL Manchester Syntax
**Files**: `manchester_parser/src/`  
**Impact**: The Manchester syntax parser crate is a stub.  No OWL Manchester
syntax is parsed.

### SPARQL DESCRIBE
**Files**: `sparql_parser/src/`  
**Impact**: `DESCRIBE` queries are not parsed.

### VoID Dataset Description
**Plan**: [`docs/architecture/PROTOCOLS.md`](../architecture/PROTOCOLS.md)  
**Impact**: `GET /.well-known/void` is not implemented.

### Changelog compaction endpoint authentication
**Impact**: Once the compaction endpoint is implemented, it must be guarded by
the Admin permission (like `POST /$/datasets`).

### Automatic (background) changelog compaction
**Impact**: The planned compaction is manually triggered.  Automatic background
compaction (e.g. when the log exceeds a configurable size threshold) is a future
extension.

### OWL ALC (tableau reasoning)
**Plan**: [`docs/architecture/PLAN.md`](../architecture/PLAN.md)  
**Impact**: The ALC tableau reasoner is deferred.  Only OWL 2 RL (datalog-
expressible) entailment is supported.

---

## Test coverage gaps

- No end-to-end persistence tests that crash + replay
- No load / performance tests for SHACL validation at scale
- No negative SHACL tests (currently only positive validation tests exist)
- No tests for concurrent SPARQL Update requests under write contention
