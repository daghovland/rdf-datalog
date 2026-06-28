# RML SQL Plan: SQL/JDBC sources for `rml`

> Tracked under [#26 RML: SQL/JDBC LogicalSource support](https://github.com/daghovland/rdf-datalog/issues/26).
> No tests, no stub code, no implementation yet. The next phase is red-phase
> stub tests (`#[ignore]`d) for user review — do not start that phase without
> separate sign-off on this design, especially the join-pushdown approach in
> "Efficient joins" below, since that is the part most likely to need
> revision before code is written.

## Goal

Add a SQL `LogicalSource` to the `rml` crate so mappings can pull rows
directly from a relational database (SQLite first, PostgreSQL later) instead
of requiring an export to CSV/JSON/XML first. This is the last major
RML/R2RML source type not yet supported — see `docs/user/rml-mapping.md`'s
"Not yet implemented" list and `PIPELINE_BACKLOG.md`.

**The central design constraint, per explicit instruction: joins must be
efficient.** `RML_JOIN_PLAN.md` already designs a correct but
Rust-side, in-memory hash join for `rml:joinCondition` (materialize the
parent source fully, index it by join key, stream the child source, probe).
That is fine when the parent is a CSV file — but if the parent is a SQL
table, materializing it row-by-row through this crate's generic interface
defeats the entire reason to use a database: the database already has
indexes, statistics, and a query planner built to do joins fast, and SQL
tables are frequently far larger than the CSV/JSON files this crate has
targeted so far. This plan's central decision (see "Efficient joins") is to
**push the join down into the database** whenever both sides of a
`rml:joinCondition` are SQL sources on the same connection, and only fall
back to `RML_JOIN_PLAN.md`'s Rust-side hash join when they aren't.

## Spec references

- R2RML §4 Logical Tables (`rr:tableName`, `rr:sqlQuery`, `rr:sqlVersion`) —
  <https://www.w3.org/TR/r2rml/#logical-tables> — RML's `rml:LogicalSource`
  is a generalization of R2RML's logical table; this crate already borrows
  R2RML's join vocabulary (`rr:joinCondition`/`child`/`parent`, ported to the
  W3C `rml:` namespace, per `RML_JOIN_PLAN.md`) and should borrow the SQL
  source vocabulary the same way: `rml:tableName`, `rml:sqlQuery`.
- R2RML §10 Join — <https://www.w3.org/TR/r2rml/#joins> — this plan builds on
  `RML_JOIN_PLAN.md`'s join design rather than re-deriving it; read that plan
  first.
- SQLite via `rusqlite` — <https://docs.rs/rusqlite/>
- PostgreSQL sync driver `postgres` — <https://docs.rs/postgres/>

## Scope

**In scope (this plan):**
- `rml:tableName` (whole-table source) and `rml:sqlQuery` (arbitrary SELECT
  as source) as alternative `LogicalSource` forms
- SQLite as the first backend (`rusqlite`, bundled — no system `libsqlite3`
  dependency, consistent with the project's established preference for
  avoiding system dependencies, e.g. the pure-Rust `zeromq` choice in
  `JUPYTER_KERNEL_PLAN.md`)
- The two-tier join execution strategy described below
- Credential handling that doesn't require committing secrets to the
  mapping file

**Out of scope (deferred):**
- PostgreSQL backend itself (the design accommodates it — `SqlConnection` is
  an enum — but only SQLite is implemented in this plan's phasing; Postgres
  is phase 5, separately scoped)
- MySQL, ODBC, or any other backend
- `sqlx`/async drivers — see "Why synchronous" below
- FunctionMap (FNML) — separate, unplanned gap
- Joins across more than two sources, or joins where the same `ObjectMap`
  has `rml:joinCondition`s pointing at more than one parent
- Query result caching/connection pooling — single connection per mapping
  run is sufficient for now

## Why synchronous

`rml/Cargo.toml` has no async/tokio dependency today (confirmed: `ingress`,
`dag-rdf`, `turtle`, `thiserror`, `csv`, `serde_json`, `serde_json_path`,
`sxd-document`, `sxd-xpath` — all synchronous). `apply_rml_mapping` is called
synchronously and eagerly from the CLI, the Jupyter kernel
(`dagalog-kernel/src/cell/rml.rs`), and — via a blocking call — the async
`sparql_endpoint` HTTP handlers (`POST /{name}/rml`, `POST /rml/map`).
Introducing `sqlx` (async-only) would force either an async `rml` crate (a
much bigger change rippling through every caller) or a `block_on` shim at
every call site. **Use synchronous drivers**: `rusqlite` for SQLite,
the sync `postgres` crate for PostgreSQL later. Both expose blocking,
cursor-based row iteration, which is exactly the shape `SourceRow`/
`Iterator<Item = Result<RawRow, RmlError>>` already needs.

