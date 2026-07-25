# Modeling notes for the dagalog backlog vocabulary

Resolves the open questions raised while writing the grounding fixtures
([PR #293](https://github.com/daghovland/rdf-datalog/pull/293)) for
[issue #283](https://github.com/daghovland/rdf-datalog/issues/283). The
formal vocabulary is [`vocabulary.ttl`](vocabulary.ttl) — this document is
the *why*, kept separate so the `.ttl` file's `rdfs:comment`s can stay short
and the reasoning here can be as long as it needs to be.

## Epic modeling: an asserted `bl:Epic` type, constrained by one SHACL shape

**Revised after review.** The first version of this document (and PR #294 as
originally opened) made "epic" a purely structural role — no `bl:Epic`
class; an issue was an epic if it had no `bl:subIssueOf` and was the target
of at least one other issue's. A subsequent design review (a second
Fable-5-backed pass specifically requested before merge, given how
expensive ontology changes are to walk back later) flagged the real problem
with that pattern: it silently assumes an **exactly-two-level** hierarchy.
GitHub's native sub-issue relation nests arbitrarily; a mid-tree issue (has
both a parent and children) matched neither the "epic" pattern (it has a
parent) nor a "leaf work item" pattern (it has children) under the old
rule, and would vanish from either kind of query with no signal that
anything was wrong.

**Decision:** `bl:Epic` is now `rdfs:subClassOf bl:Issue`, an ordinarily
asserted `rdf:type` — by the loader (#284) when it materializes an issue
that currently has sub-issues and no parent, or by hand in these fixtures.
The **one** hard constraint is enforced by
[`bl:EpicHasNoParentShape`](shapes.ttl): a `bl:Epic` must never carry
`bl:subIssueOf`. It is deliberately **not** required to have any
`bl:subIssueOf` children — a freshly-filed epic legitimately has zero
sub-issues until they're filed (e.g. #282 itself, briefly, when first
created) — and nothing here imposes a depth limit on the rest of the tree:
an Epic's children may themselves have further children; whether a
mid-tree node also gets asserted `bl:Epic` is left as a per-issue modeling
choice for whoever/whatever asserts the type, not something this class
definition dictates.

**Why a type + a narrow shape, rather than the loader-derived-cache idea the
review also raised (recompute `bl:Epic` from the edges on every load, same
single source of truth, just materialized for ergonomics)?** That's
actually very close to what's landed here — the type is still meant to be
*derived* by #284 from the same `bl:subIssueOf` edges, just asserted once at
load time rather than recomputed per-query. The difference from the
original structural proposal is narrow but important: the shape only checks
the one direction that's cheap and unambiguous to validate (an Epic has no
parent) rather than trying to define "epic-ness" as a two-sided pattern
match that breaks down at depth > 2.

**`bl:PullRequest`/`bl:Epic` disjointness:** since both are now sibling
subclasses of `bl:Issue`, `vocabulary.ttl` asserts
`bl:PullRequest owl:disjointWith bl:Epic` — nothing is both a pull request
and an epic. See "Disjointness axioms" below for the fuller set this PR
adds.

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

## Labels as resources, not strings

