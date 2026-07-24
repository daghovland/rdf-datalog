# Backlog example fixtures

Ground-truth Turtle fixtures for the [dagalog-on-dagalog epic](https://github.com/daghovland/rdf-datalog/issues/282):
a self-hosted RDF/SPARQL/SHACL mirror of this repo's GitHub issue backlog.

These files exist to give the ontology design ([#283](https://github.com/daghovland/rdf-datalog/issues/283)),
SHACL shapes ([#285](https://github.com/daghovland/rdf-datalog/issues/285)), and SPARQL query library
([#286](https://github.com/daghovland/rdf-datalog/issues/286)) something concrete to build against and
integration-test, before the loader ([#284](https://github.com/daghovland/rdf-datalog/issues/284)) exists to
pull live data from the GitHub API.

**Status: the ontology is now formalized.** See
[`../ontology/vocabulary.ttl`](../ontology/vocabulary.ttl) for the actual
class/property declarations (with `rdfs:label`/`rdfs:comment` on each) and
[`../ontology/MODELING_NOTES.md`](../ontology/MODELING_NOTES.md) for the
reasoning behind every decision, including the two questions this directory
originally left open (both resolved there: epic modeling stays structural;
workflow status became a new `bl:status` property, informed by the
Kanban/Jira/Azure DevOps comparison below). The fixtures in this directory
were updated to match.

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
- `project_and_status.ttl` — the repository itself as a `doap:Project` (see
  `MODELING_NOTES.md`'s "DOAP vs. bespoke `bl:Crate`"), every crate linked to
  it, and a small, honestly-representative set of real issues given a
  `bl:status` value (see `MODELING_NOTES.md`'s "Workflow status as its own
  axis" for why this isn't populated everywhere).

## Comparison with Kanban/Jira/Azure DevOps

There's no broadly-adopted RDF vocabulary for issue tracking the way FOAF
covers people or DOAP covers software projects — Jira and Azure DevOps have
proprietary schemas, not published ontologies. Two ideas carried over from
them into the now-finalized ontology (`../ontology/vocabulary.ttl`,
reasoning in `../ontology/MODELING_NOTES.md`):

1. **Work-item hierarchy** (Epic → Story/Task/Bug) is the same shape as
   GitHub's epic/sub-issue relation already modeled here — confirms the
   structural-role approach (below) generalizes, no new concept needed.
2. **Workflow status as its own axis**, separate from a raw open/closed
   bit — Jira's To Do/In Progress/Done, Azure's board columns. This repo
   already has exactly that distinction in practice (unlabeled = TODO,
   `ready` = reviewed-and-approved, an in-progress convention, closed =
   done); the ontology now has a `bl:status` property (range
   `bl:WorkflowStatus`) capturing it, separate from `bl:state`.

For the codebase side, **DOAP** (Description of a Project) is the closest
real precedent — reused directly for the repository as a whole
(`doap:Project`, see `project_and_status.ttl`), while individual crates keep
the bespoke `bl:Crate` (DOAP has no workspace-submodule concept to reuse
there — see `MODELING_NOTES.md` for why).

## Epic modeling (resolved in `../ontology/MODELING_NOTES.md`)

"Epic" is a purely structural role, not a first-class `rdf:type`: an issue
with no `bl:subIssueOf` that is the target of at least one other issue's
`bl:subIssueOf`. See `MODELING_NOTES.md` for the full reasoning and the
trade-off acknowledged there.
