# Fine-grained, identity-aware access control — design doc

Design doc for epic [#437](https://github.com/daghovland/rdf-datalog/issues/437).

This doc resolves the five open questions #437 lists, grounded in what the
code actually does today (checked, not recalled — see each section's "What
the code actually does" subsection), and ends with a phased breakdown
concrete enough to spin off sub-issues from once reviewed.

## Context: what this is *not* replacing

[`sparql_endpoint/src/auth.rs`](../../sparql_endpoint/src/auth.rs) and
[`docs/plans/AUTH.md`](AUTH.md) already implement a 3-tier `AuthConfig`
(`None`/`ApiKey`/`Oidc`) that classifies every HTTP request into
`Permission::{Read,Write,Admin}` via `classify(method, path)` and gates it in
`auth_middleware`. For `AuthConfig::Oidc`, a validated JWT's role claim
(`read_role`/`write_role`/`admin_role`, configurable per-provider via
`roles_claim`) must contain the right role for the classified permission.
This is **operation-shaped**: it answers "may this identity call
`POST /{name}/update` at all", never "which triples/graphs within that
update may this identity touch". The edge-proxy deployment in
[`docs/deploy/PUBLIC_DEPLOYMENT.md`](../deploy/PUBLIC_DEPLOYMENT.md) sits a
layer further out still (cookie-based email allowlist, same binary coarse
all-or-nothing shape) and explicitly calls out #437 as the next layer once
data-level control is needed.

Everything below is designed to sit **alongside** `auth.rs`, reusing its
identity/role machinery where possible rather than inventing a second,
parallel authentication stack.

## 1. Granularity: named-graph vs. triple/quad-level

### What the code actually does

- `dag_rdf::ingress::GraphElementId` is a `u32` — the *only* representation
  of a named graph is "one more interned `GraphElementId`", exactly like any
  other IRI/literal/blank-node resource. There is no `Graph` type distinct
  from `GraphElement`; a graph is a graph purely because some quad's `graph`
  slot holds that ID (`dag_rdf/src/ingress.rs:16,23,26`, `Quad` struct).
- `dag_rdf::quadtable::QuadTable` indexes quads by predicate, subject+predicate,
  object+predicate, and (via `triple_id_index` generically keyed by
  `GraphElementId`) by graph. `get_graph(id)` (`quadtable.rs:334`) iterates
  all quads whose graph component matches — an existing, already-indexed
  "give me everything in graph G" primitive.
- `dag_rdf::Datastore` (`datastore.rs:22`) holds exactly two `QuadTable`s:
  `named_graphs` (everything, default graph included —
  `DEFAULT_GRAPH_ELEMENT_ID = 0`) and `reified_triples` (RDF reification).
  There is **no per-triple metadata slot** anywhere in `Quad`/`Triple`
  (`ingress.rs:32-55`) — a quad is exactly `(subject, predicate, object,
  graph)`, four `GraphElementId`s, nothing else. Attaching an ACL tag to an
  individual triple would mean either growing `Quad` (a storage-format
  change touching every index in `QuadTable`, persistence, and every crate
  that pattern-matches on `Quad`) or encoding the triple's own identity as
  the policy's subject via reification. This is cheaper than it first
  looks: `Quad::triple_id` (`ingress.rs:49`) is *literally the same field*
  in both tables — "the ID of the containing graph" in `named_graphs` and
  "the reification ID" in `reified_triples` are the same `GraphElementId`
  slot wearing two different hats depending on which `QuadTable` it sits
  in. So a per-triple ACL doesn't need a new `Quad` field, only a
  convention: reify the triple (an existing operation) and attach a policy
  to its reification ID exactly as §3's RDF-native policy triples would
  attach to any other `GraphElementId`.

### Recommendation: named-graph-level, with reification as an escape hatch

Named-graph granularity maps onto an index the storage layer already has
(`get_graph`/`graph_exists`/`remove_graph` are all keyed by
`GraphElementId` already) and needs **zero storage-format changes**. It also
matches how data naturally partitions in this project's own real deployment
(the backlog dashboard: `bl:`/`agp:` instance data vs. the ontology vs. a
future second tenant's data) — the motivating #437 example of "different
users/roles seeing different graphs" is literally a graph-per-partition
problem already.

Triple-level control is explicitly **not** ruled out long-term, but should
be built as a second phase layered on top of graph-level control via
**reification**, not a `Quad` schema change: a policy rule can already say
"deny access to triples matching pattern P within graph G" by reifying the
matching triples (dagalog's own `reified_triples` table, `triple_id:
GraphElementId`) and attaching the policy to the reified triple's ID exactly
as RDF-native policy triples (§3) would attach to anything else. This keeps
one representation (IRI-keyed policy triples) usable at both granularities
rather than inventing a second ACL storage mechanism for the fine-grained
case. Defer actually building this until a concrete use case asks for
finer-than-graph control — the motivating use case in #437 and
`PUBLIC_DEPLOYMENT.md` is graph-level.

## 2. Where it's enforced: query-rewrite vs. post-filter vs. storage-level

### What the code actually does

- `sparql_parser::execute::execute_with_base` (`execute/mod.rs:210`) takes an
  already-*parsed* `&Query` AST and a `&Datastore`. It is the entry point
  every *external* caller (SPARQL endpoint, CLI, notebook kernel) goes
  through, but not the only call site internally: `sparql_endpoint`'s own
  SPARQL Update implementation (`sparql_update.rs::eval_where_pattern`,
  confirmed at `sparql_update.rs:1239-1247`) builds a synthetic
  `SELECT * WHERE { <update's WHERE pattern> }` query string and calls
  `sparql_parser::execute()` directly to evaluate an `INSERT`/`DELETE ...
  WHERE` clause's pattern. This is actually good news for where the
  enforcement hook belongs: if it lives *inside* `execute`/
  `execute_with_base`/`execute_inner` itself (e.g. gated by an optional
  `access: Option<&AccessScope>` parameter threaded through `execute_inner`)
  rather than as a separate pre-pass each external caller must remember to
  invoke, this internal call site inherits it automatically, with no
  separate Update-specific wiring needed for the read side. (The *write*
  side of Update — the templates, materialised separately via
  `parse_template`/`materialise_template`/`ground_quad`, never going
  through `execute()` at all — still needs its own explicit check; see
  below.) There is no later "apply security after the fact" hook point
  once `execute_inner` has returned — enforcement has to live at or before
  the point quoted here, not after.
- `ast::QueryComponent` (`ast.rs:87-110`, read in full) is a 12-variant
  enum: `BGP`, `PathPattern`, `Subquery(Box<Query>)`, `Optional`, `Union`,
  `Filter`, `Bind`, `Values`, `Minus`, `Graph(Term, Vec<QueryComponent>)`,
  `Service`, `Group`. Every variant that nests `QueryComponent`s
  (`Optional`/`Union`/`Minus`/`Graph`/`Service`/`Group`, and `Subquery`'s own
  `where_clause`) can contain a further `Graph`/`DatasetClause` arbitrarily
  deep — so a rewrite pass must **recurse** through the whole tree, not
  just the top-level `where_clause` list. It must also reach into
  `ast::Expression::Exists`/`NotExists` (`ast.rs:144-145`), which each carry
  their own `Vec<QueryComponent>` — a `FILTER EXISTS { GRAPH <denied> {...} }`
  can leak a yes/no membership bit about denied-graph content through a
  boolean filter result even if the matched rows themselves never appear in
  the output, so these need the same graph-scope check as ordinary
  `QueryComponent::Graph` nodes, not just a skip.
- `ast::DatasetClause` (`ast.rs:41-45`) has two variants, but they are
  **not symmetric today**: `Default(iri)` (`FROM <iri>`) is read by
  `dataset_active_graph` (`execute/mod.rs:780-789`) — the first `Default`
  clause present *retargets the active default graph itself* to `<iri>`,
  overriding graph 0, so it's a real access-scope lever a rewrite must
  check. `Named(iri)` (`FROM NAMED`) is a different story: it is
  constructed by the parser (`lib.rs:2467`) but **grepping its only other
  reference in the whole `sparql_parser` crate shows zero consumers** — no
  evaluation code anywhere reads `DatasetClause::Named` to restrict which
  named graphs a `GRAPH <iri>`/`GRAPH ?g` pattern may reach. In other
  words, `FROM NAMED` is parsed for conformance but currently has no
  runtime effect at all: a query can `GRAPH <any-graph-in-the-store>`
  regardless of whether that graph was ever declared via `FROM NAMED`.
  This means `Named` entries are **not** a usable access-restriction lever
  today, and the doc's earlier framing of checking "`FROM NAMED` and
  `Graph(term, ...)`" as parallel controls was wrong — the actual control
  points are the `Graph` terms themselves (bound-IRI and unbound-variable,
  both covered below) and `DatasetClause::Default`. Whether `FROM NAMED`
  should start being enforced as an *additional*, narrower allow-list on
  top of the identity's own grants is a reasonable phase-1 design
  question, but it is a new behavior to design, not an existing lever to
  reuse.
- Confirmed via the same function: with **no** `FROM` clause at all, the
  default graph is `ActiveGraph::Fixed(DEFAULT_GRAPH_ELEMENT_ID)` — i.e. a
  bare BGP with no `GRAPH`/`FROM` only ever matches graph 0, never a union
  of every graph in the store. This removes one plausible leak path (no
  "no GRAPH clause silently means everything") but makes `FROM <iri>`
  itself the thing to check, as above.
- Confirmed via `execute/components.rs:529-553` and `bgp.rs:172-253`: an
  **unbound** `GRAPH ?g` (`Term::Variable(v)` not yet bound in the current
  row) does *not* enumerate `distinct_graph_ids` and loop per graph the way
  property-path evaluation does (`paths.rs:130-139` does loop that way, for
  paths specifically) — ordinary BGP matching instead passes
  `ActiveGraph::Variable(v)` straight into the quad matcher, which matches
  with **no graph constraint at all** (`g = None` in `bgp.rs:172-174`,
  which `quads_matching` reads as "unconstrained wildcard" per its own
  doc comment) and binds `v` from whichever quad happened to match
  (`quad.triple_id`, `bgp.rs:244`). This matters for the cost model in §5:
  the match itself already touches every graph's matching quads, denied or
  not, before any `Filter` on `v` gets a chance to discard rows — so
  injecting a `Filter` after the `Graph` component, as #437's own body
  phrases it, is **correctness-sound but not actually a scan-pruning
  optimization** for this specific case; it is a post-filter wearing
  rewrite's clothing. By contrast, when `v` is *already bound* on entry to
  `Graph` (`components.rs:545-549`), the component takes the
  `ActiveGraph::Fixed(graph_id)` branch, which *is* a real per-graph
  lookup. So a `VALUES ?g { <allowed1> <allowed2> ... }` injected
  immediately **ahead of** the `GRAPH ?g { ... }` block (binding `g` before
  the `Graph` component runs, rather than filtering after) is the version
  that can take the cheaper, actually-pruning path; a plain `Filter`
  injection cannot. Always inject `VALUES` this way for every unbound
  `GRAPH ?g`, rather than trying to statically decide "is `g` already
  bound" ahead of time (the rewrite pass would need its own binding-
  liveness analysis to do that safely, which is unjustified complexity
  when unconditional `VALUES`-injection is correct either way — if `g`
  turns out to already be bound by the time evaluation reaches it, the
  injected `VALUES` simply joins against a singleton-compatible set and
  is a no-op). One caveat worth flagging for the phase-1 sub-issue to pin
  with a real test rather than assume: `component_ordering.rs` reorders
  components for join-cost reasons, so it must be confirmed (or taught)
  that it never reorders an access-control `VALUES` to *after* the `Graph`
  it was injected to protect — correctness survives such a reorder either
  way (the join still restricts the final result), but the pruning
  benefit this paragraph argues for would not.
- The executor already has three real passes over the AST before
  evaluation reaches the data: `component_ordering.rs` (join-order/cost
  estimation, which inspects `QueryComponent::Graph` to estimate selectivity
  — `component_ordering.rs:349,434,503`), `cyclic_bgp.rs` (warns on cyclic
  BGP join graphs, also pattern-matches `Graph` — `cyclic_bgp.rs:135`), and
  `explain.rs` (static EXPLAIN plan rendering, also walks `Graph` —
  `explain.rs:328,543`). A security-rewrite pass fits this same "walk the
  `QueryComponent` tree once before evaluation" shape these three already
  use, rather than being a novel kind of pass.
- `GraphElementManager`/`QuadTable` have no notion of "current identity" —
  adding one would mean threading an identity/claims argument through every
  call site in `dag_rdf`, `datalog`, `sparql_parser`'s executor, the
  Datalog reasoner, JSON-LD and Turtle parsers, GSP handlers, etc. — i.e.
  every consumer of `Datastore`, not just SPARQL query execution.

### Recommendation: query-rewrite at the AST level, built into `execute`/`execute_with_base` itself

Query-rewrite wins on every axis checked against the real code, with one
correction from the first draft of this doc: it is **not** a single
enforcement point covering everything, because `QueryResult::Describe`'s
final step (`execute/mod.rs:434-461`) calls
`datastore.named_graphs.get_quads_with_subject(subject_id)` directly —
confirmed to take **no graph argument at all** — after using the rewritten
`where_clause` only to find candidate *resources*, not to scope which
graphs' triples about that resource get returned. So a rewrite applied only
to `where_clause` evaluation would correctly restrict which resources
DESCRIBE can discover, then leak **every** graph's triples about a
resource it was allowed to discover for other reasons — this needs its own
explicit graph-filter on the final output set, not just rewrite coverage
of the pattern that selects candidates. Tracked as its own phase-1 item
below, not glossed over.

- **Correct AST shape already exists.** `QueryComponent::Graph` and
  `DatasetClause::Default` are exactly what a rewrite needs to intersect
  against an allow-list for ordinary query evaluation; no new AST variant
  is required (though `DatasetClause::Named` currently has no runtime
  effect to piggyback on, per the finding above).
- **Fewer enforcement points than storage-level, even though not exactly
  one.** Storage-level enforcement would need to reach every `Datastore`
  consumer (reasoner, parsers, GSP, CONSTRUCT/DESCRIBE serialization) — a
  far larger and more error-prone surface than rewrite-plus-a-small-list-
  of-named-exceptions (DESCRIBE's final scan, Update's write templates,
  GSP's direct graph access — all enumerated in this section), mirroring
  the existing
  `component_ordering`/`cyclic_bgp`/`explain` passes.
- **Post-filter is strictly worse than rewrite here, not just different.**
  Filtering `SelectResult`/`ResolvedTriple` rows *after* `execute_inner` has
  already run means the engine still does full joins against denied graphs
  before discarding the result — the opposite of the performance goal in
  §5, and it does nothing for `CONSTRUCT`/`DESCRIBE`, which don't produce
  row-shaped results to filter the same way `SELECT` does. Rewrite denies
  access to data the engine never touches in the first place.
- **Precedent from `AUTH.md`'s own body-inspection design.** `auth.rs`
  already treats "a request's effective semantics aren't fully determined
  by surface syntax alone" as something to resolve *before* the operation
  runs (`detect_update_smuggling`, driven by PR #164's fix for update
  smuggled under a Read-classified `POST /sparql` — confirmed via this
  session's provenance query, see the PR's trace below). The same principle
  applies here: resolve the query's effective graph scope against the
  identity's permissions before `execute_with_base` runs, not after.

Concretely: a new pass, structurally sibling to `component_ordering`/
`cyclic_bgp`, **recursively** walks the parsed `Query`'s entire
`QueryComponent` tree (including into `Subquery`'s own `where_clause` and
into `Expression::Exists`/`NotExists` bodies reachable through `Filter`,
`Having`, and `Bind` expressions) and:

- For every `DatasetClause::Named` **and** `DatasetClause::Default` entry
  (`FROM NAMED`/`FROM`) and every `Graph(term, ...)` pattern where `term` is
  a bound IRI, reject the query (or silently drop the clause, depending on
  policy — see below) unless the IRI is in the identity's allowed-graph
  set.
- For `Graph(Term::Variable(v), inner)` where `v` is not already bound by
  an enclosing component, inject a `VALUES v { <allowed1> <allowed2> ... }`
  immediately before the `Graph` component in the same component list
  (not a trailing `Filter`) — per the cost-model finding above, this is
  what actually prunes the unconstrained-wildcard scan rather than
  discarding its output after the fact. If `v` happens to already be bound
  (e.g. joined from an earlier triple pattern), fall back to a `Filter`
  check and rely on it running against the much smaller "already matched
  this specific graph" result set.
- For the default graph case covered by `execute/mod.rs:780-789` (no
  `FROM` at all → graph 0 only), apply the identity's default-graph
  permission directly — this is a fixed, single check, not a rewrite,
  since the target graph is always 0 and never attacker-influenced.
- Inside every `Expression::Exists`/`NotExists` reachable anywhere an
  `Expression` can appear — not just `Filter`, but also `Bind`'s
  expression, `Having` (`Query::Select.having`), `ORDER BY`
  (`OrderCondition.expression`), `GROUP BY` (`GroupCondition.expr`), and
  projected expressions (`ProjectionElement::Expression`) — apply the
  identical recursive check to the nested `QueryComponent` list, since a
  denied `GRAPH` block reachable only through `EXISTS`/`NOT EXISTS` in any
  of these positions still leaks a boolean signal about denied-graph
  content otherwise.

This runs as part of `execute`/`execute_with_base`/`execute_inner` itself
(per the architectural note above — gated by an optional access-scope
parameter, defaulting to "no restriction" so every pre-existing caller that
passes none is unaffected), immediately before `component_ordering`'s own
pass, rather than as a separate module external callers must remember to
invoke — this is what makes `eval_where_pattern`'s direct `execute()` call
(SPARQL Update's read side) and `run_transactional_query` (confirmed at
`query.rs:84` as a second, separate query-evaluation call site) inherit it
automatically instead of needing their own, easy-to-forget copy of the
same check. So the cost model in §5 is "one more linear-in-AST-size pass
over a tree that's already walked 2-3 times
for other static analysis", not a new per-row runtime check — with the
caveat above that an unbound `GRAPH ?g` needs `VALUES`-injection, not
`Filter`-injection, to actually realize that "no extra scan cost" property
rather than merely being correct.

### Other enforcement points this doc must not paper over

Query-rewrite covers `SELECT`/`ASK`/`CONSTRUCT`/`DESCRIBE` evaluated
through `sparql_parser::execute`. Several other paths reach `Datastore`
without going through this AST at all and need their **own**, much
smaller, enforcement points — checked directly against the identity's
allowed-graph set rather than via the rewrite pass:

- **`DESCRIBE`'s final output step is a confirmed leak, not a hypothetical
  one** — see the correction at the top of this section's recommendation
  below: `get_quads_with_subject` has no graph parameter, so once a
  resource is a "candidate" (found via the rewrite-covered `where_clause`,
  or named directly in `DESCRIBE <iri>`), every graph's triples about it
  are returned. This needs an explicit allowed-graph filter on
  `output`/`ResolvedTriple` right there in `execute/mod.rs`'s `Describe`
  arm — the one place in this doc where a post-filter genuinely is the
  right shape, because there is no pattern to rewrite against; the leak is
  in a direct store call, not AST evaluation.
- **SPARQL Update** (`sparql_endpoint::sparql_update`, confirmed via
  `UpdateOp`'s actual variants, `sparql_update.rs:62-120`): `InsertWhere`/
  `DeleteWhere`/`DeleteInsertWhere` each carry a `pattern: String` (the
  `WHERE` clause). Confirmed via `eval_where_pattern`
  (`sparql_update.rs:1234-1255`): this is wrapped as a synthetic
  `SELECT * WHERE { <pattern> }` string and passed straight to
  `sparql_parser::execute()` — the same call every other query goes
  through — so if the enforcement hook lives inside `execute`/
  `execute_with_base` itself (per the architectural note above), this read
  side is covered automatically, no Update-specific wiring needed. The
  *write* side is a separate, confirmed gap: `PreparedOp::PatternUpdate`
  (`sparql_update.rs:1071-1117`) materialises `delete_template`/
  `insert_template` via `parse_template`/`materialise_template`, which
  never calls `execute()` or goes anywhere near the rewrite pass, then
  writes the result straight into `store` via `ground_quad`. So
  `DELETE { GRAPH <allowed> {...} } INSERT { GRAPH <allowed> {...} } WHERE
  { GRAPH <denied> {...} }` copies denied-graph content into an allowed
  graph today's code would do nothing to stop — this write side needs its
  own explicit check (every graph IRI appearing in a template, checked
  against the identity's `write_graphs`) independent of the read-side fix.
  `ClearGraph(iri)`/`DropGraph(iri)`/`CreateGraph(iri)` are single-graph
  operations — a direct allowed-graph check, no AST walk needed.
  `ClearAll`/`DropAll`/`ClearNamed`/`DropNamed` are **all-graphs**
  operations with no natural way to scope to "all graphs this identity may
  write" short of iterating every named graph and checking each — flag
  this as a real open sub-question for phase 1's sub-issue (most likely
  answer: deny these outright under fine-grained ACL, require the identity
  to iterate `DropGraph` per graph instead). `LoadGraph` is parsed but not
  executed today (its own doc comment at `sparql_update.rs:88-92`:
  "Parsed for syntax conformance; execution ... is not implemented") — not
  a concern yet.
- **Other write endpoints not yet named above, confirmed from
  `server.rs`'s route list**: `POST /upload` (`upload::upload_turtle`,
  bulk Turtle ingest — needs a target-graph check same shape as GSP);
  `POST /{name}/rml` (RML mapping execution — the mapping itself can
  target arbitrary graphs, so needs a check on every graph IRI its output
  touches, not just the dataset name); `POST /{name}/ottr` (OTTR
  expansion — same shape as RML); `POST /transaction/{txId}/commit`
  (applies a transaction's buffered delta to the live store — needs the
  same write-side graph check as the operations that built that delta,
  applied at commit time since that's when it actually lands).
- **Graph Store Protocol** (`/rdf-graph-store`, `/rdf-graphs/{*path}`,
  `/{name}/data`, `/{name}/get` — `server.rs:216-320`): these identify a
  single target graph directly (query parameter or path), with no SPARQL
  AST at all. A direct check against the identity's allowed-graph set at
  each handler, parallel to `classify()`'s own method/path dispatch, not a
  reuse of §2's rewrite.
- **`/{name}/rules`, `/{name}/rules/{ruleset_id}`** (`server.rs:295-304`):
  confirmed these have **no explicit branch in `auth::classify()`** at
  all — `POST /{name}/rules` falls through every named check to the
  fail-closed `_ => Permission::Admin` default (`auth.rs:129-132`), and
  `DELETE /{name}/rules/{ruleset_id}` hits the generic "all PUT/DELETE
  except admin paths already matched → Write" branch (`auth.rs:91-93`)
  before reaching that default. Both classifications are accidents of
  `classify()`'s fallback ordering, not a deliberate decision that
  authoring a runtime ruleset should be `Admin`-gated — worth an explicit
  `classify()` branch and a comment recording the decision once this
  area is touched, especially given the inference-leak risk noted below
  makes rule authorship a genuinely higher-stakes operation than an
  accidental default should be deciding.
- **Other read endpoints confirmed from `server.rs`/`query.rs` not yet
  named above**: `run_transactional_query` (`query.rs:84`, a second query
  evaluation call site alongside the ordinary one — needs the same
  enforcement the ordinary path gets, i.e. covered for free if the hook
  lives inside `execute`/`execute_with_base` per the architectural note,
  confirmed not yet checked whether it does); `GET /vqs/productive-values`
  (query-builder support — reads the store to suggest completions, a
  smaller-scope version of the same leak class as VoID below).
  `GET /ns/agentprov/session` is **not** in scope here — it is deliberately
  carved out of `forward_auth` at the Caddy edge specifically because it
  only ever exposes public `session:*` triples by design (confirmed via
  `server.rs`'s own comment at that route), the same treatment as #441's
  `/ns/*` vocabulary routes; exempt, not an oversight.
- **`/describe`, `/id/*`** (`server.rs:166-174`, confirmed `Permission::Read`
  via `#493`'s own test comments): resource description can span multiple
  graphs for one resource — needs to intersect against the allowed-graph
  set per matched triple, same fix as `DESCRIBE`'s leak above (this
  endpoint and SPARQL `DESCRIBE` likely share, or should share, the exact
  same underlying fix).
- **VoID** (`/.well-known/void`, `/void` → `void::void_handler`): confirmed
  via `void.rs:26` that the triple count statistic is computed over
  `store.named_graphs.quad_list.len()` directly, the **whole** store with
  no graph scoping at all. Dataset-wide statistics leak the existence/size
  of denied graphs across the ACL boundary today and would need to become
  allowed-graph-scoped once this feature exists — flag as a phase-1
  follow-up, not phase 0 scope creep, since VoID has no graph-scoping
  concept to retrofit onto yet.
- **`/{name}/shacl`**: a validation report can echo matched triples from
  the dataset being validated — whether this needs scoping depends on
  whether the dataset-level/graph-level boundary set by earlier layers
  already excludes denied graphs from "the dataset" as `shacl::validate`
  sees it; flag as an open question for the phase-1 sub-issue rather than
  assume either answer here.
- **`?explain=true`** (`sparql_endpoint/src/explain.rs`, driven from
  `query.rs:77-87`): if the rewrite is built into `execute`/
  `execute_with_base` per the architectural note above, EXPLAIN's
  cardinality estimates are computed against the already-rewritten
  (denied-graph-excluded) AST by construction, since `component_ordering`/
  `explain.rs` run inside the same call — worth a dedicated test once
  built regardless, since this is exactly the kind of invariant that
  silently breaks if the enforcement hook's placement ever moves.

### A dagalog-specific leak this doc flags as an open risk, not yet resolved: inference across the ACL boundary

`datalog::types::QuadWildcard` (`types.rs:24-30`) and `RuleHead::NormalHead`
(`types.rs:35-37`) both carry a `graph: ResourceOrWildcard` — meaning a
rule's **head** can in principle write a derived quad into any graph while
its **body** reads from a different (possibly-denied) graph. Checked how
much of this is actually reachable today, rather than leaving it
theoretical: every built-in OWL 2 RL rule generated by `owl2rl2datalog`
(confirmed across `equality.rs`, `lib.rs`, and every other rule-construction
site in that crate) builds both head and body patterns exclusively via
`dag_rdf::query::get_default_graph_pattern` — i.e. **built-in OWL-RL
reasoning is hard-pinned to graph 0 everywhere** and cannot itself cause
this leak. The reachable case is **user-authored runtime rules**: the
`datalog_parser` grammar (`lib.rs:45-46,484-505`) explicitly supports a
`?graph` variable in a rule's named-graph suffix, in both body and head
position, independently — confirmed via its own test
(`named_graph_variable`, `lib.rs:929-939`) and doc comment example — so a
rule like `[?s, ex:p, ?o] <allowed-graph> :- ex:p[?s, ?o] ?g .` (fixed head
graph, variable/unconstrained body graph) is expressible today through
whatever ingests custom rulesets (the `/{name}/rules`/`/{name}/rules/{id}`
endpoints noted above). Reasoning (`DatalogProgram`'s forward-chaining
materialization) runs once, at data-load/ruleset-registration time,
**before any per-request `Identity` exists** — by the time a user's query
reaches the rewrite pass above, such a rule could already have merged
denied-graph facts into an allowed graph as an ordinary derived fact
indistinguishable from any other triple there. Query-time enforcement
(everything above) **cannot** undo this after the fact. This is specific
to dagalog's choice to feed OWL-RL axiom rules into the same
forward-chaining `DatalogProgram` as user rules rather than running
reasoning as a separate, non-extensible eager pass, and wouldn't occur in
a triplestore where reasoning and querying are the same step or where
rules can't be registered by end users at all. It needs its own
resolution, most likely enforced at **rule-authoring time**: the
`classify()` gap already flagged above (`/{name}/rules` landing on
`Admin` only by fallback accident) is exactly the lever this would use
once deliberate — a check at rule-registration time that a rule's body and
head don't together span a graph pair the registering identity isn't
itself allowed to merge. Left as an explicitly flagged open question for
whoever picks up the phase-1 sub-issue to resolve with Dag before
implementation, not something this doc resolves unilaterally.

## 3. Policy model: RBAC vs. ABAC vs. RDF-native

### What the code actually does (ingredients, not an existing policy engine)

- `auth.rs::Claims` (`auth.rs:170-199`) already does arbitrary dot-path claim
  extraction (`extract_roles("realm_access.roles")`) and stores the full
  decoded JWT as a flat `HashMap<String, serde_json::Value>` — this is
  already "ABAC-shaped" in miniature: any claim, not just a fixed role
  list, can be read if a caller wants to.
- `shacl::validate(data: &Datastore, shapes: &Datastore)` (`shacl/src/lib.rs:214`)
  takes *shapes as their own `Datastore`* and validates *data as another
  `Datastore`* against it, producing a `ValidationReport`
  (`ValidationResult`/`Severity`, `lib.rs:51-214`). This is precisely the
  "policy-as-data, queryable like everything else" pattern #437's body
  floats for RDF-native ABAC: shapes are ordinary triples in a `Datastore`,
  validated by ordinary dagalog machinery, no bespoke policy-rule parser.
  It's a strong structural precedent, but it's a *validation* report (does
  data conform to a shape), not an *authorization* decision (may identity X
  read/write graph G) — reusing it for ACL would mean writing new shape
  vocabulary and a new call site, not reusing `validate()`'s own semantics
  directly.
- There is no existing "permission" vocabulary/ontology in this repo (no
  `ingress::namespaces.rs` constant for anything ACL-shaped) and no code
  that currently reads policy triples at query time.

### Recommendation: RBAC first (reusing `read_role`/`write_role`/`admin_role`
extended per-graph), RDF-native as the natural phase 2, ABAC only if a
concrete need appears

Phase 1 (cheapest, reuses the most existing code): extend `OidcConfig`'s
role-claim model from a single global `read_role`/`write_role`/`admin_role`
to a **configured role-name → graph-IRI(-set) mapping** (e.g.
`graph_grants: HashMap<String, Vec<String>>`, role name → the full graph
IRIs it grants read/write on — not a `dagalog.read.<local-name>` naming
*convention* baked into the role string itself, since two graphs in
different namespaces can share a local name, and IRIs don't embed cleanly
into typical identity-provider role-name strings anyway), alongside the
existing global roles (global role still grants access to every graph, for
backward compatibility with existing deployments and `AuthConfig::ApiKey`,
which has no claims to scope by at all). This needs:
- No new storage for the *token* side: the policy input is still the
  identity's token, nothing new is persisted dagalog-side beyond the small
  server-side `graph_grants` config mapping role names to graph IRIs.
- No new query-time claim-extraction logic: `claims.has_role(roles_claim,
  role_name)` is the same `extract_roles`/`has_role` code path already in
  `auth.rs` — only one more lookup (`graph_grants[role_name]` → the graph
  IRI set) is new.
- A real limitation worth stating up front: this only scales to as many
  distinct roles as the identity provider's role-assignment UI tolerates
  (fine for the handful of graphs in the motivating #437/backlog-dashboard
  use case; not a fit for, say, per-user private graphs numbering in the
  thousands).

Phase 2 (the "arguably most dagalog-native" option #437's body names, and a
real differentiator vs. "generic triplestore ACL"): an **RDF-native policy
vocabulary**, permission triples living in their own named graph inside the
same `Datastore` the data lives in, e.g. (illustrative, not final — a real
vocabulary needs its own `ingress::namespaces.rs` constants and design pass):

```turtle
<urn:dagalog:policy:1> a acl:Authorization ;
    acl:agentClass <https://dagalog.no/roles#backlog-viewer> ;
    acl:accessTo <https://dagalog.no/graphs/backlog> ;
    acl:mode acl:Read .
```

This is strictly more expressive than phase-1 RBAC (can express "this role
may read graph G but not graph H", composable with the Datalog reasoner for
derived grants — e.g. "admin of parent graph implies read of child graph" as
an ordinary rule) and is genuinely query-time inspectable the way #437's
body asks for ("queryable like everything else dagalog does") — an admin
could `SELECT` the current effective policy. It is **not** phase 1 because
it needs: a resolution step mapping a verified identity → the `acl:agentClass`
it matches (itself a design question — direct claim-to-role triples? a
join against an identity-to-role graph maintained out-of-band?), and new
query-time machinery to evaluate it before §2's rewrite pass can use it.
Scope this as its own sub-issue once phase 1 ships and the per-graph-role
naming convention's scaling limit is actually hit.

ABAC (arbitrary claim-based rules, e.g. "department claim must equal
graph's `dc:subject`") is **not recommended as a separate phase** — nothing
in #437's motivating use case (a handful of named graphs, a handful of
users) needs attribute-matching beyond what phase-1 role-per-graph or
phase-2 RDF-native `acl:agentClass` already covers, and ABAC's generality
is exactly the kind of "generic triplestore ACL theory" #437 asks this doc
to avoid defaulting to. Keep it in mind only as a reason phase 2's
`acl:agentClass` resolution step should stay pluggable (not hard-code "class
== role name") rather than as its own implementation phase.

## 4. Identity source: identity-source-agnostic by construction

### What the code actually does

- `auth_middleware` (`auth.rs:590`) only ever produces one shape of identity
  today: a verified `Claims` struct (flat `HashMap<String,
  serde_json::Value>`) from `AuthConfig::Oidc`, or nothing at all for
  `AuthConfig::None`/`ApiKey` (API-key mode has no identity concept — it's a
  single shared secret, not per-user). Nothing downstream of
  `auth_middleware` currently receives or inspects an identity — `Claims` is
  checked and discarded inside the middleware; handlers never see it
  (confirmed: no `Extension<Claims>` extraction anywhere outside `auth.rs`
  itself, unlike `AUTH.md`'s own Tier-2 pseudocode sketch, which was never
  actually built that way).
- The edge-proxy deployment (`PUBLIC_DEPLOYMENT.md`) produces identity in a
  *third*, completely different shape: an HTTP header forwarded by
  oauth2-proxy (email-allowlist check already done upstream, by software
  dagalog never sees the internals of) — dagalog today does not read this
  header at all; the edge proxy's own allowlist is the only check.

### Recommendation: a small internal `Identity` type, populated from either source, consumed only by §2's rewrite pass

Introduce one small struct, independent of how it was obtained:

```rust
pub struct Identity {
    /// Opaque subject identifier (JWT `sub`, or the forwarded email claim).
    pub subject: String,
    /// Allowed graph IRIs, already resolved by whichever auth layer
    /// authenticated this request (phase-1 RBAC: via `graph_grants`
    /// role-name → graph-IRI mapping; phase-2: resolved `acl:agentClass`
    /// grants) — §2's rewrite pass consumes this directly as the
    /// allow-list, independent of which policy model produced it.
    pub read_graphs: Vec<String>,
    pub write_graphs: Vec<String>,
}
```

Two population paths, both producing the same `Identity`:
- **In-app OIDC (`AuthConfig::Oidc`):** `auth_middleware` already decodes
  and validates `Claims` — extend it to also build an `Identity` via
  `extract_roles` plus the `graph_grants` mapping (§3) and attach it as
  `Extension<Identity>` so handlers (query execution) can read it. This is
  additive to existing `auth.rs` code, not a rewrite of it.
- **Edge-proxy-forwarded header:** a new, much smaller middleware (or a
  branch inside the existing one) that trusts a configured header name —
  confirmed via `deploy/Caddyfile:128` that oauth2-proxy's own
  `forward_auth`/`copy_headers` directive forwards `X-Auth-Request-User`
  and `X-Auth-Request-Email` specifically — reuse those exact header
  names, *only when explicitly configured to* (never trust a
  client-settable header by default — this
  must be an opt-in config flag, documented as "only enable behind a
  proxy that strips/overwrites this header before forwarding",
  exactly the trust boundary `PUBLIC_DEPLOYMENT.md`'s Caddy config already
  establishes for the dashboard). The resulting `Identity.subject` is the
  forwarded email; `read_graphs`/`write_graphs` come from a small,
  separately configured subject → graph-grants mapping, since a forwarded
  email carries no role claims at all — this is the same gap
  `PUBLIC_DEPLOYMENT.md` already documents for why Google personal
  accounts don't carry a `roles` claim.

Because §2's rewrite pass and §3's policy evaluation only ever consume
`Identity.read_graphs`/`write_graphs` (plain graph-IRI lists), neither
cares which path produced them — this is what makes the design
identity-source-agnostic without a trait/plugin abstraction that would be
speculative generality for two known sources.
Adding a third source later (e.g. a future in-app Google Identity Services
flow, noted as unbuilt in `PUBLIC_DEPLOYMENT.md`) means adding a third way
to populate `Identity`, not touching the rewrite/policy code at all.

## 5. Performance: cost model and non-regression

### What the code actually does

- `tests/performance.rs` and the DBLP benchmarks exercise `execute_with_base`
  directly against large datasets; `sparql_parser`'s join-reordering
  (`component_ordering.rs`) already does a static cost-estimation pass over
  the parsed AST *before* evaluation, specifically to avoid bad join orders
  — i.e. "one more static AST pass before evaluation" is a cost category
  this codebase already budgets for and measures, not a new one.
  `explain.rs`'s EXPLAIN endpoint (#537/#572) exists precisely to make these
  static-vs-runtime costs visible, which gives this feature a ready-made way
  to show its own overhead once built.

### Recommendation: bound the cost to "one linear AST pass, no extra store scans," and make it measurable via the existing EXPLAIN machinery

- The rewrite pass from §2 is `O(size of parsed query AST)` — it walks
  `QueryComponent`s once, matching the existing `component_ordering`/
  `cyclic_bgp`/`explain` passes' own cost shape. It does **not** touch
  `QuadTable`/`Datastore` at all (no store scan to determine allowed graphs
  — that comes entirely from `Identity.read_graphs`/`write_graphs`, already
  resolved before the query reaches the parser), so there is no risk of it
  scaling with dataset
  size, only with query AST size (bounded, already true of every other
  static pass).
  - Phase-2 RDF-native policy (§3) is the one exception: resolving
    `acl:agentClass` against policy triples **does** need a `Datastore` read.
    Scope it to touch only the (small, admin-maintained) policy graph, never
    the main data graphs being queried, and cache the resolved
    identity→allowed-graphs set for the lifetime of a single request (or
    short-TTL, mirroring `JwksCache`'s existing TTL-cache pattern in
    `auth.rs:320-409`) rather than re-querying per sub-request.
- Add the rewrite pass as its own node in `explain.rs`'s `PlanNode` tree
  (the way `QueryComponent::Graph` already gets its own `PlanNode::Graph`,
  `explain.rs:328`) once phase 1 lands, so its actual overhead is visible
  in EXPLAIN output rather than asserted in prose — this closes the loop
  the issue's own performance concern asks for: a cost model that can be
  checked, not just argued.
- No change needed to `tests/performance.rs`'s existing DBLP benchmarks for
  phase 1 (no auth configured in those benchmarks today, so the rewrite
  pass simply doesn't run — `AuthConfig::None` should remain a true no-op,
  exactly like `auth_middleware`'s existing `matches!(state.config.auth,
  AuthConfig::None)` fast path at `auth.rs:602`). A new benchmark variant
  *with* `AuthConfig::Oidc` + a representative grant set should be added as
  part of phase 1's implementation sub-issue, to pin the "no measurable
  regression when access control is active" claim rather than leave it
  asserted.

## Phased implementation breakdown

Each phase below is sized to become one or more concrete sub-issues under
epic #437 once reviewed.

**Phase 1 — Per-graph RBAC + query-rewrite enforcement (the MVP)**
- Add the `graph_grants: HashMap<String, Vec<String>>` config mapping (role
  name → granted graph IRIs) to `OidcConfig`, and extend `Claims`-checking
  to resolve an identity's roles into a `read_graphs`/`write_graphs` set via
  it, alongside the existing global roles (`sparql_endpoint/src/auth.rs`).
- Introduce the `Identity` struct (§4) and attach it via
  `Extension<Identity>` from `auth_middleware`.
- Build the **recursive** AST rewrite pass (§2) as a new `sparql_parser`
  module, invoked between parsing and `component_ordering`/
  `execute_with_base`, covering: `DatasetClause::Named` and `::Default`,
  bound-IRI `QueryComponent::Graph` (at any nesting depth, including inside
  `Subquery`), variable-bound `QueryComponent::Graph` (prefer `VALUES`-
  injection ahead of the component over `Filter`-injection after it — see
  §2's cost-model finding), and `Expression::Exists`/`NotExists` bodies.
  Decide and document the default-graph policy explicitly (deny-by-default
  vs. implicit scoping — a real open sub-question this doc intentionally
  leaves to the sub-issue, since it depends on how Dag wants
  `AuthConfig::None`'s "everything open" default to interact with a
  newly-opt-in feature).
- Wire SPARQL Update (`sparql_update.rs`'s `InsertWhere`/`DeleteWhere`/
  `DeleteInsertWhere`) through the same rewrite on their `pattern` (read
  side) **and** add an explicit write-side check on every graph IRI named
  in their templates — §2 flags the cross-graph-copy leak this must close.
  `ClearGraph`/`DropGraph`/`CreateGraph` get a direct single-graph check;
  resolve the `ClearAll`/`DropAll`/`ClearNamed`/`DropNamed` all-graphs
  question (§2) with Dag before implementing.
- GSP (`/rdf-graph-store`, `/rdf-graphs/{*path}`, `/{name}/data`,
  `/{name}/get`) needs the equivalent direct single-graph check at each
  handler (it bypasses the SPARQL parser's AST entirely — a second, much
  smaller enforcement point, not a reuse of §2's pass).
- Decide and resolve the `/describe`/`/id/*` (multi-graph DESCRIBE results)
  and VoID (whole-store statistics, §2) scoping questions flagged above.
- Add the new `explain.rs` `PlanNode` entry and the `AuthConfig::Oidc`
  performance-benchmark variant (§5).
- Tests: `auth.rs`-style unit tests for `graph_grants` resolution, new
  `sparql_parser` tests for the rewrite pass (one per `QueryComponent`
  variant it touches, plus nested/`EXISTS` cases), an Update cross-graph-
  copy regression test, and `sparql_endpoint` integration tests mirroring
  the existing `tests/auth.rs`/`tests/oidc_auth.rs` pattern but asserting
  graph-scoped 403s, not just route-level ones.
- Raise the inference-across-the-ACL-boundary question (§2's dagalog-
  specific risk) with Dag explicitly before this phase ships, even if the
  resolution is "out of scope for phase 1, tracked separately" — it must
  not be silently left unaddressed.

**Phase 2 — Edge-proxy-forwarded identity support**
- Add the opt-in forwarded-header `Identity` population path (§4), with its
  own config flag and documentation of the trust boundary it requires
  (only enable behind a proxy guaranteed to strip/overwrite the header).
- A small subject→grants mapping config (file or env, mirroring
  `authenticated-emails.txt`'s existing simplicity in
  `docs/deploy/PUBLIC_DEPLOYMENT.md`) since forwarded emails carry no role
  claims.
- Tests mirroring phase 1's but with the header-based `Identity` path.

**Phase 3 — RDF-native policy-as-data (`acl:`-shaped triples)**
- Design the actual vocabulary (own `ingress::namespaces.rs` constants,
  informed by existing `acl:`/W3C WAC conventions where they fit, deviating
  where dagalog's graph model needs something W3C WAC doesn't cover).
  Likely its own short design note before implementation, given this is the
  most novel piece.
- A resolution step: verified `Identity` (subject + whatever raw claims it
  carries) → matching `acl:agentClass`es → effective allowed-graph set,
  cached per-request/short-TTL (§5).
- Extend §2's rewrite pass to consult this resolved set instead of (or
  alongside) phase 1's `graph_grants` mapping — ideally without the rewrite
  pass itself needing to know which policy model produced the grant set
  (`Identity.read_graphs`/`write_graphs` stay the uniform interface from
  §4).
- A SHACL-shape-informed validation story for the policy triples themselves
  (malformed policy triples should fail closed, loudly, at load time —
  worth a direct look at reusing `shacl::validate` here, since the shapes
  infrastructure already exists).
- Tests: policy-triple fixtures exercising resolution, interaction with
  the Datalog reasoner (can a derived grant via an OWL-RL/datalog rule work
  correctly end-to-end), and the same graph-scoped-403 integration tests as
  phase 1 but driven by policy triples instead of JWT roles.

**Phase 4 (speculative, not scheduled) — triple-level granularity via
reification**
- Only pursue if a concrete use case outgrows graph-level partitioning.
  Build on `reified_triples` (§1) rather than changing `Quad`'s shape.
