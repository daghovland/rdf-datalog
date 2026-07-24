# Backlog example fixtures

Ground-truth Turtle fixtures for the [dagalog-on-dagalog epic](https://github.com/daghovland/rdf-datalog/issues/282):
a self-hosted RDF/SPARQL/SHACL mirror of this repo's GitHub issue backlog.

These files exist to give the ontology design ([#283](https://github.com/daghovland/rdf-datalog/issues/283)),
SHACL shapes ([#285](https://github.com/daghovland/rdf-datalog/issues/285)), and SPARQL query library
([#286](https://github.com/daghovland/rdf-datalog/issues/286)) something concrete to build against and
integration-test, before the loader ([#284](https://github.com/daghovland/rdf-datalog/issues/284)) exists to
pull live data from the GitHub API.

**Status: provisional.** The predicate/class names here (`bl:` prefix,
`https://dagalog.dev/ns/backlog#` namespace) are a first-pass sketch to make
the examples writable, not a finalized vocabulary — #283's job is to settle
the actual ontology, including the open modeling question flagged inline
below (epic-as-type vs. epic-as-structural-role). Expect predicate names in
these files to change once #283 lands; the example *data* (which real
issues/PRs are represented, and how they relate) should stay accurate.

## Files

- `valid_backlog_snapshot.ttl` — a real, accurate slice of this repo's actual
  backlog: the SHACL correctness-audit epic ([#267](https://github.com/daghovland/rdf-datalog/issues/267))
  with its nine sub-issues (seven closed with their closing PRs linked, two
  still open — a realistic "epic not yet fully done" shape), the
  dagalog-on-dagalog epic (#282) with its six sub-issues, and partial slices
  of two more epics (#178, #25) included only for specific groundings noted
  inline (the `closesIssue`/`relatesToIssue` distinction, and giving the
  `sparql_endpoint` crate at least one example). Also includes one
  deliberately subtle real case: PR #292 references issue #161 without
  closing it (the agent that implemented #292 caught mid-task that
  auto-closing #161 would have been wrong, since only part of its scope was
  addressed) — modeled with a separate `bl:relatesToIssue` predicate
  distinct from `bl:closesIssue`, so the ontology/SHACL/SPARQL work don't
  collapse "referenced" and "closed" into the same relation.
- `crates_and_dependencies.ttl` — the codebase itself: every workspace
  member crate (read directly off the root `Cargo.toml`'s `members` list)
  and its real path-dependency edges (read off each crate's own
  `Cargo.toml`), plus `bl:touchesCrate` links added to the PRs above —
  answers "which crates have unresolved bugs" or "what depends on shacl"
  by joining ticket data against the dependency graph. See the file's own
  header for why a bespoke `bl:Crate` was used here instead of DOAP.
- `invalid_orphan_issue.ttl` — a small, clearly-fictional negative fixture:
  one issue that is neither an epic (no sub-issues) nor a sub-issue of any
  epic (`bl:subIssueOf` absent), violating this repo's own backlog policy
  ("every issue must either be an epic or a sub-issue of an epic",
  `CLAUDE.md`). For #285's SHACL shapes to have something to reject. Uses a
  made-up issue number/IRI (`.../issues/999999`) so it can never be confused
  with a real, currently-broken issue in this repo.
- `real_gap_standalone_issue_274.ttl` — a second, *real* example of the same
  violation shape: issue #274 genuinely has no parent epic (confirmed via
  GitHub's GraphQL `issue.parent` field), a small pre-existing gap against
  this repo's own policy, kept separate from the fictional fixture above so
  that file's documented "no real issue is broken" guarantee stays literally
  true. Not fixed here — resolving it, if wanted, is the repo owner's call.

## Comparison with Kanban/Jira/Azure DevOps (informs, doesn't resolve, #283)

There's no broadly-adopted RDF vocabulary for issue tracking the way FOAF
covers people or DOAP covers software projects — Jira and Azure DevOps have
proprietary schemas, not published ontologies. Two ideas are worth carrying
over from them into #283's actual design, though:

1. **Work-item hierarchy** (Epic → Story/Task/Bug) is the same shape as
   GitHub's epic/sub-issue relation already modeled here — no new concept
   needed, just confirms the structural-role approach below generalizes.
2. **Workflow status as its own axis**, separate from a raw open/closed
   bit — Jira's To Do/In Progress/Done, Azure's board columns. This repo
   already has exactly that distinction in practice (unlabeled = TODO,
   `ready` = reviewed-and-approved, an implicit in-progress state, closed =
   done) but these fixtures only capture GitHub's binary `bl:state`. Worth
   #283 adding a separate `bl:status` (or similar) rather than trying to
   overload `bl:state` or `bl:hasLabel` to carry that meaning — not done in
   these fixtures, to avoid re-litigating already-written examples before
   #283 gets to it, but flagged here as a concrete recommendation.

For the codebase side, **DOAP** (Description of a Project) is the closest
real precedent — it already models a project/repository/component tree.
`crates_and_dependencies.ttl` uses a bespoke `bl:Crate` for now (see that
file's header), but #283 should weigh adopting DOAP properly (e.g.
`doap:Project` for the repo as a whole) instead of reinventing it from
scratch.

## Open modeling question (for #283, not resolved here)

Whether "epic" is a first-class `rdf:type` (`bl:Epic`) or a purely
structural role (an issue that has no `bl:subIssueOf` and is the target of at
least one other issue's `bl:subIssueOf`). These fixtures take the latter,
simpler approach — no `bl:Epic` class, epics are just issues without a
parent that other issues point at — since it can never drift out of sync
with the actual sub-issue graph the way a separately-asserted type could.
#283 should treat this as a starting proposal to confirm or overturn, not a
decision already made.
