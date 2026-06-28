# SPARQL Missing Features Plan

Tracked under [epic #48](https://github.com/daghovland/rdf-datalog/issues/48).

Three features are implemented together here because they all touch `sparql_parser/src/ast.rs`
and share the same red-phase review window:

1. **DESCRIBE** ([#49](https://github.com/daghovland/rdf-datalog/issues/49))
2. **FROM / FROM NAMED** ([#50](https://github.com/daghovland/rdf-datalog/issues/50))
3. **Scalar builtin functions** ([#52](https://github.com/daghovland/rdf-datalog/issues/52))

SERVICE and SPARQL Update WHERE form are tracked in issues #51 and #53 and are deferred.

---

## 1. DESCRIBE

### Result semantics

`DESCRIBE <iri>` returns all triples where `<iri>` is the **subject**. For
`DESCRIBE ?var WHERE { ... }` the WHERE clause is evaluated first; each IRI
value bound to `?var` is then described. `DESCRIBE *` describes every subject
IRI returned by the WHERE clause.

"All triples where the IRI occurs" (as subject, predicate, or object) is
explicitly out of scope for now — subject-only is the W3C default and is
simpler to implement correctly. Expand if needed.

### AST changes (`ast.rs`)

```rust
pub enum Query {
    ...
    Describe {
        resources: Vec<Term>,        // IRIs or variables to describe
        where_clause: Vec<QueryComponent>,  // empty if bare DESCRIBE <iri>
    },
}

pub enum QueryResult {
    ...
    Describe(Vec<ResolvedTriple>),
}
```

### Parser (`lib.rs`)

After the CONSTRUCT check, add a DESCRIBE block:
- `DESCRIBE (<iri> | ?var)+ [WHERE] { ... }`
- `DESCRIBE *  [WHERE] { ... }` — describes all subjects from WHERE

### Executor (`execute.rs`)

```
Evaluate WHERE clause → Vec<Substitution>
For each resource term:
  Resolve concrete IRI (substitute variable if needed)
  Collect all triples in named_graphs where iri == subject
Deduplicate and return QueryResult::Describe(triples)
```

---

## 2. FROM / FROM NAMED

### AST changes

New type:
```rust
pub enum DatasetClause {
    Default(GraphElement),   // FROM <iri>
    Named(GraphElement),     // FROM NAMED <iri>
}
```

Add `dataset: Vec<DatasetClause>` to `Query::Select`, `Ask`, `Construct`, and `Describe`.

### Parser

After the query-form keyword and before WHERE, parse zero or more:
```
FROM <iri>          → DatasetClause::Default
FROM NAMED <iri>    → DatasetClause::Named
```

### Executor

When `dataset` is non-empty:
- `Default` clauses: pass their graph IDs as the set of default graphs (union for BGP evaluation)
- `Named` clauses: restrict which graphs are accessible through `GRAPH` patterns

For the initial implementation: single `FROM <iri>` — use that graph ID as `ActiveGraph::Fixed`
instead of `DEFAULT_GRAPH_ELEMENT_ID`. Multiple FROM clauses or FROM NAMED restriction
can be tackled in a follow-up.

---

## 3. Scalar builtin functions

No AST or parser changes — the parser already stores function calls as
`Expression::FunctionCall(name, args)`. All additions are in `eval_function_value`
and `eval_function_bool` inside `sparql_parser/src/execute.rs`.

### Groups

| Group | Functions |
|---|---|
| String | UCASE, LCASE, CONCAT, SUBSTR, STRSTARTS, STRENDS, CONTAINS, STRBEFORE, STRAFTER, ENCODE_FOR_URI, REPLACE |
| Type testing | ISNUMERIC, SAMETERM |
| Term construction | IRI, URI, BNODE, STRDT, STRLANG |
| Numeric | ABS, ROUND, CEIL, FLOOR, RAND |
| Logic | COALESCE, IF |
| Date/time | NOW, YEAR, MONTH, DAY, HOURS, MINUTES, SECONDS, TIMEZONE, TZ |
| Hash/UUID | MD5, SHA1, SHA256, SHA384, SHA512, UUID, STRUUID |

Date/time and hash functions may require new crate dependencies (e.g. `chrono`, `sha2`, `md-5`).
Implement the simpler groups first; defer date/time and hash to a follow-up if they block progress.

---

## Test plan

### DESCRIBE — `sparql_parser/tests/describe_from_tests.rs`

1. `test_describe_iri_parses` — `DESCRIBE <ex:Alice>` → `Query::Describe { resources: [Constant(ex:Alice)], .. }`
2. `test_describe_var_with_where_parses` — `DESCRIBE ?s WHERE { ?s a ex:Person }` → `Query::Describe { resources: [Variable("s")], .. }`
3. `test_describe_star_with_where_parses` — `DESCRIBE * WHERE { ?s a ex:Person }` → resources empty (sentinel)
4. `test_describe_iri_returns_subject_triples` — given 3 triples with ex:Alice as subject, returns all 3
5. `test_describe_var_resolves_to_subjects` — WHERE binds `?s` to ex:Alice; describe returns Alice's subject triples

### FROM / FROM NAMED — same file

6. `test_from_clause_parses` — `SELECT ?s FROM <ex:g> WHERE { ?s ?p ?o }` → `dataset: [Default(ex:g)]`
7. `test_from_named_clause_parses` — `SELECT ?s FROM NAMED <ex:g> WHERE { ?s ?p ?o }` → `dataset: [Named(ex:g)]`
8. `test_from_restricts_to_named_graph` — data in graph ex:g, query with `FROM <ex:g>` returns it; without FROM, empty

### Scalar builtins — `sparql_parser/tests/builtin_tests.rs`

9. `test_ucase` 10. `test_lcase` 11. `test_concat` 12. `test_substr`
13. `test_strstarts` 14. `test_strends` 15. `test_contains`
16. `test_strbefore` 17. `test_strafter`
18. `test_abs` 19. `test_round` 20. `test_ceil` 21. `test_floor`
22. `test_coalesce_first_non_error` 23. `test_if_true` 24. `test_if_false`
25. `test_sameterm_equal` 26. `test_sameterm_different`
27. `test_isnumeric_integer` 28. `test_isnumeric_string`
29. `test_iri_from_string` 30. `test_strdt` 31. `test_strlang`
