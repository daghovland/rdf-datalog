# Known Lacking Features and Bugs

This document tracks known limitations, bugs, and planned features that have not
yet been implemented.  Each item links to a more detailed plan document where one
exists.

Last updated: 2026-06-27.

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
**Issue**: [#72 Persistence changelog compaction endpoint](https://github.com/daghovland/rdf-datalog/issues/72)  
**Plan**: [`docs/plans/changelog-compaction.md`](changelog-compaction.md)  
**Impact**: The redb changelog grows without bound; startup replay time grows
linearly with total historical mutations.  A `POST /$/compact` admin endpoint is
planned to atomically replace the log with a minimal snapshot.

### SPARQL missing scalar builtins (BNODE, ENCODE_FOR_URI, REPLACE, date/time, hash functions, UUID)
**Issue**: [#52 SPARQL missing scalar built-in functions](https://github.com/daghovland/rdf-datalog/issues/52)  
**Plan**: [`docs/plans/SPARQL_MISSING_FEATURES_PLAN.md`](SPARQL_MISSING_FEATURES_PLAN.md)  
**Impact**: Several SPARQL 1.1 scalar functions are not yet implemented, including
`BNODE()`, `ENCODE_FOR_URI()`, `REPLACE()`, `RAND()`, `NOW()`, date/time extraction
functions (`YEAR`, `MONTH`, `DAY`, `HOURS`, `MINUTES`, `SECONDS`, `TZ`), hash
functions (`MD5`, `SHA1`, `SHA256`, `SHA384`, `SHA512`), and UUID functions.

### SPARQL SERVICE (federated queries)
**Issue**: [#51 SPARQL SERVICE / federated queries](https://github.com/daghovland/rdf-datalog/issues/51)  
**Impact**: `SERVICE <endpoint> { … }` is not parsed.

### JSON-LD External Context Fetching (`@import`)
**Issue**: [#82 JSON-LD @import / external context URL fetching](https://github.com/daghovland/rdf-datalog/issues/82)  
**Files**: `jsonld_parser/src/`  
**Impact**: `@import` in a JSON-LD context is not implemented.  Contexts that
reference external URLs via `@import` will fail silently.

### OWL Manchester Syntax
**Files**: `manchester_parser/src/`  
**Impact**: The Manchester syntax parser crate is a stub.  No OWL Manchester
syntax is parsed.

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
