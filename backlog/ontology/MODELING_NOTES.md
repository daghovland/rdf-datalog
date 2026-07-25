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

**`bl:PullRequest`/`bl:Epic` disjointness:** not separately asserted.
`bl:Epic rdfs:subClassOf bl:Issue`, and `bl:Issue owl:disjointWith
bl:PullRequest` (see "`bl:Issue` and `bl:PullRequest` are disjoint" below),
so nothing being both a pull request and an epic is already entailed
through subsumption — asserting it again would be a redundant, not an
independent, fact.

## `bl:Issue` and `bl:PullRequest` are disjoint (via a common `bl:WorkItem`)

**Revised after a second round of review**, requested directly: a PR and an
issue are conceptually different things (a PR proposes and can merge/close
a code change; an issue reports or requests something) even though they
happen to share most of the same fields — the original `bl:PullRequest
rdfs:subClassOf bl:Issue` conflated "shares properties with" and "is a kind
of."

**Decision:** introduced `bl:WorkItem` as an abstract common superclass.
`bl:Issue` and `bl:PullRequest` are now disjoint siblings under it
(`bl:Issue owl:disjointWith bl:PullRequest`), and the properties that
genuinely apply to both (`bl:number`, `bl:state`, `bl:hasLabel`,
`bl:touchesCrate` — title uses `rdfs:label`, see "`rdfs:label` instead of a
bespoke `bl:title`" below) moved their `rdfs:domain` from `bl:Issue` to
`bl:WorkItem`. Properties that are genuinely issue-only (`bl:subIssueOf`) or
PR-only (`bl:closesIssue`, `bl:relatesToIssue`) keep their original,
narrower domain. `bl:status` also stays domain `bl:Issue`, not `bl:WorkItem`
— this repo's practice never gives a PR its own workflow stage distinct from
`bl:state` (a PR is open, then merged/closed; there's no separate
ready/in-progress pipeline for the PR itself, only for the issue(s) it
closes). `bl:Epic` stays `rdfs:subClassOf bl:Issue` (not `bl:WorkItem`) — an
epic is specifically a kind of issue, never a kind of pull request.

**Consequence for the fixtures:** every PR individual, which previously
carried `a bl:PullRequest, bl:Issue` (see "`rdfs:subClassOf` is not free"
below), now carries `a bl:PullRequest, bl:WorkItem` instead — asserting `a
bl:Issue` on a PR would now be a direct contradiction of the new
disjointness axiom. Every `bl:Issue` individual likewise now also carries
an explicit `a bl:WorkItem`, for the same "subclass inference doesn't fire
at query time" reason documented below.

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

## `rdfs:label` instead of a bespoke `bl:title`

**Decision:** there is no `bl:title` property. An issue/PR's title is given
directly by `rdfs:label` — RDFS's own "a human-readable name for the
subject" — which is exactly what a GitHub title is, and reusing it is
standard RDF practice (Wikidata, DBpedia, and most published vocabularies
use `rdfs:label` as the generic display name for both schema terms and
instance data, not just schema terms).

**Why not keep a bespoke property, given this vocabulary already
distinguishes controlled-vocabulary resources from plain literals
carefully elsewhere?** Because a separate `bl:title` would say nothing
`rdfs:label` doesn't already say — there's no domain-specific meaning to
"issue/PR title" beyond "the human-readable name of this resource", unlike
(for contrast) `bl:hasLabel`, which specifically means "carries this GitHub
label", a real domain concept `rdfs:label` doesn't capture.

**One thing to be aware of, not a real problem:** this vocabulary already
uses `rdfs:label` heavily as schema-level annotation on classes, properties,
and controlled-vocabulary individuals (e.g. `bl:Issue rdfs:label "Issue"`).
Reusing the same predicate for instance-level titles means one predicate
now serves both purposes in the same files. This is normal — `rdf:type`
always distinguishes a schema term from a `bl:Issue`/`bl:PullRequest`
instance if it ever matters for a query — but worth naming so a future
reader isn't surprised to see `rdfs:label` show up on both a `bl:Issue` and
an `owl:Class` in the same graph.

## Disjointness axioms

**Added after review**, in response to being asked directly whether the
ontology declared what *can't* overlap, not just what can. `vocabulary.ttl`
now asserts (via two `owl:AllDisjointClasses` groups, to avoid writing out
every pairwise combination by hand):

- `bl:WorkItem`, `bl:Crate`, `bl:IssueState`, `doap:Project`, and `bl:Label`
  are pairwise disjoint from each other (group 1).
- `bl:WorkItem`, `bl:Crate`, `bl:IssueState`, `doap:Project`, and
  `bl:WorkflowStatus` are pairwise disjoint from each other (group 2).
- `bl:Issue owl:disjointWith bl:PullRequest` — the load-bearing sibling
  disjointness under `bl:WorkItem`, see the section above.

**The one deliberate exception:** `bl:Label` and `bl:WorkflowStatus` are
never listed in the same `AllDisjointClasses` group, so nothing entails
disjointness between them specifically — required by `bl:Ready`'s
intentional dual-typing (see "Labels as resources" above). Every other pair
across all six top-level classes is disjoint. `bl:Issue`/`bl:PullRequest`
inherit disjointness from `bl:WorkItem` vs. the other four classes
automatically through subsumption, and `bl:Epic` inherits its disjointness
from `bl:PullRequest` the same way through `bl:Issue` (a reasoner running
full OWL-RL over this data would derive all of this; none of it is
separately re-asserted).

Verified directly (not just parsed): loaded these axioms plus the example
fixtures through dagalog's own SHACL endpoint and confirmed
`bl:EpicHasNoParentShape` both conforms on the real (compliant) fixture data
and correctly reports a violation (`sh:conforms false`, with the exact
offending `sh:focusNode`/`sh:value`) when a parent edge is deliberately
added to an epic in a scratch test.

## `rdfs:subClassOf` is not free: every instance needs an explicit `rdf:type` per level

**Finding, made concrete while writing these notes.** Originally observed
as: declaring `bl:PullRequest rdfs:subClassOf bl:Issue` does *not* make a
plain SPARQL query like `?x a bl:Issue` match a resource asserted only as
`a bl:PullRequest` — dagalog's SPARQL executor (like most, absent an
explicit reasoning pass) doesn't perform RDFS/OWL-RL entailment at query
time. Verified directly at the time: `SELECT (COUNT(*) AS ?n) WHERE { ?pr a
bl:PullRequest . ?pr a bl:Issue . }` against these fixtures returned `0`
before fixtures were fixed to dual-type.

