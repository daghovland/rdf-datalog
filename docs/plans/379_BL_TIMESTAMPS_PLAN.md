# Plan: `bl:createdAt`/`bl:updatedAt`/`bl:closedAt` on `bl:WorkItem`

Issue: [#379](https://github.com/daghovland/rdf-datalog/issues/379). Part of
the dagalog-on-dagalog backlog-mirror epic ([#282](https://github.com/daghovland/rdf-datalog/issues/282)).
Branch: `feat/379-bl-timestamps`.

## Problem

`backlog/ontology/vocabulary.ttl` has no timestamp properties on
`bl:WorkItem`, even though GitHub's Issues API already returns
`created_at`/`updated_at`/`closed_at` for every issue and PR, and the
`backlog` crate's loader already consumes that same API response
(`backlog/src/github.rs`'s `GhCliSource::list_issues`, via
`--jq '.[]'` — the whole issue object, so these fields are already present
in the raw JSON, just not deserialized). This blocks any time-ordered view
(activity feed, "recently updated", staleness) of the backlog mirror.

## Scope

1. **Vocabulary** (`backlog/ontology/vocabulary.ttl`): add
   `bl:createdAt`, `bl:updatedAt`, `bl:closedAt` —
   `owl:DatatypeProperty`, domain `bl:WorkItem`, range `xsd:dateTime`.
   `bl:closedAt` is optional (open items have none) — matching the existing
   pattern for optional properties in this vocabulary (e.g.
   `bl:touchesFile`, `bl:parent_issue_url`-derived `bl:subIssueOf`): no
   `sh:minCount` is added for it anywhere. `bl:createdAt`/`bl:updatedAt` are
   *not* added to `bl:WorkItemRequiredFieldsShape` either — that shape is a
   pre-existing baseline-completeness check that this issue doesn't ask to
   extend, and the fixture-derived example data may not be exhaustive
   enough for every legacy item to have both.

2. **Loader**:
   - `backlog/src/model.rs`'s `RawIssue`: add `created_at: String`,
     `updated_at: String`, `closed_at: Option<String>` fields (GitHub
     returns these as ISO-8601 `xsd:dateTime`-compatible strings, or `null`
     for `closed_at` on an open issue). No `--jq` change needed in
     `backlog/src/github.rs` — `.[]` already passes the whole object
     through; only the (de)serialization target needed the new fields.
   - `backlog/src/loader.rs`'s `load_issues`: emit `bl:createdAt` and
     `bl:updatedAt` for every issue/PR (`add_datetime`, a new helper
     alongside `add_integer`/`add_string`, parsing the ISO-8601 string into
     `RdfLiteral::DateTimeLiteral`), and `bl:closedAt` only when
     `closed_at` is `Some`.

3. **Fixture**: `backlog/tests/fixtures/repo_slice.ndjson` is hand-authored
   (not a raw `gh api` capture) and currently omits `created_at`/
   `updated_at`/`closed_at` entirely. Add realistic values to each line
   (closed items get a `closed_at`, open items get `closed_at: null`).

4. **Tests** (`backlog/tests/loader_test.rs`, red before green): a new test
   asserting `bl:createdAt`/`bl:updatedAt` triples exist with the right
   literal value for a known fixture issue, and that `bl:closedAt` is
   present for a closed item and absent for an open one.

5. **Snapshot regeneration**: once the loader change is green, regenerate
   `backlog/examples/snapshot.ttl` via
   `cargo run -p backlog --bin backlog-regenerate` (hits live `gh api`).
   Expected to produce a large diff (three new triples per work item) —
   that's correct, not a mistake.

## Out of scope

- Deriving `bl:status bl:Done`/`bl:InProgress` from timestamps — not asked
  for here, and already flagged as future work in
  `docs/plans/BACKLOG_GITHUB_LOADER_PLAN.md`.
- Any SPARQL/query-side "recently updated" view — this issue is about the
  data being present, not about a consuming query/dashboard.
