---
name: rdf_owl_translator crate
description: Status and design of the rdf_owl_translator crate - translates RDF triples into OWL Ontology
type: project
---

The `rdf_owl_translator` crate was added to complete the end-to-end pipeline from Turtle → OWL → Datalog → reasoning.

**Why:** The `main.rs` was using an empty OWL `Ontology` so no reasoning rules were generated. This crate bridges turtle_parser output (a Datastore of RDF triples) with owl2rl2datalog (which takes an OWL Ontology).

**How to apply:** Call `rdf2owl(&mut datastore)` to get an `OntologyDocument` after parsing Turtle. Then pass `&doc.ontology` to `owl2datalog`.

Key files:
- `rdf_owl_translator/src/ingress.rs` — `WellKnownIds` struct (pre-computed IRI IDs), `get_rdf_list_elements`, `topological_sort`
- `rdf_owl_translator/src/class_expression_parser.rs` — Builds CE/DR/OPE/DPE/AP/Annotation maps, parses anonymous class expressions and restrictions
- `rdf_owl_translator/src/axiom_parser.rs` — Translates individual triples → Axiom using the maps
- `rdf_owl_translator/src/rdf2owl.rs` — Top-level entry point `rdf2owl(&mut Datastore)`

Mirrors `DagSemTools.RdfOwlTranslator` (Rdf2Owl.fs + ClassExpressionParser.fs + AxiomParser.fs + Ingress.fs).
