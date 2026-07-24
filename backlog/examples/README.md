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
  still open — a realistic "epic not yet fully done" shape), plus the
  dagalog-on-dagalog epic (#282) itself with its six sub-issues (all still
  open/unlabeled except #283). Also includes one deliberately subtle real
  case: PR #292 references issue #161 without closing it (the agent that
  implemented #292 caught mid-task that auto-closing #161 would have been
  wrong, since only part of its scope was addressed) — modeled with a
  separate `bl:relatesToIssue` predicate distinct from `bl:closesIssue`, to
  make sure the ontology/SHACL/SPARQL work don't collapse "referenced" and
  "closed" into the same relation.
- `invalid_orphan_issue.ttl` — a small, clearly-fictional negative fixture:
  one issue that is neither an epic (no sub-issues) nor a sub-issue of any
  epic (`bl:subIssueOf` absent), violating this repo's own backlog policy
  ("every issue must either be an epic or a sub-issue of an epic",
  `CLAUDE.md`). For #285's SHACL shapes to have something to reject. Uses a
  made-up issue number/IRI (`.../issues/999999`) so it can never be confused
  with a real, currently-broken issue in this repo.

## Open modeling question (for #283, not resolved here)

Whether "epic" is a first-class `rdf:type` (`bl:Epic`) or a purely
structural role (an issue that has no `bl:subIssueOf` and is the target of at
least one other issue's `bl:subIssueOf`). These fixtures take the latter,
simpler approach — no `bl:Epic` class, epics are just issues without a
parent that other issues point at — since it can never drift out of sync
with the actual sub-issue graph the way a separately-asserted type could.
#283 should treat this as a starting proposal to confirm or overturn, not a
decision already made.