**Still true after introducing `bl:WorkItem`, just at a different level.**
Now that `bl:Issue`/`bl:PullRequest` are disjoint siblings under
`bl:WorkItem` rather than one subclassing the other, every individual needs
an explicit `a bl:WorkItem` alongside its specific type
(`a bl:PullRequest, bl:WorkItem` or `a bl:Issue, bl:WorkItem`) for the same
reason — properties domained on `bl:WorkItem` (`bl:number`, `bl:state`,
`bl:hasLabel`, `bl:touchesCrate`) won't match a plain `?x a
bl:WorkItem` query otherwise. Verified again after the change: `SELECT
(COUNT(*) AS ?n) WHERE { ?w a bl:WorkItem }` returns 34 (23 issues + 11
PRs) with both explicit types present in the fixtures; it would return `0`
for either category if either's dual-typing were dropped.

**This is a requirement for #284 (the loader), not just a fixture quirk**:
it must emit `a bl:WorkItem` on every issue and PR it materializes,
alongside the more specific type, or every property/shape/query in
#285/#286 that's domained on `bl:WorkItem` will silently miss data — unless
something first runs this repo's own OWL-RL/RDFS reasoner
(`dagalog::apply_ontologies`) over the mirrored data, an extra,
easy-to-forget step for what's supposed to be a lightweight, always-fresh
local mirror.

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
  requiring every `bl:Issue` and `bl:PullRequest` to also carry
  `a bl:WorkItem` — enforcing the dual-typing requirement from
  "`rdfs:subClassOf` is not free" above, so a forgotten dual-type doesn't
  silently drop an issue or PR out of any `bl:WorkItem`-domained
  property/query.
