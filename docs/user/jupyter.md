# Jupyter Kernel — interactive pipeline notebooks

The dagalog Jupyter kernel lets you write RDF data pipelines as interactive notebooks.
Each cell is a pipeline step; the in-memory datastore persists across all cells in
a session. Results from SPARQL SELECT appear as HTML tables inline in the notebook.

No system ZeroMQ library is required — the kernel is a single self-contained binary.

---

## Prerequisites

You need Rust (to build dagalog) and JupyterLab (or classic Jupyter Notebook):

```sh
pip install jupyterlab
```

---

## Install the kernel

```sh
# Build the kernel binary
cargo build -p dagalog-kernel --release

# Write the kernel spec to ~/.local/share/jupyter/kernels/dagalog/
./target/release/dagalog-kernel install

# Verify Jupyter can see it
jupyter kernelspec list
# dagalog    ~/.local/share/jupyter/kernels/dagalog
```

---

## Open the example notebook

The repo ships an introductory notebook at `notebooks/dagalog_intro.ipynb`:

```sh
jupyter lab notebooks/dagalog_intro.ipynb
```

Select **Dagalog (SPARQL + RDF)** from the kernel picker. Run cells top-to-bottom to
see a complete pipeline: load data, query, apply an RML mapping, run reasoning, and
inspect the results.

---

## Cell types

### SPARQL (default)

Cells without a `%%` prefix are treated as SPARQL. SELECT results render as an HTML
table; UPDATE and ASK return a plain status line.

```sparql
PREFIX foaf: <http://xmlns.com/foaf/0.1/>

SELECT ?name ?age WHERE {
    ?person a foaf:Person ;
            foaf:name ?name ;
            foaf:age  ?age .
}
ORDER BY ?name
```

### `%%turtle` — load inline Turtle

Parse the cell body as Turtle and add the resulting triples to the session datastore.

```text
%%turtle
@prefix ex:   <http://example.com/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Alice a foaf:Person ; foaf:name "Alice" ; foaf:age 30 .
ex:Bob   a foaf:Person ; foaf:name "Bob"   ; foaf:age 25 .
```

Output: `Loaded 6 triples.`

### `%%load` — load a file

Load a Turtle, TriG, or N-Triples file from disk.

```text
%%load data/people.ttl
```

### `%%rml` — apply an RML mapping

Apply an [RML mapping file](rml-mapping.md) to its declared sources (CSV, JSON,
XML) and add the resulting triples to the session datastore.

```text
%%rml mappings/persons.ttl
```

### `%%reason` — run OWL-RL reasoning

Materialise all triples that can be inferred from any OWL axioms already in the
datastore. Adds the inferred triples in-place.

```text
%%reason
```

Output: `Reasoning complete. 1 243 triples added.`

### `%%datalog` — assert Datalog rules

Parse the cell body as Datalog rules and run forward-chaining materialisation.

```text
%%datalog
?x <http://example.com/colleague> ?y :-
    ?x <http://example.com/worksFor> ?org ,
    ?y <http://example.com/worksFor> ?org .
```

### `%%validate` — SHACL validation

Validate the current datastore against a SHACL shapes file.

```text
%%validate shapes/person.ttl
```

---

## Typical pipeline

```text
[Cell 1] %%turtle        — seed a few triples inline
[Cell 2] SELECT …        — verify the data is there
[Cell 3] %%rml …         — map a CSV file to RDF
[Cell 4] %%reason        — infer additional triples
[Cell 5] SELECT …        — inspect the enriched graph
[Cell 6] %%datalog       — apply custom rules
[Cell 7] %%validate …    — check SHACL shapes
```

---

## Uninstall

```sh
jupyter kernelspec remove dagalog
```
