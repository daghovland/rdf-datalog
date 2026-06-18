# Memory Index

- [rdf_owl_translator crate](project_rdf_owl_translator.md) — Status and design of the RDF→OWL translation crate that bridges turtle_parser and owl2rl2datalog
- [dagalog CLI tool](project_cli.md) — CLI flags, library API, status, and datalog parser notes (--rules not yet working)
- [SHACL crate and test suite](project_shacl.md) — stub shacl crate, 30 ignored tests + parse guard, 60 TTL test data files, README section; validate() is todo!()
- [Persistence implementation](project_persistence.md) — redb changelog over in-memory store; --data-dir flag; 6 integration tests; literal type and concurrent-write invariants
- [Expression plan (EXPRESSION_PLAN.md)](project_expression_plan.md) — SPARQL expressions as Datalog FILTER guards; E1-E5 all done; E4 (SHACL refactor) deferred
- [SPARQL aggregates + property paths](project_sparql_aggregates_paths.md) — both implemented 2026-06-18; key gotchas: multispace0 before AS, HAVING needs group-aware evaluator, backward BFS for + must not include start node
