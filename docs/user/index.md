# dagalog user documentation

dagalog is a fast RDF triplestore with OWL-RL reasoning, custom Datalog rules, and a
SPARQL HTTP endpoint. It can be used as a CLI tool, a Rust library, or a server.

---

## Where to start

| I want to… | Go to |
|---|---|
| Load data and run my first query in 5 minutes | [Quickstart](quickstart.md) |
| Learn to write SPARQL queries | [SPARQL guide](sparql-guide.md) |
| Understand supported file formats | [Formats](formats.md) |
| Map CSV, JSON, or XML data to RDF | [RML mapping](rml-mapping.md) |
| Define reusable RDF triple patterns | [OTTR templates](ottr-templates.md) |
| Add OWL reasoning or Datalog rules | [Reasoning and rules](reasoning.md) |
| Write interactive pipeline notebooks | [Jupyter kernel](jupyter.md) |
| Deploy dagalog as a server | [Deployment](deployment.md) |

---

## What is RDF?

RDF (Resource Description Framework) is a standard for describing things on the web.
Data is stored as **triples**: every fact has a subject, a predicate, and an object —
like a sentence with a subject, verb, and object.

```
<http://example.org/Alice>  <http://example.org/knows>  <http://example.org/Bob> .
```

This triple says "Alice knows Bob". Everything has a URI, which means data from
different sources can be linked together automatically.

dagalog stores these triples and lets you query them with SPARQL — a SQL-like query
language for RDF.

---

## Two ways to use dagalog

**CLI / one-shot queries** — useful for data exploration, scripts, and batch jobs:

```sh
dagalog --data people.ttl --query "SELECT ?name WHERE { ?x <http://example.org/name> ?name }"
```

**Server** — useful for applications, dashboards, and shared data access:

```sh
dagalog --serve --data people.ttl
# then open http://localhost:3030 in your browser
```

The server exposes a standard SPARQL 1.1 endpoint and a web UI with a query editor,
a resource browser, and a visual query builder.
