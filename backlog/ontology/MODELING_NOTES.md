# Modeling notes for the dagalog backlog vocabulary

Resolves the open questions raised while writing the grounding fixtures
([PR #293](https://github.com/daghovland/rdf-datalog/pull/293)) for
[issue #283](https://github.com/daghovland/rdf-datalog/issues/283). The
formal vocabulary is [`vocabulary.ttl`](vocabulary.ttl) — this document is
the *why*, kept separate so the `.ttl` file's `rdfs:comment`s can stay short
and the reasoning here can be as long as it needs to be.

## Epic modeling: structural role, not a type

**Decision:** there is no `bl:Epic` class. An issue is an epic purely
structurally: it has no `bl:subIssueOf`, and it is the object of at least
one other issue's `bl:subIssueOf`. Equivalently, in SPARQL:

```sparql
SELECT ?epic WHERE {
  ?epic a bl:Issue .
  FILTER NOT EXISTS { ?epic bl:subIssueOf ?anyParent }
  FILTER EXISTS     { ?anyChild bl:subIssueOf ?epic }
}
```

**Why not `bl:Epic a owl:Class`, asserted directly on qualifying issues?**
Because it would be a second, separately-maintained source of truth that
could silently drift from the actual `bl:subIssueOf` graph — e.g. a loader
bug that fails to assert `bl:Epic` on a genuinely-childless-but-parentless
issue would go undetected by anything checking `rdf:type bl:Epic`, since the
type triple itself would just be missing, not wrong. Deriving it structurally
means there is only one fact to get right (`bl:subIssueOf` edges), not two.

**Trade-off acknowledged:** this makes "is this an epic" a join instead of a
type lookup, which is slower and slightly less ergonomic for SPARQL/SHACL
authors. Given the actual data size (this repo's backlog, not a
web-scale dataset), that cost is negligible — revisit only if this vocabulary
is ever reused somewhere the query cost genuinely matters.

## Workflow status as its own axis (`bl:status`, distinct from `bl:state`)

**Decision:** added `bl:status` (range `bl:WorkflowStatus`: `Todo` / `Ready` /
`InProgress` / `Done`) as a property separate from the pre-existing
`bl:state` (range `bl:IssueState`: `Open` / `Closed`).

**Why:** raised while comparing this vocabulary against Jira/Azure DevOps/
Kanban conventions (see [PR #293](https://github.com/daghovland/rdf-datalog/pull/293)'s
README discussion). Those systems all separate a raw open/closed bit from a
richer workflow-stage concept (Jira's To Do/In Progress/Done, Azure's board
columns) — and this repo already has that exact distinction in practice
(CLAUDE.md's "Implementation workflow": unlabeled = awaiting review, `ready`
label = approved, an in-progress convention signaled by a branch-name
comment, closed = done). The original fixtures only captured `bl:state`,
which would have made `bl:status` impossible to add later without either
overloading `bl:state` (conflating "still open" with "not started yet") or
overloading `bl:hasLabel` (making "ready" do double duty as both a literal
GitHub label *and* a workflow signal, when in this repo's practice
`InProgress` has no label at all — it's a branch-comment convention).

**Not populated for most fixture entities.** Detecting `bl:InProgress`
specifically requires recognizing a working-branch-comment convention, not
just reading a label — that's genuinely the loader's (#284) job, requiring
either comment-body pattern matching or a firmer future convention (e.g. a
dedicated label) to do reliably. Adding the class/property now, without
forcing every fixture to populate it accurately, keeps this ontology issue
from silently taking on #284's scope.

## DOAP vs. bespoke `bl:Crate`

**Decision:** reuse real [DOAP](https://github.com/ewilderj/doap/wiki)
(`doap:Project`) for the repository as a whole; keep a bespoke `bl:Crate` for
individual workspace-member crates.

**Why not push DOAP further, e.g. modeling each crate as its own
`doap:Project`?** DOAP's `Project` carries release/maintainer/download
metadata suited to an independently-shipped software project, not an
internal workspace module that's never published or versioned on its own —
forcing every crate into that shape would mean either leaving most DOAP
properties unpopulated (schema noise) or inventing meaning for them that
doesn't hold (e.g. what would a crate's own `doap:release` even mean, when
crates in this workspace don't have independent version numbers or
changelogs). A crate is closer to a build-system unit than a project.

**Why use DOAP at all, then, instead of a bespoke `bl:Project` too?**
Because the repository-as-a-whole genuinely *is* the kind of thing DOAP
was built to describe (a single, named, versioned, published software
project with a homepage and a primary language) — reusing an existing,
recognized vocabulary there costs nothing and buys interoperability with
any other DOAP-aware tooling, unlike the ticket-tracking half of this
ontology where no such broadly-adopted vocabulary exists to reuse. See
[`../examples/project_and_status.ttl`](../examples/project_and_status.ttl)
for a real instance.

## `rdfs:subClassOf` is not free: PRs need an explicit `rdf:type bl:Issue`

**Finding, made concrete while writing these notes:** declaring
`bl:PullRequest rdfs:subClassOf bl:Issue` in `vocabulary.ttl` does *not*
make a plain SPARQL query like `?x a bl:Issue` match a resource asserted
only as `a bl:PullRequest` — dagalog's SPARQL executor (like most, absent an
explicit reasoning pass) doesn't perform RDFS/OWL-RL entailment at query
time. Verified directly: before the fix below, `SELECT (COUNT(*) AS ?n)
WHERE { ?pr a bl:PullRequest . ?pr a bl:Issue . }` against these fixtures
returned `0`.

**Decision:** the fixtures now assert `a bl:PullRequest, bl:Issue` on every
PR individual, redundantly but explicitly — not relying on subclass
inference. **This is a requirement for #284 (the loader), not just a fixture
quirk**: it must emit both `rdf:type` triples for every PR it materializes,
or every "all Issues" query/shape in #285/#286 will silently miss every PR
unless something first runs this repo's own OWL-RL/RDFS reasoner
(`dagalog::apply_ontologies`) over the mirrored data — an extra, easy-to-forget
step for what's supposed to be a lightweight, always-fresh local mirror.

## What's still open (deliberately, past this issue's scope)

- Whether `bl:hasLabel` should eventually become resource-valued (one IRI per
  label, carrying color/description) rather than a plain string — deferred
  per the "deliberately minimal v1" framing in #282; revisit if label
  metadata beyond the name is ever actually needed by #285/#286.
- A `bl:blockedBy`/"in review" `bl:WorkflowStatus` value, if this repo's own
  practice ever grows one — not invented speculatively here.
