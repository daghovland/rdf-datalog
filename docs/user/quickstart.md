# Quickstart — 5 minutes to your first query

This guide walks you from nothing to loading real data and getting query results.
No Rust knowledge required for the CLI or server paths.

---

## Step 1 — Install

**Option A: Docker (no Rust required)**

```sh
docker run -p 3030:3030 ghcr.io/daghovland/dagalog
```

Then open <http://localhost:3030> and skip to [Step 3](#step-3--load-some-data).

**Option B: From source**

You need Rust 1.85 or later. Install via [rustup.rs](https://rustup.rs/) if needed.

```sh
cargo install --git https://github.com/daghovland/rdf-datalog dagalog
```

This places the `dagalog` binary in `~/.cargo/bin/` (already on your `$PATH` after
rustup installs Rust).

---

## Step 2 — Create a data file

Create a file called `people.ttl` with these contents:

```turtle
@prefix ex: <http://example.org/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:Alice a ex:Person ;
    rdfs:label "Alice" ;
    ex:age 30 ;
    ex:knows ex:Bob .

ex:Bob a ex:Person ;
    rdfs:label "Bob" ;
    ex:age 25 .

ex:Carol a ex:Person ;
    rdfs:label "Carol" ;
    ex:age 17 .
```

This Turtle file defines three people with ages and a "knows" relationship.
RDF uses angle-bracket URIs for everything; `@prefix` lets you abbreviate them.

---

## Step 3 — Load some data

### CLI: run a single query

```sh
dagalog --data people.ttl \
        --query "SELECT ?name WHERE { ?x rdfs:label ?name }"
```

Expected output:

```
?name
---------
"Alice"
"Bob"
"Carol"
```

### Server: interactive web UI

```sh
dagalog --serve --data people.ttl
```

Open <http://localhost:3030>. Paste the query below into the SPARQL box and click **Run**:

```sparql
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?name WHERE { ?x rdfs:label ?name }
```

---

## Step 4 — Try a more interesting query

Find all people under 18:

```sparql
PREFIX ex: <http://example.org/>
SELECT ?person ?age WHERE {
    ?person a ex:Person ;
            ex:age ?age .
    FILTER (?age < 18)
}
```

Find who Alice knows, including the contact's label:

```sparql
PREFIX ex: <http://example.org/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?contact ?name WHERE {
    ex:Alice ex:knows ?contact .
    ?contact rdfs:label ?name .
}
```

---

## What's next

- [SPARQL guide](sparql-guide.md) — more query patterns, OPTIONAL, UNION, GRAPH
- [Formats](formats.md) — JSON-LD, Turtle, TriG, N-Triples
- [OTTR templates](ottr-templates.md) — reusable triple-pattern templates (stOTTR)
- [RML mapping](rml-mapping.md) — load CSV, JSON, or XML data as RDF
- [Reasoning and rules](reasoning.md) — OWL-RL reasoning, custom Datalog rules
- [Deployment](deployment.md) — authentication, Docker, environment variables
- [Full reference in README](../../README.md) — complete feature reference
