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

## An OWL `maxCardinality` restriction was attempted, then reverted (blocked on #298)

Raised directly: doesn't OWL already have a way to express "an issue with no
parent" as a class, via a cardinality restriction, rather than only via
SHACL? Yes — checked precisely, not just asserted from profile theory:
`bl:Epic rdfs:subClassOf [ a owl:Restriction ; owl:onProperty bl:subIssueOf ;
owl:maxCardinality "0"^^xsd:nonNegativeInteger ]` is a completely valid OWL
2 RL axiom, and (in the *checking*, not *defining*, direction — see
`eli/src/extractor.rs::eli_class_extractor`, which has no case for max
cardinality in sub-concept/defining position, only
`ObjectMinQualifiedCardinality` with cardinality exactly 1) is exactly the
kind of thing this repo's own OWL-RL translator is supposed to reason over
as a redundant, alongside-SHACL enforcement mechanism — the same pattern
already used for the disjointness axioms above.

**Reverted before merge.** Adding it and running it through real reasoning
(`--ontology backlog/ontology/vocabulary.ttl`, not just flat `--data`
loading, which is all this vocabulary had ever been exercised with before)
surfaced a genuine, unrelated bug: `eli/src/extractor.rs` translates
`ObjectMaxCardinality(0, prop)` in super-concept position by discarding
`prop` entirely and mapping straight to `owl:Nothing` —

```rust
ClassExpression::ObjectMaxCardinality(card, _prop) if *card == 0u32.into() => {
    (vec![], vec![], vec![NormalizedConcept::Bottom])
}
```

— which means the reasoner treats `C ⊑ ≤0 R` as "`C` is empty," unconditionally,
the instant anything is asserted `a C`, regardless of whether that instance
has any `R` edges at all. Compounded by `datalog/src/reasoner.rs:125`
`panic!`-ing (rather than returning a `Result::Err`) the moment any
contradiction is derived, this axiom crashed the whole program on the real
`valid_backlog_snapshot.ttl` fixtures the instant any of the four real
epics was loaded — confirmed by isolating the exact trigger (vocabulary
alone: fine; every other individual fixture file: fine; `valid_backlog_snapshot.ttl`,
the only file with `a bl:Epic` instances: panics) before concluding this
wasn't a mistake in the axiom itself.