**Revised after review.** The original `bl:hasLabel` was `xsd:string`-valued
("deferred to v1", per the first version of this document). Review flagged
that this was an unprincipled inconsistency — `bl:state`/`bl:status` were
already modeled as controlled-vocabulary resources one section earlier, and
for one specific, present-tense case (not hypothetical) it created a real
duplication: the fixtures asserted **both** `bl:hasLabel "ready"` (a string)
**and** `bl:status bl:Ready` (a resource) on the same issues (#264, #266) —
the same real-world fact, recorded twice, in two different representations,
in two different files, with nothing tying them together.

**Decision:** `bl:hasLabel` is now `owl:ObjectProperty`-valued, range
`bl:Label`, with named individuals per label actually in use
(`bl:Bug`, `bl:Enhancement`, `bl:Ready`) — not an exhaustive enumeration of
every label this repo could ever use, just what these fixtures need; add
more as needed. Critically, **`bl:Ready` is deliberately typed as both
`bl:Label` and `bl:WorkflowStatus`** — one resource, referenced by both
`bl:hasLabel` and `bl:status`, rather than two separate individuals that
happen to mean the same thing. `bl:hasLabel bl:Ready` and
`bl:status bl:Ready` now point at the literal same IRI; there is nothing
left to drift apart. `#284` (the loader) should derive `bl:status` for any
value with a corresponding `bl:Label` (currently just `bl:Ready`) from the
matching `bl:hasLabel`, rather than asserting it as an independent fact.

**Consequence for disjointness:** because `bl:Ready` is intentionally in
both `bl:Label` and `bl:WorkflowStatus`, those two classes are the one
deliberate exception to the disjointness axioms below — see that section.

## Disjointness axioms

**Added after review**, in response to being asked directly whether the
ontology declared what *can't* overlap, not just what can. `vocabulary.ttl`
now asserts (via two `owl:AllDisjointClasses` groups, to avoid writing out
every pairwise combination by hand):

- `bl:Issue`, `bl:Crate`, `bl:IssueState`, `doap:Project`, and `bl:Label`
  are pairwise disjoint from each other (group 1).
- `bl:Issue`, `bl:Crate`, `bl:IssueState`, `doap:Project`, and
  `bl:WorkflowStatus` are pairwise disjoint from each other (group 2).
- `bl:PullRequest owl:disjointWith bl:Epic` (sibling subclasses of
  `bl:Issue` — see "Epic modeling" above).

**The one deliberate exception:** `bl:Label` and `bl:WorkflowStatus` are
never listed in the same `AllDisjointClasses` group, so nothing entails
disjointness between them specifically — required by `bl:Ready`'s
intentional dual-typing (see "Labels as resources" above). Every other pair
across all six top-level classes is disjoint. `bl:Epic`/`bl:PullRequest`
inherit disjointness from `bl:Issue` vs. the other five classes
automatically through subsumption (a reasoner running full OWL-RL over this
data would derive it; not separately re-asserted here).

Verified directly (not just parsed): loaded these axioms plus the example
fixtures through dagalog's own SHACL endpoint and confirmed
`bl:EpicHasNoParentShape` both conforms on the real (compliant) fixture data
and correctly reports a violation (`sh:conforms false`, with the exact
offending `sh:focusNode`/`sh:value`) when a parent edge is deliberately
added to an epic in a scratch test.

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

- `bl:Label` individuals carry only `rdfs:label` today, no color/description
  — revisit if that metadata is ever actually needed by #285/#286.
- A `bl:blockedBy`/"in review" `bl:WorkflowStatus` value, if this repo's own
  practice ever grows one — not invented speculatively here.
- A terminal non-completion `bl:WorkflowStatus` value (e.g. `WontFix`/
  `Duplicate`) for an issue closed without going through the
  Todo→Ready→InProgress→Done pipeline — `bl:Done` and raw `bl:state
  bl:Closed` currently leave that case with no coherent `bl:status` at all.
- A canonical-IRI rule for PRs (`/pull/N`, never `/issues/N`, even though
  GitHub serves the same PR under both) — worth pinning down explicitly
  before #284 is built, in case it ever reads the issues endpoint for
  anything and mints an IRI from that response instead.
- A placeholder-IRI convention (e.g. `urn:dagalog:draft:...`) for an issue
  drafted locally before it's actually filed on GitHub — relevant once #287
  (write-back) exists; every IRI in this ontology currently assumes a real
  `github.com` resource exists first.
- A SHACL shape (in #285's fuller shape library, not this narrow one)
  requiring every `bl:PullRequest` to also carry `a bl:Issue` — enforcing
  the dual-typing requirement from "`rdfs:subClassOf` is not free" below, so
  a forgotten dual-type doesn't silently under-count PRs out of "all
  Issues" queries.
