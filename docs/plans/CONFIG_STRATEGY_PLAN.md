# Config/command-language strategy for dagalog setup & administration

Epic: [#420](https://github.com/daghovland/rdf-datalog/issues/420) (consolidates
[#233](https://github.com/daghovland/rdf-datalog/issues/233) and
[#401](https://github.com/daghovland/rdf-datalog/issues/401)). Surfaced while
working on [#415](https://github.com/daghovland/rdf-datalog/issues/415).

This is a research/recommendation document, not an implementation plan. It does
not spin off implementation sub-issues itself (per #420's scope) — it gives
enough detail that sub-issues can be scoped from it once reviewed.

**Status caveat on anything about Fuseki or RDFox below:** descriptions of
Fuseki's `config.ttl` assembler model and RDFox's `.rdfox` script commands
(`dstore create`, `import`, `importaxioms`, `commitproc set`, `endpoint
start`) are used only as general design reference points, carried over from
#233's and #420's own framing. They are not independently re-verified against
those tools' current documentation in this pass. Everything said about
dagalog's current behavior below is checked against the code at the paths
cited, and everything said about Dexpi2Imf's scripts (§3) is checked against
the scripts' verbatim contents, fetched directly.

## 1. The four developer stories (from #233)

These are concretized as scenarios — what each persona actually does at a
terminal or in a deployment script, not abstract requirements.

### 1a. The database / transaction-system persona

Alice is building a service that uses dagalog as the backing store for a live
application: user actions become SPARQL Updates, occasionally wrapped in
explicit transactions (`/transaction/begin` / `/transaction/{id}/commit` /
`/transaction/{id}/rollback` — `sparql_endpoint/src/transaction_routes.rs`).
She needs:
- The server to come up already durable (`--data-dir`, backed by a redb
  changelog — `sparql_endpoint/src/persistence.rs`) so a restart replays
  exactly what was committed, not a re-run of some seed file.
- To be able to provision new logical datasets for new tenants *without*
  restarting the process — `POST /$/datasets` already does this
  (`sparql_endpoint/src/admin.rs::admin_create_dataset`), but only creates an
  **empty** `Datastore::new(1024)`: no data, no rules, no ontology. Alice has
  to follow up with more calls to actually seed a new tenant.
- Access control per tenant/role (OIDC roles, API key) and CORS already exist
  as CLI flags/env vars (`--oidc-*`, `--api-key`, `--cors-allow-origin`), but
  they are process-global, not per-dataset.
- Transactions (`/transaction/begin`/`{id}/commit`/`{id}/rollback`) are
  **server-wide**, not per-dataset — `dataset_routes.rs`'s doc comment on
  `dataset_state()` notes this explicitly, citing
  [#125](https://github.com/daghovland/rdf-datalog/issues/125) as the
  tracking issue for per-dataset transactions. Alice's multi-tenant story
  can't isolate one tenant's transaction from another's today.
- What she explicitly does **not** want: a static file that's only consulted
  at startup. Her setup is inherently a sequence of runtime operations
  (create tenant, load schema, open transaction, commit), which is closer to
  a command/script model than a declarative snapshot.

### 1b. The OWL-RL-over-changing-data persona

Bob has a dataset that keeps changing (sensor data, a live asset register)
and wants OWL-RL inference kept up to date incrementally as it does. This is
dagalog's actual sweet spot architecturally: `IncrementalReasoner`
(`datalog` crate) is built once at startup from `--ontology`/`--rules` (via
`collect_serve_rules`, `src/main.rs`), then every subsequent SPARQL
Update / Graph Store Protocol write re-runs materialization incrementally
(`sparql_endpoint/src/dataset_routes.rs::dataset_update_post`,
`sparql_update::apply_prepared_update_with_options`). Bob needs:
- A reliable way to declare, once, "this dataset reasons over these ontology
  files plus these Datalog rules files" — exactly what `--ontology`/`--rules`
  already do for the **one** default dataset at startup.
- A way to change the active ruleset *without* restarting, for a non-default
  dataset too — `POST /{name}/rules` / `POST /{name}/rules/{ruleset_id}`
  already exist (`sparql_endpoint/src/rules_endpoint.rs`) and do an
  incremental diff-and-rebuild rather than a full reload
  ([#568](https://github.com/daghovland/rdf-datalog/issues/568)) — but they
  only accept `text/x-datalog`/`text/plain` Datalog rule text, **not** OWL
  ontology files (confirmed: `rules_endpoint.rs` parses every request body
  with `datalog_parser::parse`, with no OWL/Manchester/Functional-Syntax
  branch). So Bob can swap Datalog rules live on any dataset at runtime, but
  there is **no runtime OWL-ontology loading at all**, for any dataset,
  default or not — `--ontology` only ever runs once, at process startup,
  against the implicit default dataset (`src/main.rs::run`, before
  `--serve` branches). Changing Bob's ontology currently means a full
  process restart, regardless of which dataset he's using.
- The `owl:Nothing` 409-conflict check (`sparql_endpoint/src/constraints.rs`,
  wired into `dataset_update_post`) to keep firing as data changes — it
  already does, for any dataset that has a reasoner slot populated.

### 1c. The full-OWL reasoner/validation persona

Carol wants to *validate and navigate* an ontology — not just materialize
RL-derivable consequences over data, but get genuine OWL DL-level reasoning
services (consistency checking beyond `owl:Nothing`, classification,
explanation). **This persona's story cannot be fully told by any config or
command language dagalog adopts**, because dagalog's own reasoning engine is
OWL 2 RL only (see `owl2rl2datalog`, `eli` crates in the architecture map) —
there is no DL reasoner underneath to invoke regardless of how setup is
expressed. What dagalog *can* offer this persona today is:
- Loading an ontology for OWL-RL materialization (`--ontology`, same path as
  1b) plus SHACL shape validation per dataset (`POST /{name}/shacl` —
  `sparql_endpoint/src/shacl_endpoint.rs`) as a validation substitute.
- `.omn` / `.ofn` / `.owx`/`.owl`-sniffed ontology loading for TBox, with
  ABox assertions materialized into quads
  (`owl2rl2datalog::abox::assert_abox`) for the formats that have no native
  RDF-triple ABox representation (Manchester, Functional Syntax).
A config/command-language decision should **not** attempt to paper over this
gap by promising Carol something dagalog's reasoner can't do; it should just
make the OWL-RL-plus-SHACL path as easy to declare/replay as possible, same
as persona 1b.

### 1d. The fast-analytics querier persona

Dana just wants to point dagalog at a fixed, large, mostly-static dataset and
run SPARQL queries against it — batch analytics, not a live service. Her
actual commands are closest to Dexpi2Imf's real usage (§3 below): fixed
flag combinations, run once per analysis session. She needs:
- One-shot query mode (`--data ... --query ...`, printing results and
  exiting — no server) to keep working exactly as it does.
- Named, reusable "scenarios" (a fixed file-set for a fixed analysis) instead
  of hand-maintained shell wrappers — this is `--profile foo` territory, not
  a running-server requirement.
- No need for transactions, incremental reasoning, or per-tenant anything —
  her story is the simplest of the four, and the config/command-language
  decision should not force complexity from 1a/1b onto her.

## 2. What dagalog already has (verified against code)

### 2a. CLI flags (`src/main.rs`, `clap::Parser` struct `Cli`)

- `--data`/`-d` (repeatable): Turtle/TriG files loaded via `src/lib.rs::load_file`
  into the single in-process `Datastore` at startup, in listed order.
- `--mapping`/`-m` (RML), `--ottr` (stOTTR): additional repeatable pre-load
  transforms, applied in a fixed order (data → RML → OTTR → ontology/rules)
  before anything is served or queried.
- `--ontology`/`-o` (repeatable): `.omn` (Manchester), `.ofn` (Functional
  Syntax), `.owx`/sniffed `.owl` (OWL/XML), or Turtle/TriG RDF translated
  into OWL axioms (`rdf_owl_translator`) — dispatch to the right parser is
  done by `src/lib.rs::apply_ontologies`/`collect_serve_rules` (**not**
  `load_file`, which only handles the plain-RDF `--data` path), by file
  extension plus `.owl` content-sniffing via
  `owl_xml_parser::looks_like_owl_xml`, all converging on the same
  `owl_ontology::Ontology` type.
- `--rules`/`-r` (repeatable): `.datalog` rule files (`datalog_parser`).
- `--query`/`-Q`, `--query-file`/`-q`, `--format`/`-f`: one-shot query mode.
- `--serve`, `--port`, `--base-iri`: start the HTTP endpoint instead.
- `--read-only`, `--query-timeout`, `--max-rdf-upload-bytes`,
  `--max-rml-upload-bytes`, `--request-timeout`, `--max-concurrent-writes`,
  `--cors-allow-origin`: server hardening/limits, every one of them also an
  env var (`DAGALOG_*`) already — i.e. most *non-input-file* flags already
  have an env-var escape hatch; `--data`/`--mapping`/`--ottr`/`--ontology`/
  `--rules` (the actual input-file lists, repeatable) do **not**.
- `--api-key`, `--require-auth-for-reads`, `--oidc-*` (issuer/audience/
  jwks-uri/roles-claim/read-role/write-role/admin-role/browser-client-id):
  auth config, all env-backed too.
- `--data-dir`/`--no-persist`: durable persistence toggle.
- `--network`: SSRF-hardening policy for LOAD/`@context`/SERVICE fetches.

**Critical gap confirmed for #401's "multiple scenarios" story:** every one
of `--data`/`--ontology`/`--rules`/`--mapping`/`--ottr` only ever populates
the **one** default dataset (`"ds"`) built before `--serve` branches
(`src/main.rs::run`, lines ~337–466). There is currently no CLI-flag way to
seed more than one named dataset at startup — the only way to get a second
dataset today is `POST /$/datasets` (empty) at runtime, followed by
per-dataset HTTP calls to populate it (`/{name}/data` for triples,
`/{name}/rules` for Datalog rules only — see 1b above for the OWL gap).

**One-shot mode is file-based, not registry-based**: `--query` on a fresh
process loads files, runs one query, and exits; it never touches the
admin-API dataset registry at all. Any config/command-language design that
routes everything through "the admin API" would have to special-case this
mode, or give up on it.

**Persistence semantics confirmed (`sparql_endpoint/src/lib.rs::serve_on_listener`,
`sparql_endpoint/src/persistence.rs`):** `--data`/`--ontology`/`--rules` are
loaded into a *fresh* `Datastore` on every process start, and only *after*
that does `serve_on_listener` open the changelog (if `--data-dir` is set)
and call `QuadChangelog::replay_into()`, whose own doc comment says it
"layers changelog mutations ON TOP of any pre-loaded data (e.g. from `--data`
files), so both sources are visible" (issue
[#66](https://github.com/daghovland/rdf-datalog/issues/66)). So **`--data`/
`--ontology`/`--rules` are effectively re-seeded on every boot**, with the
durable changelog's own writes (from `/update`, GSP, `/{name}/rules`, etc.)
replayed on top each time. A config file inherits this as-is unless it says
otherwise: by default, every dataset entry's `data`/`ontology`/`rules` lists
are "load every start," not "seed once." Persona 1a (Alice), whose actual
want is durable state that survives restarts *without* being re-derived from
a seed file every time, should instead rely on `--data-dir` alone (no file
lists) for the parts of her dataset that change at runtime, and only use the
config file's file lists for genuinely static schema/ontology/rules inputs —
this distinction should be called out explicitly in the config-file's own
documentation (and arguably in a `once: true`/`reload: true` per-entry flag,
see §5), not left implicit the way plain `--data` leaves it today.

### 2b. Admin/HTTP API (`sparql_endpoint/src/*.rs`)

Already a materially larger set of *runtime* operations than the CLI
surfaces at startup:
- `GET/POST /$/ping`, `GET/POST /$/ready`, `GET /$/server`: liveness/metadata.
- `GET /$/datasets`, `POST /$/datasets`, `GET/DELETE /$/datasets/{name}`:
  dataset lifecycle. `POST` accepts either Fuseki-form
  (`dbName=/{name}&dbType=mem`) or a `text/turtle` Fuseki assembler payload
  (`admin.rs::extract_fuseki_name_from_assembler`, added properly in
  [#415](https://github.com/daghovland/rdf-datalog/issues/415)/PR
  [#419](https://github.com/daghovland/rdf-datalog/pull/419), replacing an
  earlier substring-search implementation). The assembler path extracts
  **only** `fuseki:name` via real Turtle parsing; a declared `fuseki:dataset`
  `rdf:type` other than `ja:MemoryDataset`, or any `fuseki:endpoint` blocks,
  are logged via `log::warn!` but not honored — dagalog always creates an
  in-memory dataset with its own fixed route set regardless of what the
  assembler Turtle otherwise declares.
- `POST /$/compact`: changelog compaction.
- Per-dataset: `/{name}/sparql`, `/{name}/query` (read),
  `/{name}/update` (SPARQL Update, incremental-reasoner-aware, `owl:Nothing`
  checked), `/{name}/data`, `/{name}/get` (Graph Store Protocol),
  `/{name}/shacl` (SHACL validation), `/{name}/rml` (RML mapping, dataset-
  scoped), `/{name}/ottr` (OTTR expansion, dataset-scoped), `/{name}/rules`
  and `/{name}/rules/{ruleset_id}` (Datalog-only live ruleset, incremental
  since #568).
- Root-level (implicit default dataset, `"ds"`): `/sparql`, `/rdf-graph-store`,
  `/rdf-graphs/*`, `/upload`, `/transaction/begin|{id}/commit|{id}/rollback`.

So **the admin API already covers almost every operation a command/script
language would need to express** (create dataset, load data, load rules,
query, transact) — except OWL ontology loading at runtime (missing for
every dataset, not just non-default ones — see 1b above), and multi-dataset
provisioning at process startup. Those are the two real gaps, not "there's
no API at all."

### 2c. Existing scripting hooks

- **`dagalog-kernel`**: a real Jupyter-protocol kernel (`dagalog-kernel/src/
  cell/*.rs`) with cell magics `%%load` (Turtle/TriG), `%%rml`, `%%manchester`,
  `%%functional`, `%%owlxml`, `%%ottr`, `%%datalog`, `%%validate` (SHACL),
  `%%sparql` — i.e. an interactive, replayable, imperative sequence of
  operations against one in-process store, cell by cell. This is the closest
  thing dagalog already has to RDFox's `.rdfox` script model, but scoped to
  the Jupyter wire protocol rather than a standalone batch file/CLI
  sub-command — it requires a Jupyter frontend (or `jupyter console`/
  `nbconvert --execute`) to drive it, not `dagalog somefile.script`.
- No standalone "script file interpreter" binary/mode exists outside that
  kernel and the CLI's own fixed startup sequence.

## 3. The #401 evidence (Dexpi2Imf), verified against the actual scripts

Fetched the verbatim contents of all three scripts from
[equinor/Dexpi2Imf@explore/rdf-datalog-replacement/dagalog/](https://github.com/equinor/Dexpi2Imf/tree/explore/rdf-datalog-replacement/dagalog)
(public repo; org SAML blocks the GitHub *API*, but the raw files are
readable directly). Their actual content:

```sh
# dexpi.sh
exec "$DAGALOG" --serve --port 3030 \
  --ontology ../owl/dexpi.ttl \
  --ontology ../owl/imf-ontology.owl.ttl \
  --ontology ../owl/boundary.ttl \
  --rules ../datalog/noaka_boundary.datalog \
  --data ../rml_mappings/pandid.trig \
  "$@"

# boundaries.sh
exec "$DAGALOG" --serve --port 3030 \
  --ontology ../owl/dexpi.ttl \
  --ontology ../owl/imf-ontology.owl.ttl \
  --ontology ../owl/boundary.ttl \
  --rules ../datalog/noaka_boundary.datalog
  # (no --data; this one is fed entirely via SPARQL Update once running)

# connected.sh
exec "$DAGALOG" --serve --port 3030 \
  --ontology ../owl/dexpi.ttl \
  --ontology ../owl/imf-ontology.owl.ttl \
  --ontology ../owl/boundary.ttl \
  --rules ../datalog/surrounding.datalog \
  --data ../rml_mappings/pandid.trig \
  "$@"
```

(`DAGALOG="${DAGALOG:-dagalog}"` and `cd "$(dirname "$0")"` precede each
`exec` in all three; `set -e` is also common to all three.)

Corrected reading (the previous draft of this doc paraphrased the sibling
scripts from a summary and got the specifics wrong — this is the actual,
verbatim comparison):
- All **three** scripts load the identical three `--ontology` files
  (`dexpi.ttl`, `imf-ontology.owl.ttl`, `boundary.ttl`).
- `dexpi.sh` and `boundaries.sh` use the **same** rules file
  (`noaka_boundary.datalog`); `connected.sh` is the one that differs, using
  `surrounding.datalog` instead.
- `dexpi.sh` and `connected.sh` load the **same** data file
  (`pandid.trig`); `boundaries.sh` is the one that differs, loading no data
  at all (its backend feeds it via SPARQL Update once serving).
- `dexpi.sh` and `connected.sh` both forward `"$@"`, so either can be run
  one-shot via an appended `--query`/`-Q`; `boundaries.sh` does not forward
  extra args (it's serve-only by design, matching its own comment).
- None of the three take overrides through their own named flags or
  environment variables beyond `DAGALOG` (the binary path) — there is no
  existing per-scenario parameterization to preserve, only the raw
  duplication to replace.

This confirms "profiles" in #401 means **alternate, named, fixed invocations
run one at a time** (pick a scenario, then `--serve` or one-shot `--query`
it) — not several datasets served simultaneously from one process. But it
also surfaces a design requirement the earlier draft missed: **the
duplication #401 complains about is mostly the shared ontology list (and
partly the shared data file)**, repeated identically across two or three
scripts, with only one or two fields actually varying per scenario (rules
file; presence/absence of data). A config-file design that just gives each
profile its own fully self-contained `[[dataset]]` block (§5 as originally
drafted) would merely relocate that same duplication into TOML instead of
removing it — see §5's revision below for how the schema addresses this.

## 4. Evaluating the three directions

### Direction 1 — declarative config file (Fuseki `config.ttl`-style)

Fits personas 1d (fast analytics) and the startup half of 1b/1c well: a
static manifest of "load these ontologies/rules/data, serve on this port"
is exactly #401's ask, and directly replaces the Dexpi2Imf scripts with one
file (or one file per named profile, selected by `--profile`/positional
arg). It does **not** on its own solve 1a (Alice's provision-tenants-at-
runtime story) or the live-ruleset-swap half of 1b — those are inherently
runtime operations, not "what to load once at boot." A pure declarative file
read only at startup would leave those personas exactly where they are
today (admin API).

### Direction 2 — imperative command/script language (RDFox `.rdfox`-style)

Subsumes direction 1 (a script can "do" the equivalent of a static manifest
by issuing a fixed sequence of create/load commands at startup) and *also*
reaches 1a's and 1b's runtime-operation stories, if the same interpreter can
be driven both as a startup batch and attached live to a running server (closer
to what `dagalog-kernel`'s cell magics already do, minus the Jupyter wire
protocol dependency). The cost is a materially bigger surface to design,
parse, and keep stable than a config file — a genuinely new DSL, its own
grammar crate, error handling, and documentation — for a payoff that, per
§2b, the *existing* admin API already delivers for every runtime operation
except the two confirmed gaps (runtime OWL-ontology loading, multi-dataset
startup provisioning).

### Direction 3 — thin client-side tool over the existing HTTP admin API

Reframes the problem as "dagalog already has the right primitives
server-side (§2b); what's missing is a convenient way to *drive* them,"
rather than inventing new server-side surface. Concretely: a small client
(could be a `dagalog-admin` companion binary, or a mode of the existing
`dagalog` binary) that reads one file and replays it as a sequence of HTTP
calls (`POST /$/datasets`, `PUT`/`POST .../data`, `POST .../rules`, etc.).
This is portable (no shell dependency — closing #401's Windows complaint;
note that "a file is more portable than a script" on its own doesn't
distinguish this from a Rust CLI subcommand either, since both are
cross-platform binaries — the actual portability win here is dropping `.sh`
specifically) and reuses tested server-side logic instead of
duplicating it. Its hard limitation: it needs a **running server** to talk
to, so it cannot serve persona 1d's and 2a's one-shot (`--data ... --query
...`, no `--serve`) mode at all — that mode has no HTTP surface to drive by
construction, and must keep its own flag/config path regardless of what else
is decided.

## 5. Recommendation

**Adopt a declarative startup config file, defined so that every entry in it
is explicitly equivalent to a sequence of operations the admin API (§2b)
already performs** — not a competing model, but a serialization of the same
primitives direction 3 would otherwise replay over HTTP. Concretely:

**Explicit v1 scope — profile ≠ dataset.** §3 showed real Dexpi2Imf usage
never needs more than one dataset per run; §1a's multi-tenant story is a
*runtime* (admin-API) concern, not a startup-config one. So v1 is scoped to
**(a): a profile is one run configuration targeting the single implicit
default dataset** — exactly what `--data`/`--ontology`/`--rules` already
populate today, just selected from a named block in a file instead of
spelled out as flags every time. This is cheap (§7 below: no server-side
registry/reasoner-seeding changes, only a new way to populate the existing
`Cli`-equivalent fields before `run()`), and it fully covers §3's evidence
and persona 1d. **Multi-dataset startup provisioning (profiles seeding
several named datasets at once) is explicitly deferred**, not folded into
this recommendation — it would need new server-side seeding of the dataset
registry and per-dataset reasoners (`DatasetRegistry`,
`sparql_endpoint/src/registry.rs`), a materially bigger change than v1, and
nothing in §1–§3's evidence requires it yet. If a real multi-dataset-at-
startup need shows up later, it should be scoped as its own follow-up
against this same file format (each current top-level profile becomes one
of several dataset entries inside a profile, rather than a new format).

- Each profile ≡ the admin-API-equivalent operation sequence `PUT`/`POST
  /{name}/data` for each data/RML/OTTR source, then the ontology-loading
  path for OWL files, then `POST /{name}/rules` for Datalog rules — against
  the implicit default dataset, per the v1 scope above. (`POST
  /$/datasets` itself is not part of v1's equivalence, since v1 never
  creates a second dataset — see above.)
- **Schema composition, to avoid relocating #401's duplication into TOML**
  (§3's finding): profiles do not each list a fully independent, repeated
  `ontology`/`data`/`rules` set. Instead the schema separates a shared
  `[inputs]` (or similarly named) block of reusable named file-lists from
  the profiles that reference them, e.g. (illustrative, not final):
  ```toml
  [inputs.dexpi_ontologies]
  ontology = ["owl/dexpi.ttl", "owl/imf-ontology.owl.ttl", "owl/boundary.ttl"]

  [profiles.dexpi]
  uses = ["dexpi_ontologies"]
  rules = ["datalog/noaka_boundary.datalog"]
  data = ["rml_mappings/pandid.trig"]

  [profiles.boundaries]
  uses = ["dexpi_ontologies"]
  rules = ["datalog/noaka_boundary.datalog"]

  [profiles.connected]
  uses = ["dexpi_ontologies"]
  rules = ["datalog/surrounding.datalog"]
  data = ["rml_mappings/pandid.trig"]
  ```
  This maps §3's three scripts onto one file with the shared ontology list
  written exactly once, which is the actual drift problem #401 reports
  ("forgetting to add a new ontology file everywhere it's needed") —
  changing `dexpi_ontologies` once updates all three profiles. A flatter
  alternative (profile *inheritance*, `profiles.connected.extends =
  "dexpi"` plus per-profile overrides) achieves the same goal and should be
  compared on its own merits when the schema is actually designed (§7 item
  1) — this doc fixes *that composition must exist*, not which of the two
  concrete mechanisms wins.
- Named profiles (`--profile dexpi`, matching §3's actual Dexpi2Imf shape)
  select one profile block from the file; omitting `--profile` with only
  one profile defined behaves like today's single-dataset startup.
- Each profile's file lists default to "load every start" (§2a's confirmed
  persistence semantics) unless paired with `--data-dir`, in which case the
  changelog's own replayed writes still layer on top, exactly as `--data`
  does today — the config file does not change that interaction, it only
  changes how the file lists are spelled.
- **Precedence: CLI flag > environment variable > config file value**, for
  any setting present in more than one place — consistent with the fact
  that most non-file CLI flags already have an `env = "DAGALOG_*"` binding
  (§2a) and none currently have a config-file equivalent; this closes that
  gap uniformly rather than bolting config-file support onto only the
  file-list flags.
- **Format: native TOML** (directly answering #401's own suggested shape),
  not Turtle. Turtle/Fuseki-assembler-compatible ingestion already exists
  as a narrow, explicitly-partial compatibility shim for
  `POST /$/datasets` (§2b) and should stay scoped to that one Fuseki-
  interop purpose rather than be grown into dagalog's native config
  format — a TOML file describing `[profiles.*]` entries with
  `data`/`ontology`/`rules`/`mapping`/`ottr` arrays (mirroring the existing
  CLI flag names 1:1) is both closer to Rust-ecosystem convention
  (`clap`/`serde` already in the dependency tree) and avoids conflating
  "declare what to load" with "describe a Fuseki assembler," which are
  different audiences.
- This does **not** retire the admin API or propose a command/script
  language (direction 2) now: 1a's and the live-ruleset half of 1b's runtime
  operations keep using the HTTP admin API exactly as today, since §2b shows
  it already covers them (bar the OWL-rules gap, which should be closed
  regardless of this decision — see §6). A thin client tool (direction 3)
  remains a reasonable **follow-up**, layered on top of the same config
  format, if a future need emerges to *replay* a config file's dataset
  entries against an already-running server rather than only at the
  process's own startup — but that is not needed to close #401's actual
  complaint, so it is not recommended as part of this issue's scope.
- Persona 1c (full OWL/DL) gets no new capability from this: dagalog has no
  DL reasoner to configure regardless of config format (§1c). The config
  file should let that persona declare OWL-RL + SHACL inputs as easily as
  everyone else, and no more should be promised.

### Why not direction 2 outright

An RDFox-style command language subsumes directions 1 and 3, but at a
proportionally much larger design/implementation/maintenance cost (a new
grammar, interpreter, and its own test suite) for a payoff §2b shows the
existing admin API (plus the config file above) already delivers for every
identified persona need except the two confirmed gaps. If a future need
emerges that the config-file-plus-admin-API combination genuinely can't
express — e.g. conditional/branching setup logic, or scripted bulk
operations beyond what a flat declarative list can say — direction 2 should
be reopened as its own issue with that concrete need as its driver, rather
than built speculatively now.

### Why not direction 3 as the primary recommendation

It is the lowest-cost option and should still be kept in mind as a natural
follow-up (see above), but on its own it cannot touch persona 1d's and the
CLI's one-shot query mode at all (§2a, §4), which direction 1 covers
directly and at no extra cost beyond the config file itself.

## 6. Concrete gaps worth closing regardless of which direction is chosen

Independent of the recommendation above, two gaps were confirmed during this
survey that block real persona stories and should be tracked as their own
issues before (or alongside) config-file implementation work:

1. `POST /{name}/rules` / `POST /{name}/rules/{ruleset_id}`
   (`sparql_endpoint/src/rules_endpoint.rs`) accept only Datalog rule text
   (`datalog_parser::parse`) — there is **no runtime HTTP endpoint to
   load/replace an OWL ontology at all**, for any dataset, default or not;
   `--ontology` only ever runs once, at process startup. This blocks
   persona 1b's "runtime ontology swap" story outright (today it requires a
   full process restart, regardless of dataset) and is a prerequisite for
   any future extension of the config-file direction (§5) into genuinely
   live-updatable OWL-RL setups.
2. `POST /$/datasets` (`sparql_endpoint/src/admin.rs::admin_create_dataset`)
   always creates an **empty** `Datastore::new(1024)` — there's no way, via
   the admin API alone, to create-and-seed a dataset in one call; a client
   has to follow up with separate `/{name}/data`/`/{name}/rules` calls. Not
   necessarily wrong (it keeps each call single-purpose), but worth noting
   explicitly as a design constraint for any future multi-dataset-at-
   startup work (§5 v1 does not need this, since v1 never creates a second
   dataset).

Per this repo's CLAUDE.md rule on deferred work discovered mid-task, item 1
above (the missing runtime-OWL-loading endpoint) is filed as a tracked
follow-up issue,
[#759](https://github.com/daghovland/rdf-datalog/issues/759) (sub-issue of
this epic, #420), at Status `Todo`, since it is a real capability gap
independent of whichever direction this doc recommends and is independently
useful regardless of this issue's outcome. Item 2 is noted here as
implementation context for whoever scopes future multi-dataset-at-startup
work, not filed separately, since it is not a deficiency to fix on its own —
it's a design constraint that work would have to account for, and v1 (§5)
doesn't need it.

## 7. Suggested next steps (for review, not committed to by this doc)

Once reviewed, concrete sub-issues could include:
1. Define the TOML config schema (shared-inputs block + profile composition
   per §5, precedence rules, `once`/`reload` persistence semantics per
   §2a's confirmed behavior) as its own design-doc-plus-tests issue —
   including picking between the "named shared-inputs block" and "profile
   inheritance" composition mechanisms sketched in §5.
2. Wire `--config <file>` / `--profile <name>` into `src/main.rs`,
   resolving a chosen profile into the same `--data`/`--ontology`/
   `--mapping`/`--ottr`/`--rules` *file lists* `apply_ontologies`/
   `collect_serve_rules`/`load_file`/`apply_rules` already consume — v1
   scope only (§5): one profile targets the single implicit default
   dataset, so this is a config-parsing-and-field-population change in
   `src/main.rs`, not a `sparql_endpoint`/registry change.
3. File #759 (already created, Status `Todo`) for review/triage separately
   — it is not blocking v1 (§5 doesn't touch runtime OWL loading) but
   should be picked up before any future work that tries to make OWL-RL
   setups as live-updatable as Datalog-only ones.
4. Update `docs/user/deployment.md` with the config-file workflow alongside
   the existing CLI-flag examples, including the `once`/`reload` semantics
   distinction from §2a so persona 1a's story doesn't get accidentally
   broken by someone assuming file lists are "seed once" by default.
