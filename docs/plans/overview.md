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
**Files**: `sparql_parser/src/` — parser and executor  
**Impact**: Any SPARQL query using aggregate functions fails to parse.  This blocks
analytics queries that use COUNT, SUM, MIN, MAX, AVG, GROUP_CONCAT.

### SPARQL Property Paths (beyond `/`)
**Files**: `sparql_parser/src/`  
**Plan**: [`docs/plans/QUERY_BUILDER_PLAN.md`](QUERY_BUILDER_PLAN.md)  
**Impact**: Only the sequence path operator (`/`) is supported.  `*`, `+`, `?`,
`|`, `^`, `!`, and `<iri>` paths are not yet implemented.

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
