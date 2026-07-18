# Reasoning and custom rules

dagalog can automatically infer new facts from your data using two mechanisms:

1. **OWL-RL reasoning** — standard W3C inference rules applied when you load an OWL ontology
2. **Custom Datalog rules** — your own forward-chaining rules written in a compact syntax

Both work by materialising inferred triples into the same store as the original data, so
they are immediately queryable with SPARQL.

---

## OWL-RL reasoning

OWL (Web Ontology Language) lets you declare class hierarchies, property constraints, and
equivalences. OWL-RL is a tractable subset of OWL 2 that can be evaluated efficiently via
Datalog materialisation.

### What it does

Given an ontology that says:

```turtle
ex:Employee rdfs:subClassOf ex:Person .
```

And data that says:

```turtle
ex:Bob a ex:Employee .
```

After reasoning, dagalog will also know that `ex:Bob a ex:Person` — because every Employee
is a Person.

### CLI

Use `--ontology` to load one or more OWL files and apply reasoning before querying:

```sh
dagalog --data data.ttl --ontology schema.ttl \
        --query "SELECT ?x WHERE { ?x a ex:Person }"
```

The ontology and data can be the same file (OWL ontologies typically contain both the
schema and instance data):

```sh
dagalog --ontology myontology.ttl \
        --query "SELECT ?x WHERE { ?x a ex:Person }"
```

### Rust API

```rust,no_run
use dagalog::{apply_ontologies, load_file};
use dag_rdf::Datastore;
use std::path::{Path, PathBuf};

let mut ds = Datastore::new(100_000);
load_file(&mut ds, Path::new("data.ttl")).unwrap();

let stats = apply_ontologies(&mut ds, &[PathBuf::from("schema.ttl")]).unwrap();
println!("Derived {} new triples from {} axioms", 
         stats.triples_after - stats.triples_before,
         stats.axiom_count);
```

### Supported OWL-RL patterns (non-exhaustive)

- `rdfs:subClassOf`, `rdfs:subPropertyOf` — class and property hierarchy
- `owl:sameAs` — equality propagation
- `owl:intersectionOf`, `owl:unionOf` — class expressions
- `owl:someValuesFrom`, `owl:allValuesFrom` — existential/universal restrictions
- `owl:minQualifiedCardinality` — cardinality constraints
- Inverse object properties

---

## Custom Datalog rules

When OWL-RL does not cover your use case, you can write custom Datalog rules. These use a
straightforward syntax: a rule says "if these patterns match, conclude this fact".

### Rule syntax

Rules go in a `.datalog` file:

```datalog
# Prefix declarations — same syntax as SPARQL or Turtle
PREFIX ex: <http://example.org/>

# Head :- Body
# If ?x is a Manager, conclude ?x is an Employee
ex:Employee[?x] :- ex:Manager[?x] .

# Multiple body patterns — all must match
ex:SeniorEmployee[?x] :- ex:Employee[?x], [?x, ex:yearsOfService, ?y], FILTER(?y > 10) .

# Stratified negation
ex:ActiveEmployee[?x] :- ex:Employee[?x], NOT ex:Terminated[?x] .

# Use bracket syntax for any triple shape
[?x, ex:teamMember, ?y] :- [?x, ex:manages, ?y] .
```

**Built-in prefixes** (no declaration needed): `rdf:`, `rdfs:`, `xsd:`, `owl:`

**`a`** expands to `rdf:type` everywhere.

**`FILTER(expr)`** in the body acts as a guard — the rule fires only when the SPARQL
expression is true. All SPARQL operators and functions are supported.

### CLI

```sh
dagalog --data data.ttl --rules rules.datalog \
        --query "SELECT ?x WHERE { ?x a ex:SeniorEmployee }"
```

### Rust API

```rust,no_run
use dagalog::apply_rules;
use std::path::PathBuf;
# use dag_rdf::Datastore;
# let mut ds = Datastore::new(100_000);

apply_rules(&mut ds, &[PathBuf::from("rules.datalog")]).unwrap();
```

### Combining with OWL reasoning

You can use both `--ontology` and `--rules` together. OWL-RL reasoning runs first,
then your custom rules:

```sh
dagalog --data data.ttl --ontology schema.ttl --rules custom.datalog \
        --query "SELECT ?x WHERE { ?x a ex:SeniorEmployee }"
```

---

## How materialisation works

Both OWL-RL and Datalog rules use **naive forward-chaining materialisation**:

1. Start with the original triples.
2. Apply all applicable rules, adding any newly inferred triples.
3. Repeat until no new triples are added (fixed point).

The inferred triples live in the same store as the original data and are immediately
queryable with SPARQL. No special query syntax is needed — you just query normally and
the inferred facts appear alongside the loaded ones.

Stratified negation is supported: rules with `NOT` in the body are evaluated in a
topological order that ensures the negated predicate is fully materialised before the
rule fires.

---

## See also

- [Formats](formats.md) — loading the ontology/data files reasoning operates on
- [SPARQL guide](sparql-guide.md) — querying the materialised triples
- [`tests/owl_integration.rs`](../../tests/owl_integration.rs) (`cargo test --test owl_integration`)
  and [`tests/datalog_integration.rs`](../../tests/datalog_integration.rs)
  (`cargo test --test datalog_integration`) — the executable test suites this page is based on
- [`tests/manchester_owl_reasoning.rs`](../../tests/manchester_owl_reasoning.rs) — Manchester
  Syntax ontologies driving the same reasoning pipeline
