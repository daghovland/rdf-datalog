# Network Access Policy Plan

Configurable behaviour for operations that require remote HTTP fetches:
SPARQL `LOAD`, JSON-LD `@import` / external context URLs, and SPARQL `SERVICE`.

Tracked in epic [#117](https://github.com/daghovland/rdf-datalog/issues/117).

---

## Motivation

Three places in the codebase silently no-op when a remote fetch would be needed:

| Operation | Location | Current behaviour |
|---|---|---|
| `LOAD <url>` | `sparql_endpoint/src/sparql_update.rs` | silently ignored |
| JSON-LD external context URL | `jsonld_parser/src/lib.rs:106` | silently ignored |
| `SERVICE <endpoint>` (non-SILENT) | `sparql_parser/src/execute.rs:69` | returns an error |

Silent no-ops are wrong: users get no feedback that their data was not loaded. The fix
is a three-way policy that callers configure at startup.

---

## `NetworkPolicy` enum

Defined in `ingress/src/network_policy.rs` and re-exported from `ingress`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkPolicy {
    /// Remote fetches return an error with a clear explanation.
    /// This is the default — silent data loss is worse than a loud failure.
    #[default]
    Deny,

    /// Remote fetches are silently skipped (the previous behaviour).
    /// Use when consuming data that occasionally references remote URIs
    /// and you prefer graceful degradation.
    Ignore,

    /// Remote fetches are performed via HTTP.
    /// Requires active network access; exposes the server to SSRF if URLs
    /// come from untrusted input.
    /// Implementation tracked in sub-issues of [#117](https://github.com/daghovland/rdf-datalog/issues/117).
    Allow,
}
```

`ingress` is the right home: `sparql_parser` already depends on it directly;
`jsonld_parser` gets it transitively via `dag_rdf`.

---

## Error messages

### SPARQL LOAD — `Deny` → HTTP 403

```
LOAD <{url}> was rejected: remote network access is disabled.
Start the server with --network=allow to enable remote loading.
```

### JSON-LD external context — `Deny` → `JsonLdError`

```
External @context URL "{url}" was not fetched: remote network access is disabled.
Configure with --network=allow to enable external context loading.
```

### SPARQL SERVICE — `Deny` → propagated error (HTTP 400 or 500 depending on call site)

Non-SILENT SERVICE already returns an error; the message should be updated to reference the policy.
SILENT SERVICE returns empty results regardless of policy (the SPARQL spec mandates this).

---

## Signature changes

### `sparql_parser::execute`

```rust
// Before
pub fn execute(query: &Query, datastore: &Datastore) -> Result<QueryResult, String>

// After
pub fn execute(query: &Query, datastore: &Datastore, network: NetworkPolicy) -> Result<QueryResult, String>
```

All call sites pass `NetworkPolicy::Deny` initially; callers that have a `Config` pass
`config.network_policy`.

### `jsonld_parser::parse_jsonld`

```rust
// Before
pub fn parse_jsonld<R: Read>(datastore: &mut Datastore, reader: R) -> Result<(), JsonLdError>

// After
pub fn parse_jsonld<R: Read>(datastore: &mut Datastore, reader: R, network: NetworkPolicy) -> Result<(), JsonLdError>
```

### `sparql_update::apply_prepared_update`

`apply_prepared_update` already takes `store` and `ops`; add `network: NetworkPolicy`.

---

## Configuration propagation

```
main.rs --network flag
    │
    ▼
sparql_endpoint::Config { network_policy: NetworkPolicy }
    │
    ├──▶ AppState { network_policy: NetworkPolicy }
    │        │
    │        ├──▶ apply_prepared_update(..., state.network_policy)  [LOAD]
    │        └──▶ execute(query, store, state.network_policy)        [SERVICE]
    │
    └──▶ parse_jsonld(store, reader, config.network_policy)          [@import at load time]
```

---

## CLI flag

```
--network <mode>    Remote network access policy [default: deny]

    deny    Return 403 / error for any remote fetch (LOAD, @import, SERVICE).
            Recommended for production deployments unless you control all input.

    ignore  Silently skip remote fetch operations without error.
            Preserves previous behaviour.

    allow   Perform actual HTTP fetches.
            Not yet implemented — tracked in sub-issues of #117.
```

The flag applies to both `--serve` mode and direct file-processing mode.

---

## Phases

### Phase 1 — Framework ([#118](https://github.com/daghovland/rdf-datalog/issues/118))

- Add `NetworkPolicy` to `ingress`
- Add `network_policy: NetworkPolicy` to `sparql_endpoint::Config` and `AppState`
- Add `--network` CLI flag in `src/main.rs`
- Change `execute`, `parse_jsonld`, `apply_prepared_update` signatures
- Wire policy through all three call sites
- `Allow` variant returns a "not yet implemented" error pointing to sub-issues

### Phase 2 — Allow: SPARQL LOAD ([#119](https://github.com/daghovland/rdf-datalog/issues/119))

- Fetch the URL with `reqwest` (already a dependency of `sparql_endpoint`)
- Parse as Turtle/N-Triples/RDF-XML depending on `Content-Type`
- Load into the target graph (default graph or `INTO GRAPH <g>`)
- Respect timeout and follow redirects

### Phase 3 — Allow: JSON-LD @import ([#82](https://github.com/daghovland/rdf-datalog/issues/82))

- Fetch the context document with `reqwest`
- Parse as JSON-LD context
- Merge into the active context (existing `@import` spec behaviour)

### Phase 4 — Allow: SPARQL SERVICE ([#51](https://github.com/daghovland/rdf-datalog/issues/51))

- Federated queries: dispatch sub-query to remote SPARQL endpoint
- Already tracked as its own epic

---

## Security notes

`Allow` mode should document the SSRF risk clearly. Future hardening (allowlist of
permitted domains, `--network-allowlist` flag) is out of scope here but should be
mentioned in the `--network=allow` help text.