Filed as [#298](https://github.com/daghovland/rdf-datalog/issues/298)
(unlabeled, awaiting review — not this ontology issue's job to fix a
reasoner bug). The `owl:Restriction` is **not** in `vocabulary.ttl` today;
add it back once #298 is fixed. Until then, `bl:EpicHasNoParentShape`
(SHACL, unaffected by this bug since it never invokes OWL-RL reasoning) is
the sole actually-enforced mechanism for this constraint — which was
already true in practice even before this attempt, since nothing in this
epic's tooling runs `apply_ontologies` over the backlog data anyway.

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

## The #285 shape library (`shapes.ttl`)

Six shapes, enacting CLAUDE.md's own backlog policy and the structural
invariants this document establishes but the RDFS/OWL class axioms alone
can't enforce (see "`rdfs:subClassOf` is not free" above):

- `bl:EpicHasNoParentShape` (from #294) — an Epic never carries
  `bl:subIssueOf`.
- `bl:IssueIsEpicXorHasParentShape` — every Issue is either an Epic or has
  exactly one `bl:subIssueOf` parent (CLAUDE.md's actual policy sentence).
  Combined with the shape above, this is effectively XOR in practice, even
  though `sh:or` alone only expresses "at least one" — the two branches are
  mutually exclusive by construction, not by an explicit XOR constraint.
- `bl:RequiresWorkItemTypeShape` — every `bl:Issue`/`bl:PullRequest` also
  carries a literal `a bl:WorkItem` triple.
- `bl:IssueAndPullRequestMutuallyExclusiveShape` — practical enforcement of
  the `owl:disjointWith` axiom (which is never itself actively checked
  without running a reasoner).
- `bl:InProgressImpliesOpenShape` — `bl:status bl:InProgress` and
  `bl:state bl:Closed` can't both hold.
- `bl:WorkItemRequiredFieldsShape` — every work item has a title
  (`rdfs:label`), a `bl:number`, and a `bl:state`.

**Two items from #285's own issue text were deliberately NOT implemented as
literally worded**, since the current vocabulary doesn't model the fields
they'd need:
- "A `ready`-labeled issue must have a non-empty body" — there is no
  `bl:body`/description property in this ontology (out of the "deliberately
  minimal v1" scope from #282). Revisit once #284 (the loader) actually
  needs to capture issue body text for some other reason.
- "No issue should be both open and have an in-progress marker with no
  linked working branch" — there is no branch-link property either.
  `bl:InProgressImpliesOpenShape` above implements the closest checkable
  approximation with what's actually modeled (`bl:InProgress` implies
  `bl:Open`), not the literal "has no linked branch" check.

**Important finding, made concrete while verifying these shapes**:
`sh:class` in this repo's `shacl` crate correctly follows `rdfs:subClassOf`
per the SHACL spec (the #265/PR #290 fix from earlier in this epic's work)
— which means `sh:class bl:WorkItem` is a no-op check here: it's trivially
satisfied by anything typed `bl:Issue`/`bl:PullRequest` via subclass closure
alone, whether or not the literal `a bl:WorkItem` triple this repo's
plain-SPARQL layer actually needs (see "`rdfs:subClassOf` is not free"
above) is present. Verified empirically: an `sh:class`-based first draft of
`bl:RequiresWorkItemTypeShape` did NOT catch a deliberately-missing
`bl:WorkItem` type in a scratch test; switching to
`sh:property [ sh:path rdf:type ; sh:hasValue bl:WorkItem ]` (a raw
graph-edge check, unaffected by class semantics) did. Any future shape
checking for an explicit type triple, as opposed to "is this semantically
an instance of C by any means," should reach for `rdf:type`/`sh:hasValue`,
not `sh:class`.

**Verification**: all shapes conform against the real fixture data
(`valid_backlog_snapshot.ttl` + `crates_and_dependencies.ttl` +
`project_and_status.ttl`), and correctly report exactly the two known
violations (`invalid_orphan_issue.ttl`'s fictional issue,
`real_gap_standalone_issue_274.ttl`'s real one) when those are included —
nothing else in the corpus trips any shape. Each new shape was also proven
to fire on a targeted synthetic violation, not just parsed. See
`tests/backlog_ontology.rs` for the automated version of all of this.

## `agp:` agent-provenance vocabulary (#326)

Added alongside `bl:` for [issue #326](https://github.com/daghovland/rdf-datalog/issues/326)
(a sub-issue of the agent-provenance epic
[#306](https://github.com/daghovland/rdf-datalog/issues/306)). Full design
rationale lives in
[`docs/plans/AGENT_PROVENANCE_PLAN.md`](../../docs/plans/AGENT_PROVENANCE_PLAN.md)
(already reviewed and resolved by the repo owner) -- this section records
two follow-up decisions made while actually writing
[`agentprov-vocabulary.ttl`](agentprov-vocabulary.ttl) that the plan doc
doesn't cover, using the same "why, not what" convention as the rest of
this document.

### `agp:summaryText` has no `rdfs:domain`

**Finding.** `agp:summaryText` is used on both `agp:TranscriptSummary` and
`agp:Decision` (a decision point's own distilled reasoning). A first draft
gave it `rdfs:domain agp:TranscriptSummary`, matching every other
single-class-domain property in this file. That's wrong: `rdfs:domain` is
an entailment, not documentation -- `X agp:summaryText "..."` `⊨` `X a
agp:TranscriptSummary` under RDFS semantics, for every `X`, including an
`agp:Decision`. Plain SHACL validation (what `tests/agentprov_ontology.rs`
actually runs) never triggered this, since this engine performs no
RDFS/OWL-RL entailment at plain-SPARQL-or-SHACL time (see "`rdfs:subClassOf`
is not free" above, which is the identical class of bug).

**Verified directly (not just reasoned about), both directions**: with
`rdfs:domain agp:TranscriptSummary` reinstated on a scratch copy of the
vocabulary, `dagalog -d <fixture with only ex:decision1 a agp:Decision> -o
<scratch copy> -Q 'SELECT ?x WHERE { ?x a agp:TranscriptSummary }'` returned
`ex:decision1` -- reproducing the bug exactly, a real derivation, not a
hypothetical one. With the domain removed (as shipped in
`agentprov-vocabulary.ttl`), the identical query against the identical
fixture returns nothing. (Had the domain stayed, the derived
`ex:decision1 a agp:TranscriptSummary` would go on to fail
`agp:TranscriptSummaryRequiredFieldsShape` and `agp:SummaryGeneratedByShape`
for lack of `agp:reasoningFor`/`prov:wasGeneratedBy` -- a real, silent
contradiction if this vocabulary is ever run through `apply_ontologies`
rather than validated with plain SHACL alone.)

**Decision:** `agp:summaryText` declares no `rdfs:domain` at all. A `owl:unionOf
(agp:TranscriptSummary agp:Decision)` domain was considered and rejected for
now -- this repo already has one open, unresolved reasoner-crash bug from an
under-tested OWL axiom over this exact kind of fixture (see "An OWL
`maxCardinality` restriction was attempted, then reverted" above, tracked as
[#298](https://github.com/daghovland/rdf-datalog/issues/298)), and adding a
second untested axiom shape in the same PR that introduces `#298`'s sibling
vocabulary isn't worth the risk for a domain declaration that buys little
(nothing in this vocabulary's current SHACL shapes or SPARQL query library
plans needs `agp:summaryText`'s domain to be inferable). Revisit if a real
use case needs it, and test it through the reasoner first if so.

### `agp:Decision` is explicitly disjoint from `agp:AgentSession`/`agp:TranscriptSummary`

Mirrors `bl:`'s own Disjointness section above. `agp:AgentSession`
(`rdfs:subClassOf prov:Activity`) and `agp:TranscriptSummary`
(`rdfs:subClassOf prov:Entity`) already inherit disjointness from each
other through their PROV-O superclasses -- verified directly:
`tests/testdata/prov-o.ttl` asserts `prov:Activity owl:disjointWith
prov:Entity`. `agp:Decision`, however, has no PROV-O superclass to inherit
disjointness through, so `agp:Decision owl:disjointWith agp:AgentSession,
agp:TranscriptSummary` is asserted directly in `agentprov-vocabulary.ttl`.

**Checked against the #298 hazard before shipping.** This repo already has
one open, unresolved reasoner `panic!` bug (#298, see above) triggered by
an under-tested OWL axiom over this exact vocabulary/fixture family, so
this disjointness axiom was not shipped on parsing alone: ran the full
multi-type grounding fixture (`agp:AgentSession`, `agp:TranscriptSummary`,
`agp:Decision`, a `bl:PullRequest`, and `prov:SoftwareAgent`/`prov:Person`
individuals, all together) through `apply_ontologies` with `--ontology
backlog/ontology/agentprov-vocabulary.ttl` active. Completes cleanly, no
panic -- the axiom does not trigger #298's failure mode.

## #334 SHACL strengthening: catching a malformed hand-authored summary

[Issue #334](https://github.com/daghovland/rdf-datalog/issues/334) (a
sub-issue of the agent-provenance epic
[#306](https://github.com/daghovland/rdf-datalog/issues/306)) asked
`agentprov-shapes.ttl`'s three existing shapes (from #326) to actually
catch a malformed or degenerate future agent-authored summary, not just
the structural gaps (missing required properties) they already covered.
Two additions, both landed on
`agp:TranscriptSummaryRequiredFieldsShape` (plus a new
`agp:DecisionRequiredFieldsShape` for `agp:Decision`, previously
unconstrained entirely):

**`sh:minLength 30`/`sh:maxLength 4000` on `agp:summaryText`.** Catches
both failure directions the vocabulary's own Privacy note warns about: a
degenerate one-line non-summary (e.g. "Fixed it.") on the low end, and a
raw-transcript dump on the high end. Bounds are not arbitrary: measured the
three real summaries already committed in `provenance/summaries/pr-300.ttl`
(461/506/1101 characters) and picked round numbers with real headroom on
both sides (30 well below the shortest real one, 4000 well above the
longest) rather than tight-fitting the existing corpus, since a
legitimately thorough summary for a complex PR shouldn't fail CI just for
being complete. Applied identically to `agp:Decision`'s own
`agp:summaryText` (a decision point's distilled reasoning is typically
shorter than a full summary but has no principled reason to need a
different bound).

**`sh:class bl:WorkItem` on `agp:reasoningFor`.** Before #334, nothing
checked that `agp:reasoningFor`'s object was actually a work item at all —
a typo'd IRI or a reference to something never asserted as a
`bl:Issue`/`bl:PullRequest` would silently pass. `sh:class bl:WorkItem`
follows `rdfs:subClassOf` closure in this engine (see this shape's own
neighbor `bl:RequiresWorkItemTypeShape` above, and #265/PR #290) — so it's
satisfied by anything typed `bl:Issue` or `bl:PullRequest`, without
requiring the separate literal `a bl:WorkItem` triple those two classes'
own instances need for `bl:WorkItem`-domained properties. That's the
correct check *here*: `agp:reasoningFor`'s target only needs to genuinely
*be* some kind of work item semantically; it is not itself a
`bl:PullRequest`/`bl:Issue` instance being validated against
`bl:RequiresWorkItemTypeShape`'s stricter literal-type requirement (that
shape already fires separately, on the referenced PR/issue stub itself, if
its own literal type is missing).

**Deliberately not added**: a `sh:class prov:Agent` constraint on
`prov:wasAttributedTo`/`prov:wasAssociatedWith`. `prov:SoftwareAgent`/
`prov:Person rdfs:subClassOf prov:Agent` is only asserted in
`tests/testdata/prov-o.ttl` (the real W3C PROV-O file, loaded only by
`tests/real_world_ontologies.rs`'s smoke test) — none of the shapes tests
here load it, and requiring every future summary-validation call site to
additionally load the full PROV-O ontology just to satisfy one class
check wasn't judged worth the coupling. Revisit if a real malformed-agent
case shows up in practice.

Verified all of the above empirically before shipping: the three existing
real summaries (`pr-300.ttl`) and the new `pr-328.ttl` worked example still
conform under the strengthened shapes (`tests/provenance_queries.rs`'s
`every_summary_file_conforms_to_shapes`), and a deliberately malformed
fixture (missing `agp:reasoningFor`, a 9-character `agp:summaryText`) fails
(`malformed_summary_fails_shacl_validation`).

## `agp:abstractText` reuses `dcterms:abstract`; `bl:touchesFile` does NOT reuse `prov:wasInfluencedBy`

Repo owner review of PR #358 asked whether existing vocabulary could be
reused for both new properties -- "it's important to me to relate to
existing vocabulary when possible, this will help reuse of data." Checked
both against their normative sources rather than assuming either would
work:

**`agp:abstractText rdfs:subPropertyOf dcterms:abstract`** -- done. Dublin
Core's `dcterms:abstract` ("A summary of the resource") has no `rdfs:domain`
and no `rdfs:range` constraint in the DCMI Metadata Terms spec -- DCMI
properties are deliberately unconstrained this loosely, precisely so they
can be reused as a super-property without entailing anything unwanted. Safe,
implemented.

**`bl:touchesFile rdfs:subPropertyOf prov:wasInfluencedBy`** -- investigated,
NOT done, real type mismatch found (not just an imprecise fit). Checked the
actual PROV-O ontology: `prov:wasInfluencedBy` is genuinely PROV's most
generic relation (`used`, `wasGeneratedBy`, `wasAssociatedWith`,
`wasAttributedTo`, etc. are all its `rdfs:subPropertyOf` it), but its
`rdfs:domain`/`rdfs:range` are BOTH a union of `{prov:Activity, prov:Agent,
prov:Entity}` -- i.e. it relates two proper RDF resources. `bl:touchesFile`
is necessarily an `owl:DatatypeProperty` (its value is a file-path string
literal, mirroring `bl:Crate`'s own `bl:path`, deliberately -- see
`AGENT_PROVENANCE_PLAN.md` "Granularity" for why file identity stays this
cheap, no per-file resource is minted). In OWL2, datatype properties and
object properties form disjoint hierarchies: a datatype property cannot be
a genuine `rdfs:subPropertyOf` an object property without a real modeling
inconsistency, since range entailment would require a plain string literal
to be typed `rdf:type prov:Entity` (or Activity/Agent) -- not expressible in
RDF at all (a literal cannot be the subject of a type assertion the way a
resource can). This is the same class of hazard `rdfs:subClassOf is not
free` (above) already warns about for this ontology, just on the property
side instead of the class side.

Two real ways forward exist, not decided here (raised back to the repo
owner rather than picked unilaterally, since it's a genuine design fork,
not a bug fix): (a) leave `bl:touchesFile` as a plain string property with
only a documentation-level cross-reference to `prov:wasInfluencedBy` (no
formal axiom, cheap, no design change), or (b) mint file paths as real IRI
resources (e.g. a repo-relative URI scheme) so `bl:touchesFile` becomes a
proper `owl:ObjectProperty` that genuinely CAN be `rdfs:subPropertyOf
prov:used`/`prov:wasInfluencedBy` -- a bigger change, touching the
already-shipped `bl:path`-mirroring convention, but the more spec-correct
reuse if file-level identity ever needs to carry more than a bare path
string anyway.

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
- ~~A SHACL shape requiring every `bl:Issue`/`bl:PullRequest` to also carry
  `a bl:WorkItem`~~ — **done**, `bl:RequiresWorkItemTypeShape` in
  `shapes.ttl` (#285).
- A `createdAt`/`updatedAt`/last-activity timestamp property — needed for
  any "stale" or "open longer than N days" view. #286's query library
  (`../queries/README.md`) explicitly couldn't implement that view without
  one; revisit if it's wanted badly enough to add now rather than waiting
  for #284.
