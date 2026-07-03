# Persistence Plan

> Progress tracking: [#34 Persistence epic](https://github.com/daghovland/rdf-datalog/issues/34)

Two orthogonal goals live here:

1. **Durable transactional storage** — when the server returns `200 OK` to a
   mutating HTTP request, the written data is guaranteed to survive a crash or restart.
   Tracked by [#34](https://github.com/daghovland/rdf-datalog/issues/34).
2. **Incremental Datalog maintenance** — when base facts change (insert or delete),
   the materialised closure is updated incrementally using the Backward/Forward (BF)
   algorithm rather than re-materialising from scratch.
   Tracked by [#83](https://github.com/daghovland/rdf-datalog/issues/83).

---

## Part 1 — Durable Transactional Storage

### Durability guarantee

Every mutating endpoint (SPARQL Update, GSP PUT/POST/DELETE, graph-store admin
create/delete) must commit a durable write transaction *before* returning a
success response. If the server crashes between the client receiving `200 OK`
and the next restart, the data must still be present after restart.

### Storage backend: `redb`

`redb` is a pure-Rust embedded key-value database with ACID transactions
(write-ahead log + fsync on commit). It has no external C dependencies and
compiles to a single static binary alongside dagalog.

**Why `redb` over alternatives:**
- `sled` — mature but does not guarantee durability by default; fsync is optional.
- `rocksdb` — C++ dependency, large compile overhead.
- Custom WAL — high correctness risk; `redb` already handles this correctly.
- `redb` — pure Rust, simple API, proven ACID guarantees, actively maintained.

### Data model

`redb` stores key-value pairs in named tables. The quad tables map to:

| `redb` table | Key | Value | Notes |
|---|---|---|---|
| `quads_by_spog` | `(graph, subject, predicate, object): (u32,u32,u32,u32)` | `()` | Primary dedup table |
| `quads_by_predicate` | `(predicate, graph, subject, object)` | `()` | Index for predicate lookups |
| `quads_by_sp` | `(subject, predicate, graph, object)` | `()` | Subject+predicate index |
| `quads_by_op` | `(object, predicate, graph, subject)` | `()` | Object+predicate index |
| `quads_by_graph` | `(graph, subject, predicate, object)` | `()` | Graph-local scan |
| `reified_triples` | `(subject, predicate, object, graph)` | `()` | Reified triple store |
| `resources_fwd` | `GraphElement` (serialised) | `u32` (id) | Interning: element → id |
| `resources_rev` | `u32` (id) | `GraphElement` (serialised) | Interning: id → element |
| `resources_next_id` | `()` | `u32` | Next free id counter |

The `GraphElementManager` in-memory cache is warmed from `resources_fwd` /
`resources_rev` on startup and kept in sync during writes.

### `PersistentDatastore` wrapper

A new type `PersistentDatastore` wraps a `redb::Database` and an in-memory
`GraphElementManager` (for fast id lookups without hitting the DB on every
intern call):

```rust
pub struct PersistentDatastore {
    db: redb::Database,
    resources: GraphElementManager, // in-memory cache, synced to redb on commit
}
```

The existing `Datastore` remains the in-memory-only type (used in tests and
the CLI one-shot query path). The HTTP server switches to `PersistentDatastore`.

### Transaction lifecycle in the HTTP layer

```
Incoming mutating request
    │
    ▼
Open redb write transaction
    │
    ▼
Apply quad insertions / deletions to redb tables
    │
    ▼
Commit (fsync)  ──── error? ──► 500, transaction auto-rolled-back
    │ Ok
    ▼
Respond 200/204
```

Read requests open a `redb` read transaction (snapshot-isolated; zero contention
with concurrent writes).

### Startup / recovery

On server start:
1. Open the `redb` database file (creates it if absent — empty store).
2. `redb` automatically replays its WAL to recover any committed-but-not-checkpointed
   transactions from a previous crash.
3. Warm the `GraphElementManager` cache by iterating `resources_fwd`.
4. The server is ready to accept requests.

No application-level crash recovery code is needed; `redb` handles it.

### Configuration

#### Storage mode

The server supports two modes. The mode is selected at startup and cannot be
changed at runtime.

| Mode | How to select | Behaviour |
|---|---|---|
| **In-memory** (default) | omit `--data-dir` and `DAGALOG_DATA_DIR` | Data is lost on restart. Current behaviour; useful for development, CI, and ephemeral pipelines. |
| **Persistent** | supply `--data-dir <PATH>` or `DAGALOG_DATA_DIR` | Data is durable: committed writes survive crash and restart. |

The in-memory mode is the default so existing deployments (Docker Compose,
CI pipelines, CLI one-shot queries) are unaffected.

#### CLI flags and environment variables

| CLI flag | Env var | Description | Default |
|---|---|---|---|
| `--data-dir <PATH>` | `DAGALOG_DATA_DIR` | Directory in which `dagalog.redb` is created | *(absent → in-memory)* |
| `--no-persist` | `DAGALOG_NO_PERSIST=1` | Force in-memory mode even if `DAGALOG_DATA_DIR` is set | `false` |

`--no-persist` exists so that a container image that bakes in `DAGALOG_DATA_DIR`
can be overridden for local development or testing without editing environment
configuration.

CLI flags take precedence over environment variables. `--no-persist` always
overrides `--data-dir` / `DAGALOG_DATA_DIR`.

#### Effective path

The database file is opened at `<data-dir>/dagalog.redb`:

```
--data-dir /var/lib/dagalog
→ /var/lib/dagalog/dagalog.redb
```

The directory is created on first start if it does not exist.

#### Multi-dataset limitation (current implementation)

The current changelog implementation uses a **single `dagalog.redb` file** and
keys log entries only by named-graph IRI.  Writes to extra datasets created via
the admin API (`POST /$/datasets`) are persisted in the same changelog but will
replay into the **default dataset** on restart — not into the separately-named
dataset.

**Recommendation:** use the admin API to create extra datasets only in in-memory
mode (no `--data-dir`), or route all durable writes through the default dataset.

Per-dataset isolation (each dataset its own `redb` file) is planned as phase P6
once the single-dataset path is proven stable.

---

### Storage location options

#### Local disk (recommended)

Pass any local path via `--data-dir`. Works on bare metal, VMs, and containers
with a local volume.

```sh
dagalog --serve --data-dir /var/lib/dagalog
```

#### Docker — named volume

Mount a Docker named volume at the data directory. Named volumes are managed by
Docker and persist across container restarts:

```sh
docker run -p 3030:3030 \
  -v dagalog_data:/var/lib/dagalog \
  dagalog --serve --data-dir /var/lib/dagalog
```

Or in `docker-compose.yml`:

```yaml
services:
  dagalog:
    image: ghcr.io/daghovland/rdf-datalog
    ports: ["3030:3030"]
    volumes:
      - dagalog_data:/var/lib/dagalog
    environment:
      DAGALOG_DATA_DIR: /var/lib/dagalog
volumes:
  dagalog_data:
```

#### Docker — bind mount (host path)

```sh
docker run -p 3030:3030 \
  -v /home/user/dagalog-data:/var/lib/dagalog \
  dagalog --serve --data-dir /var/lib/dagalog
```

Bind mounts expose the raw database file to the host, which is useful for
inspection or backup but requires the host directory to have correct permissions
for the container user.

#### Kubernetes — PersistentVolumeClaim

Use a `PersistentVolumeClaim` backed by block storage (ReadWriteOnce). Block
storage is required; do not use `ReadWriteMany` NFS volumes (see caveat below).

```yaml
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: dagalog-data
spec:
  accessModes: [ReadWriteOnce]
  resources:
    requests:
      storage: 10Gi
  storageClassName: standard  # or managed-premium, gp3, etc.
---
spec:
  containers:
    - name: dagalog
      image: ghcr.io/daghovland/rdf-datalog
      env:
        - name: DAGALOG_DATA_DIR
          value: /var/lib/dagalog
      volumeMounts:
        - name: data
          mountPath: /var/lib/dagalog
  volumes:
    - name: data
      persistentVolumeClaim:
        claimName: dagalog-data
```

Cloud block storage products (Azure Managed Disks, AWS EBS, GCP Persistent
Disk) all appear as local block devices when mounted as a PVC. No special
dagalog configuration is needed — `--data-dir` suffices.

#### NFS and network file systems — caveat

`redb` relies on `fsync(2)` to guarantee that a committed write has reached
durable storage. Many NFS mounts, and some managed file services (e.g. Azure
Files with the NFS protocol, EFS), do not fully honour `fsync` semantics
across the network. Using such a path for `--data-dir` risks database
corruption on a crash.

**Recommendation:** use block storage (mounted as a local filesystem) rather
than NFS. If NFS is unavoidable, run without persistence (`--no-persist`) and
accept that data is not durable, or use a separate backup/export mechanism.

Azure File shares and AWS EFS accessed via the SMB/CIFS protocol have the
same caveat and should not be used as the `--data-dir`.

#### Cloud object storage (Azure Blob, AWS S3, GCP Cloud Storage) — out of scope

Object stores are not suitable as the database backend:
- They expose a put/get/delete API for named objects, not a POSIX filesystem
  with mmap and fsync.
- `redb` (and all embedded WAL-based databases) require random-write access and
  reliable fsync that object stores cannot provide.

**Object storage can be used for backup/export** (e.g. exporting a Turtle dump
of the dataset and uploading to a blob container), but that is a separate
backup pipeline, not the live persistence mechanism.

If you need to run dagalog in a fully serverless / stateless container
environment without a persistent volume, use in-memory mode and populate from
an upstream source on startup.

### Implementation phases

| Phase | Description |
|---|---|
| P1 | Add `redb` dependency; implement `resources_fwd` / `resources_rev` tables; persistence for `GraphElementManager` |
| P2 | Implement `quads_by_spog` (primary dedup table) + `reified_triples`; wire up `add_quad` / `remove_quad` |
| P3 | Add remaining quad indexes; verify all SPARQL query paths work against `PersistentDatastore` |
| P4 | Wire HTTP layer: each mutating handler opens a write transaction, commits before responding |
| P5 | `--data-dir` flag; startup warm-up; crash-recovery integration test (kill server mid-write, restart, verify data) |
| P6 | Dataset-level isolation (each dataset gets its own `redb` database file under `<data-dir>/`) |

---

## Part 2 — Incremental Datalog Maintenance (Backward/Forward Algorithm)

### Motivation

The current `datalog::evaluate_rules` performs **naive forward-chaining
full re-materialisation**: it recomputes the entire closure from scratch each
time. This is correct but expensive when:
- The base graph receives small incremental updates (INSERT / DELETE via SPARQL
  Update or GSP).
- The closure (materialised OWL-RL or custom rules) is large.

The **Backward/Forward (BF) algorithm** (Gupta, Katseberis & Mumick 1993) maintains
the materialised closure incrementally across insertions and deletions.

### Algorithm overview

Given a set of *base facts* Δ⁺ to insert and Δ⁻ to delete:

#### For deletions (Backward phase then Forward phase)

1. **Backward phase** — compute the set of *possibly-deleted derived facts* `PD`:
   for each fact `f` in the current closure that was derived using a deleted base
   fact (directly or transitively), add `f` to `PD`. This is done by backward
   chaining from the deleted base facts through the rules.

2. **Forward phase** — determine which facts in `PD` can still be derived from
   the remaining base facts and the closure minus `PD`. Re-derive using the
   surviving base facts; anything in `PD` that cannot be re-derived is truly
   deleted.

3. Remove all facts in `PD` that were not re-derived.

#### For insertions

Run **semi-naive evaluation** starting from the new base facts Δ⁺: propagate
only the *new* consequences (those not already in the closure) rather than
re-running everything from scratch.

#### Combined update

When a SPARQL Update or GSP request contains both insertions and deletions
(e.g. `DELETE {...} INSERT {...} WHERE {...}`):

1. Apply deletions with BF (backward + forward phases).
2. Apply insertions with semi-naive evaluation.
3. The order matters when inserted and deleted facts overlap — handle via
   transaction-level staging (see [#114](https://github.com/daghovland/rdf-datalog/issues/114)).

### Data structures

| Structure | Purpose |
|---|---|
| `MaterialisedClosure` | The current derived fact set (on top of base facts). Stored separately from base facts so BF can distinguish them. |
| `DerivedFrom` index | For each derived fact, records which rules and which body facts were used to derive it. Required for the backward phase. |
| `SemiNaiveEvaluator` | Runs delta evaluation: tracks `new_this_round` and `total_new` sets, iterates until fixpoint. |

### API changes

The current `evaluate_rules(rules, datastore)` re-materialises everything. The
new API:

```rust
pub struct IncrementalReasoner {
    rules: Vec<Rule>,
    closure: MaterialisedClosure,
}

impl IncrementalReasoner {
    pub fn new(rules: Vec<Rule>, base: &Datastore) -> Self;

    // Apply a batch of inserts and deletes to the base store, then update the closure.
    pub fn apply_update(
        &mut self,
        base: &mut Datastore,
        inserts: &[Quad],
        deletes: &[Quad],
    );
}
```

The closure is stored as a separate `QuadTable` (or a flag per quad in the
persistent store indicating `base` vs. `derived`).

### Stratified negation

Stratified negation is already handled by `RulePartitioner`. With BF:
- Strata are evaluated in order; lower strata are stable before higher strata run.
- A deletion that affects stratum *k* may propagate upward to strata > *k* —
  the BF backward phase must traverse strata in reverse order.
- The `DerivedFrom` index stores the stratum at which each fact was derived,
  enabling correct stratum-ordered backward traversal.

### Implementation phases

| Phase | Issue | Description |
|---|---|---|
| D1 | [#107](https://github.com/daghovland/rdf-datalog/issues/107) | Separate base facts from derived facts in `QuadTable`: add `derived_quads: HashSet<Quad>`, `add_derived_quad`, `is_base`, `base_quads`. |
| D2 | [#108](https://github.com/daghovland/rdf-datalog/issues/108) | Build `DerivedFrom` index during materialisation; store rule id + body witnesses per derived fact. |
| D3 | — | Semi-naive forward evaluation for insertions. **Already implemented** as `DatalogProgram::materialise_seminaive` in `datalog/src/reasoner.rs`. |
| D4 | [#109](https://github.com/daghovland/rdf-datalog/issues/109) | BF backward phase: given Δ⁻, compute `PD` by backward traversal of `DerivedFrom`. |
| D5 | [#109](https://github.com/daghovland/rdf-datalog/issues/109) | BF forward phase: re-derive from surviving base facts; prune unre-derivable facts from `PD`. |
| D6 | [#110](https://github.com/daghovland/rdf-datalog/issues/110) | Integrate with SPARQL Update / GSP handlers: call `IncrementalReasoner::apply_update` after each mutation. |
| D6a | [#114](https://github.com/daghovland/rdf-datalog/issues/114) | Batch all quad inserts/deletes from one SPARQL Update request before calling the reasoner (transaction atomicity). Currently the reasoner is called once per statement; intermediate states can produce transient wrong inferences. |
| D7 | [#111](https://github.com/daghovland/rdf-datalog/issues/111) | Benchmark BF vs. full re-materialisation on LUBM scale 1/5/10; measure DerivedFrom index memory overhead and tipping point. |

### Memory and performance cost model

The DerivedFrom index (D2) is the most memory-intensive addition. See
[`docs/plans/PERFORMANCE.md`](PERFORMANCE.md) §"BF Incremental Datalog: memory and
performance analysis" for a full breakdown including:

- Baseline QuadTable cost: ~160 bytes per quad (10× raw data)
- D1 `derived_quads` cost: ~56 bytes per *derived* quad only (zero for pure-TBox workloads)
- D2 DerivedFrom cost: ~144 bytes per derived quad minimum; ~1.1 GB for LUBM scale 10 at 5×
  OWL-RL expansion
- Mitigations: lazy DerivedFrom (opt-in), compact witness storage (`QuadListIndex` vs `Quad`),
  single-derivation cap, depth cap, automatic fallback to full re-mat when |PD| > 25%
- Empirical tipping point: BF wins for |Δ⁻| / |base| ≲ 15–20%; full re-mat wins above

**Key design decision for D2:** the DerivedFrom index must be **opt-in** (only built when
`IncrementalReasoner` is explicitly constructed). The static batch-load + query-only path must
pay zero incremental maintenance cost.

### References

- Gupta, A., Katseberis, I. S., & Mumick, J. A. (1993). Maintaining views incrementally. *SIGMOD Record*, 22(2), 157–166.
- Motik, B., Nenov, Y., Piro, R., & Horrocks, I. (2015). Incremental update of datalog materialisation: the backward/forward algorithm. *AAAI 2015*.
- The Motik et al. 2015 paper refines the original BF for the specific case of RDF/OWL-RL materialization and is the recommended reference for this implementation.