## AST changes (`rml/src/ast.rs`)

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum LogicalSourceRef {
    File(PathBuf),
    Sql(SqlSourceRef),               // new
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlSourceRef {
    pub connection: SqlConnection,
    pub query: SqlQuery,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlConnection {
    /// rml:source holds a filesystem path to a SQLite database file,
    /// resolved relative to the mapping's base_dir like File(PathBuf) is.
    Sqlite(PathBuf),
    // Postgres(String) — added in phase 5; DSN, env-var-interpolated (see
    // "Credentials" below), not a literal connection string in the mapping.
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlQuery {
    /// rml:tableName "people" — whole-table scan, `SELECT * FROM people`.
    Table(String),
    /// rml:sqlQuery "SELECT id, name FROM people WHERE active = 1"
    Query(String),
}
```

`ReferenceFormulation` gains no new variant — SQL rows are column-keyed like
CSV rows, so `rml:reference "column_name"` and template placeholders
(`{column_name}`) work exactly like `ReferenceFormulation::Csv` does today.
A SQL `LogicalSource` simply pairs `LogicalSourceRef::Sql(..)` with
`ReferenceFormulation::Csv` (reusing it rather than adding a vacuous `Sql`
variant whose behavior would be identical).

This makes `LogicalSourceRef` a two-variant enum, which means the one
irrefutable `let LogicalSourceRef::File(rel_path) = &scan.source;` in
`engine.rs::execute_plan` (today's only match site, since the enum currently
has a single variant) **must become a real `match`** — noted here so it
isn't missed in the red phase's "stub enough to compile" step.

## `plan.rs` changes

None beyond what `RML_JOIN_PLAN.md` already proposes (pluralizing
`JoinCondition` → `Vec<JoinCondition>`). This plan adds one more
`JoinAlgorithm` variant:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum JoinAlgorithm {
    HashJoin,      // existing — RML_JOIN_PLAN.md's Rust-side hash join
    SqlPushdown,   // new — synthesize one SQL query, let the DB join
}
```

`LogicalScan` also needs no change beyond carrying `LogicalSourceRef::Sql`
through the existing `source` field — it's already typed as
`LogicalSourceRef`.

## Efficient joins — the two-tier strategy

This is the part of the design the user explicitly flagged as critical, so
it's spelled out in full before any of the surrounding plumbing.

### Tier 1: SQL pushdown (same-connection SQL-to-SQL joins)

When `translate.rs` builds a `LogicalJoin` for an `ObjectMap` with
`parent_triples_map` set (per `RML_JOIN_PLAN.md`'s translate-phase sketch),
check: is the child `TriplesMap`'s logical source `LogicalSourceRef::Sql`,
is the parent's too, and are their `SqlConnection`s equal (same SQLite file
path, or later, same Postgres DSN)? If yes, set
`algorithm: JoinAlgorithm::SqlPushdown` instead of `HashJoin`.

At execution time (`engine.rs`), a `SqlPushdown` join does **not** open two
separate scans and join them in Rust. Instead it synthesizes one SQL query
that performs the join in the database:

```sql
SELECT
    c.<col> AS child_<col>, ...   -- every column the child side's
                                    -- attributes need
    p.<col> AS parent_<col>, ...  -- every column the parent's subject-map
                                    -- term_map needs, for the join object
FROM (<child base query>) AS c
JOIN (<parent base query>) AS p
  ON c.<child_col1> = p.<parent_col1>
 AND c.<child_col2> = p.<parent_col2>   -- one AND clause per JoinCondition
 ...
```

Where `<child base query>` / `<parent base query>` is `SELECT * FROM
<table>` for `SqlQuery::Table` or the literal text for `SqlQuery::Query`,
wrapped as a subquery so both forms compose uniformly.

**Column-prefixing solves the exact correctness hazard `RML_JOIN_PLAN.md`
flags** (child and parent tables both having a column named e.g. `ID`):
every selected column is aliased `child_<col>`/`parent_<col>` in the
synthesized query, so the row returned to `engine.rs` is a single flat
`RawRow` with disambiguated keys, and the existing per-attribute evaluation
logic only ever reads `child_<col>` for child-side attributes (Subject,
Predicate, Graph, non-join Object) and `parent_<col>` for the join Object's
term-map evaluation — no separate "child row" and "parent row" bookkeeping
needed at all for this tier, unlike the hash-join tier where keeping them
separate is the documented invariant.

**This is a genuine architectural divergence from the CSV/JSON/XML source
readers**, which all collect their entire input into a `Vec` up front
(`CsvSource::collect_rows`, etc.). For SQL, the corresponding move is to
**not** materialize anything in Rust beyond one row at a time: prepare the
synthesized statement once, then stream rows via the driver's cursor
(`rusqlite::Statement::query` + `Rows::next()`, or `postgres::Client::query_raw`
later). The database — not this crate — does the join, using whatever
indexes and query plan it has. This is the whole point of pushing down:
a join over a million-row parent table with an index on the join column
costs the database an index lookup per child row, not a full Rust-side
table scan and hash build.

Because the join happens at the SQL level, `LogicalPlan::Join`'s `left`/
`right` fields are not separately scanned for this algorithm — the
`LogicalJoin` node carries enough information (`SqlConnection`, both
`SqlQuery`s, the `Vec<JoinCondition>`) for `engine.rs` to synthesize the one
query without walking `left`/`right` as independent sub-plans. (For
`HashJoin`, `left`/`right` are still scanned independently, per
`RML_JOIN_PLAN.md`.)

**Index guidance, not enforcement**: this plan does not attempt to create
indexes automatically — that would be surprising, write-coupled behavior for
what should be a read mapping operation. `docs/user/rml-mapping.md`'s SQL
section (written once this feature ships) should simply note: *"for large
parent tables, add an index on the parent join column — RML's generated
join query relies on it the same way any other SQL JOIN would."*

### Tier 2: fallback (cross-connection or cross-source-type joins)

When the child and parent aren't both SQL on the same connection — e.g. one
is SQL and the other CSV/JSON/XML, or they're SQL on two different database
connections — `algorithm` stays `JoinAlgorithm::HashJoin` and execution is
exactly `RML_JOIN_PLAN.md`'s existing design: materialize the parent fully
(via whatever its source type's `rows()` iterator yields — for a SQL parent
in this position, that means running its base query and collecting all
rows, no pushdown possible since there's no shared connection to push into),
index by join key, stream the child, probe, and evaluate child/parent
attributes against their own row keeping the two separate.

`SqlSource` (the new plain, non-join scan type) therefore still needs a
`rows()` method returning `Box<dyn Iterator<Item = Result<RawRow, RmlError>>>`
that satisfies `SourceRow`/`RawRow`'s existing contract, used both directly
(plain SQL scan, no join) and as the materialization path for tier 2. It
*can* stream lazily even there — only the join key/value pairs need to be
buffered, not full `Vec<RawRow>` row clones — but that's a tier-2 efficiency
nicety, not the headline efficiency story. The headline is tier 1: no
materialization, full pushdown.

### How `translate.rs` decides between tiers

```rust
fn choose_join_algorithm(child_source: &LogicalSourceRef, parent_source: &LogicalSourceRef) -> JoinAlgorithm {
    match (child_source, parent_source) {
        (LogicalSourceRef::Sql(c), LogicalSourceRef::Sql(p)) if c.connection == p.connection => {
            JoinAlgorithm::SqlPushdown
        }
        _ => JoinAlgorithm::HashJoin,
    }
}
```

This is a pure, easily-unit-tested function — the red phase should stub it
and write `#[ignore]`d tests for exactly the three cases (same-connection
SQL/SQL, cross-connection SQL/SQL, SQL/CSV) before any engine-side execution
exists.

## Credentials

Mapping files (`.ttl`) are ordinary source-controlled text. Embedding a
plaintext database password in `rml:source` would mean every mapping that
touches Postgres leaks a credential into git history. For SQLite (phase 1)
this doesn't arise — `rml:source` is just a filesystem path, no credential.
For Postgres (phase 5): require the DSN to be given as an environment
variable reference rather than a literal value, e.g.
`rml:source "${DATABASE_URL}"`, resolved by the loader at mapping-load time
by looking up `DATABASE_URL` in the process environment and erroring
(`RmlError::MissingProperty`-shaped, naming the env var) if unset. No
literal `postgres://user:pass@host/db` form should be accepted at all for
non-`${VAR}` values — fail loudly rather than silently support the insecure
path "for convenience."

## Test plan

All tests use **SQLite in-memory databases** (`rusqlite::Connection::open_in_memory()`),
seeded by a short setup script inline in the test, mirroring this project's
established preference for CI-friendly tests with no external service
dependency (the existing `tests/performance.rs` pattern of `#[ignore]`d,
download-requiring tests is for genuinely expensive/external cases — SQLite
in-memory is neither, so these tests should run by default once green).

Planned fixture/test shape, building directly on `RML_JOIN_PLAN.md`'s
`rmltc0009a_join` CSV fixtures (same `student`/`sport` data, re-expressed as
SQL tables, so the expected triples are identical and easy to cross-check):

- `rml/tests/sql_tests.rs` (unit-level, no end-to-end mapping):
  - plain `rml:tableName` scan of an in-memory SQLite table yields the
    expected `RawRow`s
  - `rml:sqlQuery` (arbitrary SELECT) scan yields the expected `RawRow`s
  - missing table / malformed SQL yields `RmlError`, not a panic
- `rml/tests/sql_join_tests.rs` (unit-level, `choose_join_algorithm`):
  - same-connection SQL/SQL pair → `JoinAlgorithm::SqlPushdown`
  - cross-connection SQL/SQL pair (two different in-memory connections) →
    `JoinAlgorithm::HashJoin`
  - SQL child / CSV parent → `JoinAlgorithm::HashJoin`
- `rml/tests/sql_end_to_end.rs` (full `apply_rml_mapping` against a mapping
  pointing `rml:source` at a SQLite file fixture committed under
  `rml/tests/fixtures/rmltc_sql_join/`, containing a tiny pre-built
  `.sqlite` file with `student`/`sport` tables — committing a binary fixture
  is consistent with how `dagalog_intro.ipynb` and other binary-ish fixtures
  already live in this repo):
  - same-connection join (tier 1) produces the correct join triples
  - a second mapping pointing the parent at a CSV file instead (tier 2)
    produces the *same* triples via the fallback path — proving tier 1 and
    tier 2 are observably equivalent in output, only differing in how the
    join executes

A later, explicitly-`#[ignore]`d performance test (added only once both
tiers are green, not in the red phase) is worth keeping in mind: seed a
larger SQLite table (tens of thousands of rows) with an index on the join
column, and assert the pushdown path completes within a generous bound
while a deliberately-forced `HashJoin` over the same data takes
measurably longer — making the "efficient joins" claim verifiable rather
than just asserted. Not specified further here; revisit once tiers 1 and 2
are both implemented and correct.

## Dependencies

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
```

`features = ["bundled"]` compiles SQLite from source as part of the Rust
build, avoiding a `libsqlite3-dev` system dependency — the same reasoning
`JUPYTER_KERNEL_PLAN.md` used to pick the pure-Rust `zeromq` crate over
`libzmq`-binding alternatives.

Postgres (phase 5, not phase 1): `postgres = "0.19"` (sync), deferred until
SQLite + the join tiers are proven.

## Phasing

1. **`SqlSource` (SQLite, `rusqlite`, bundled)** — `rml:tableName` whole-table
   scan only, no joins. `LogicalSourceRef::Sql`, `SqlConnection::Sqlite`,
   `SqlQuery::Table`. Proves the basic plumbing and the `engine.rs` match
   exhaustiveness change.
2. **`rml:sqlQuery`** — arbitrary SELECT as source (`SqlQuery::Query`).
3. **Join tier 1 (SQL pushdown)** — `choose_join_algorithm`, query
   synthesis with column-prefixing, `JoinAlgorithm::SqlPushdown` execution.
   This is the phase that delivers the "efficient joins" requirement.
4. **Join tier 2 (fallback wiring)** — connect `JoinAlgorithm::HashJoin`
   cases (SQL involved on at least one side, but not eligible for
   pushdown) into `RML_JOIN_PLAN.md`'s existing hash-join engine design.
   Depends on `RML_JOIN_PLAN.md` being implemented first, or being
   implemented alongside this phase.
5. **PostgreSQL backend** — `SqlConnection::Postgres`, `${VAR}`-only DSN
   resolution, sync `postgres` crate. Independent of phases 1–4's design
   (just a second `SqlConnection` variant and driver implementation behind
   the same `SourceRow` contract).

## Dependencies on other plans

- **Requires `RML_JOIN_PLAN.md`'s AST work** (`JoinConditionRef`,
  `ObjectMap.join_conditions`, pluralized `LogicalJoin.conditions`) as a
  prerequisite — this plan's tier 1/tier 2 split is additive on top of that
  join-detection and hash-join design, not a replacement for it. Phase 3/4
  above should not start before `RML_JOIN_PLAN.md`'s own green phase (or at
  minimum its translate.rs/engine.rs join-construction code) exists, since
  tier 2 calls into it directly and tier 1's `choose_join_algorithm` slots
  into the same `translate.rs` decision point `RML_JOIN_PLAN.md` sketches.
