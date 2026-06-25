# SPARQL guide

SPARQL is the standard query language for RDF. If you know SQL, many concepts will
feel familiar — but instead of rows and columns, you match graph patterns.

All examples below assume the `people.ttl` from the [quickstart](quickstart.md) is loaded.

---

## Basic pattern matching

A SPARQL query has a `SELECT` clause (what to return) and a `WHERE` clause (what to match).

Variables start with `?`. A triple pattern in the WHERE clause matches any triple where
the fixed parts match and the variables bind to whatever is there.

```sparql
PREFIX ex: <http://example.org/>
SELECT ?person WHERE {
    ?person a ex:Person .
}
```

`a` is shorthand for `rdf:type`. This finds every subject that has been declared a `Person`.

---

## Filtering results

Use `FILTER` with any comparison expression:

```sparql
PREFIX ex: <http://example.org/>
SELECT ?person ?age WHERE {
    ?person a ex:Person ;
            ex:age ?age .
    FILTER (?age >= 18)
}
```

Available operators: `=`, `!=`, `<`, `<=`, `>`, `>=`, `&&`, `||`, `!`

Useful functions: `regex(?var, "pattern")`, `strlen(?var)`, `lang(?var)`,
`datatype(?var)`, `isIRI(?var)`, `isLiteral(?var)`, `str(?var)`

---

## Optional data

`OPTIONAL` lets you include data that may or may not exist, without excluding rows where
it is missing (like a LEFT JOIN in SQL):

```sparql
PREFIX ex:   <http://example.org/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?person ?label WHERE {
    ?person a ex:Person .
    OPTIONAL { ?person rdfs:label ?label }
}
```

Rows where the person has no `rdfs:label` will have `?label` unbound (empty).

---

## Multiple patterns on the same subject

Turtle shorthand (`;` and `,`) works in SPARQL too:

```sparql
PREFIX ex: <http://example.org/>
SELECT ?person ?age WHERE {
    ?person a ex:Person ;
            ex:age ?age .
}
```

This is equivalent to writing two separate triple patterns for `?person`.

---

## Following relationships

Join on a shared variable to follow relationships across triples:

```sparql
PREFIX ex:   <http://example.org/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?person ?contact ?contactName WHERE {
    ?person a ex:Person .
    ?person ex:knows ?contact .
    ?contact rdfs:label ?contactName .
}
```

---

## UNION — alternative patterns

Use `UNION` when you want results matching either of two patterns:

```sparql
PREFIX ex: <http://example.org/>
SELECT ?x WHERE {
    { ?x a ex:Person . }
    UNION
    { ?x a ex:Organisation . }
}
```

---

## Named graphs

Data can be stored in named graphs. Use `GRAPH` to query a specific graph:

```sparql
SELECT ?s ?p ?o WHERE {
    GRAPH <http://example.org/myDataset> {
        ?s ?p ?o .
    }
}
```

Omit the `GRAPH` clause to query the default graph only.

---

## DISTINCT and LIMIT

Remove duplicates with `DISTINCT`. Cap result count with `LIMIT`:

```sparql
PREFIX ex: <http://example.org/>
SELECT DISTINCT ?class WHERE {
    ?x a ?class .
}
LIMIT 20
```

`OFFSET` skips the first N results — useful for pagination:

```sparql
SELECT ?s WHERE { ?s ?p ?o } LIMIT 10 OFFSET 20
```

---

## Inline data with VALUES

`VALUES` lets you supply a fixed set of values to match against:

```sparql
PREFIX ex: <http://example.org/>
SELECT ?person ?age WHERE {
    VALUES ?person { ex:Alice ex:Bob }
    ?person ex:age ?age .
}
```

---

## SELECT *

`SELECT *` returns all variables bound in the WHERE clause:

```sparql
SELECT * WHERE { ?s ?p ?o } LIMIT 5
```

---

## Supported features at a glance

| Feature | Supported |
|---|---|
| `SELECT`, `SELECT DISTINCT`, `SELECT *` | ✓ |
| Basic graph patterns, `;` and `,` shorthand | ✓ |
| `FILTER` — comparisons, `regex()`, `lang()`, `bound()`, `EXISTS`, `NOT EXISTS` | ✓ |
| `OPTIONAL` | ✓ |
| `UNION`, `MINUS` | ✓ |
| `GRAPH` — named-graph patterns | ✓ |
| `BIND`, subqueries | ✓ |
| `VALUES` — inline data | ✓ |
| `LIMIT`, `OFFSET` | ✓ |
| Property paths — `/`, `*`, `+`, `?`, `\|`, `^`, `!` (all forms) | ✓ |
| `GROUP BY`, `HAVING`, `ORDER BY` | ✓ |
| Aggregates (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, `SAMPLE`, `GROUP_CONCAT`) | ✓ |
| `CONSTRUCT`, `ASK` | ✓ |
| `DESCRIBE` | Not yet implemented ([#49](https://github.com/daghovland/rdf-datalog/issues/49)) |
| `FROM` / `FROM NAMED` dataset clauses | Not yet implemented ([#50](https://github.com/daghovland/rdf-datalog/issues/50)) |
| `SERVICE` federated queries | Parsed; silently returns empty ([#51](https://github.com/daghovland/rdf-datalog/issues/51)) |
| Scalar builtins (`COALESCE`, `IF`, `CONCAT`, `UCASE`, date/time, hash…) | Partial ([#52](https://github.com/daghovland/rdf-datalog/issues/52)) |
| SPARQL Update `INSERT/DELETE WHERE` | Not yet implemented ([#53](https://github.com/daghovland/rdf-datalog/issues/53)) |

---

## See also

- [Full SPARQL section in the README](../../README.md#sparql-queries) — more examples
- [W3C SPARQL 1.1 specification](https://www.w3.org/TR/sparql11-query/)
