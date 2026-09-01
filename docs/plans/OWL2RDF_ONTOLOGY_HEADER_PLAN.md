# Plan: `owl2rdf` — ontology header triples ([#515](https://github.com/daghovland/rdf-datalog/issues/515))

Split out from [#373](https://github.com/daghovland/rdf-datalog/issues/373) item 8, part of the
general OWL 2 → RDF structural mapping. Implemented in `owl2rl2datalog/src/owl_to_rdf.rs`.

## Spec citation

<https://www.w3.org/TR/owl2-mapping-to-rdf/> §2.1 ("Translation of Axioms without Annotations"),
Table 1's `T(Ontology(...))` row (the header of the mapping is the whole-ontology-document
translation entry point, not a per-axiom dispatch entry — there is no separately titled
"Translation of Ontology Headers" section in the current REC, despite the anchor name; the rule
lives in Table 1). Fetched and verified against the live page.

`T` applied to the whole ontology document:

```
Ontology( ontologyIRI [ versionIRI ]
    Import( importedOntologyIRI1 ) ...
    Annotation( ... ) ...
    Axiom ...
)
```

maps to:

```
ontologyIRI rdf:type owl:Ontology .
[ ontologyIRI owl:versionIRI versionIRI ] .        # only if a version IRI is declared
ontologyIRI owl:imports importedOntologyIRI1 .     # one triple per import
...
```

and for an **anonymous** ontology (no ontology IRI at all):

```
_:x rdf:type owl:Ontology .
_:x owl:imports importedOntologyIRI1 .
...
```

— a fresh blank node `_:x` stands in for the (absent) ontology IRI as the subject of every header
triple. The literal spec text means `T(O)` **always** emits at least the `rdf:type owl:Ontology`
triple, even for a completely bare anonymous ontology with no imports/version/annotations/axioms.

**Deliberate deviation from the literal base case**: this implementation does *not* emit that
lone blank-node triple when the ontology is anonymous *and* has no imports *and* no annotations.
Reason: `rdf_owl_translator::rdf2owl` produces exactly this shape
(`OntologyVersion::UnNamedOntology`, empty imports/annotations) for plain RDF that never declared
`<x> rdf:type owl:Ontology` anywhere — i.e. RDF data that was never an "OWL ontology document" in
the first place, just facts. Always synthesizing the blank node here would silently add a node
that was never in the source data on every such round trip
(`rdf2owl` → `owl2rdf`), breaking `tests/manchester_roundtrip.rs`'s
`rdf_starting_roundtrip_preserves_graph_isomorphism` graph-isomorphism check — this was caught by
actually running that test, not anticipated up front. A bare, unattached blank node carries no
information a caller couldn't already infer from `ontology.version` being `UnNamedOntology` with
empty imports/annotations, so suppressing it loses nothing. See `Translator::ontology_header`'s
doc comment for the same reasoning in code.

Ontology-level `Annotation(...)` elements (this codebase's `Ontology::annotations` field) map
through the general `TANN` annotation function the same way axiom annotations do, but as **plain,
non-reified** triples on the header node — `owl:Axiom` reification only applies to annotations
*on an axiom*, not to the ontology's own annotation block. So: `mainNode AP T(av)` for each
`(AP, av)` pair, where `mainNode` is the ontology IRI node (or `_:x` if anonymous).

## Placement

`owl2rdf`'s current `for axiom in &ontology.axioms { translator.axiom(axiom); }` loop only walks
axioms — the `Ontology` struct's own `version` (`ingress::OntologyVersion`),
`directly_imports_documents`, and `annotations` fields were never touched. This is not a
dispatch-table entry (there's no `Axiom` variant for the header) — it's a new
`Translator::ontology_header` method called once per `owl2rdf` call, before the axiom loop, whose
return value (the header node id — either the ontology IRI's node or the fresh blank node) is
available if a later change wants to attach more things to it.

```rust
fn ontology_header(&mut self, ontology: &Ontology) -> Option<GraphElementId> {
    let bare_anonymous = matches!(ontology.version, OntologyVersion::UnNamedOntology)
        && ontology.directly_imports_documents.is_empty()
        && ontology.annotations.is_empty();
    if bare_anonymous {
        return None;
    }
    let subject = match &ontology.version {
        OntologyVersion::UnNamedOntology => self.datastore.new_anonymous_blank_node(),
        OntologyVersion::NamedOntology(iri) => self.iri(&iri.0),
        OntologyVersion::VersionedOntology { ontology_iri, .. } => self.iri(&ontology_iri.0),
    };
    self.type_triple(subject, OWL_ONTOLOGY);
    if let OntologyVersion::VersionedOntology { version_iri, .. } = &ontology.version {
        let version_id = self.iri(&version_iri.0);
        self.triple_p(subject, OWL_VERSION_IRI, version_id);
    }
    for import in &ontology.directly_imports_documents {
        let import_id = self.iri(&import.0);
        self.triple_p(subject, OWL_IMPORT, import_id);
    }
    for (ap, av) in &ontology.annotations {
        let ap_id = self.full_iri(ap);
        let av_id = self.annotation_value(av);
        self.triple(subject, ap_id, av_id);
    }
    Some(subject)
}
```

called as `translator.ontology_header(ontology);` at the top of `owl2rdf`, before the existing
axiom loop (the `Option` return is unused there today, kept for a future caller).

## Reverse-direction fix: `rdf_owl_translator` must not re-materialize the header as a spurious axiom

Emitting `<ontologyIRI> rdf:type owl:Ontology` exposed a latent gap on the read side, again found
by actually running the round-trip tests rather than anticipated up front:
`rdf_owl_translator::translator::extract_ontology_name` already correctly special-cases this
triple (has since before this PR) to populate `Ontology::version`/`directly_imports_documents` —
but `axiom_parser.rs`'s generic `rdf:type` dispatch (`extract_axiom`) had no matching special case,
so the same `<ontologyIRI> rdf:type owl:Ontology` triple *also* fell through to the catch-all
"ClassAssertion" arm, producing a bogus
`ClassAssertion(ClassName(owl:Ontology), NamedIndividual(ontologyIRI))` axiom — i.e. "the ontology
IRI is an individual of class `owl:Ontology`" — alongside the correct header extraction. Fixed by
adding an explicit `o if o == ids.owl_ontology_id => None` arm in `axiom_parser.rs`'s `extract_axiom`,
right before the generic ClassAssertion fallback, so the triple is consumed exactly once (by
`extract_ontology_name`) and never double-materialized as an axiom. `owl:versionIRI`/`owl:imports`
triples needed no equivalent fix — they were never in `axiom_structural_predicate_ids`'s scanned
predicate set to begin with, so `extract_axioms_indexed` never saw them at all.

## Pre-existing bug found and fixed in the same PR

`ingress::namespaces::OWL_VERSION_IRI` is currently defined as
`"http://www.w3.org/2002/07/owl#versionIri"` (lowercase `Iri`) — the correct OWL 2 predicate,
per the spec fetched above and per every real-world ontology fixture already checked into this
repo (`tests/testdata/owl-time.ttl`, `LIS-14.ttl`, `prov-o.ttl`, all of which use
`owl:versionIRI` with a capital `IRI`), is `owl#versionIRI`. This is not a naming-convention
nitpick: `rdf_owl_translator/src/ingress.rs` looks up this exact constant when parsing RDF back
into an `Ontology`, so a real Turtle file's `owl:versionIRI` triple has never actually been picked
up — it silently fails the `iri_id` lookup and the version IRI is lost on ingestion. Fixed as part
of this PR (not deferred) since correctly implementing #515's `owl:versionIRI` *emission* requires
using the correct predicate IRI in the first place, otherwise the new code would just be
introducing the same wrong casing on the writer side too. Also fixed the matching log message in
`rdf_owl_translator/src/translator.rs`. No test data hardcoded the wrong casing, so this is a
pure bugfix with no compensating breakage.

## Effect on existing `owl2rdf` unit tests

None, by design. Every existing `owl_to_rdf.rs` unit test goes through the `translate(axioms)`
helper, which wraps axioms in
`Ontology::new(vec![], OntologyVersion::UnNamedOntology, vec![], axioms)` — a bare anonymous
ontology, exactly the case `ontology_header` deliberately suppresses (see above). Their
`assert_eq!(report.triples_added, N)` counts are unchanged.

## Test cases (initially `#[ignore]`d, one unignored per implementation step)

All in `owl2rl2datalog/src/owl_to_rdf.rs`'s existing `tests` module, following its established
helpers (`ex`, `full`, `has_triple`, `object_of`, `translate`/`id_of`). A new
`ontology_translate(ontology)` helper (parallel to `translate(axioms)`, but taking a full
`Ontology` so version/imports/annotations can vary) and a `header_node(ds)` helper (find the
single node typed `owl:Ontology`, for the anonymous-subject cases where there's no IRI to look up
by) are added.

1. **Named ontology, IRI only, no version, no imports** — `<ontologyIRI> a owl:Ontology` and
   nothing else header-related.
2. **Named ontology with `versionIRI`** — both the type triple and
   `<ontologyIRI> owl:versionIRI <versionIRI>`.
3. **Named ontology with one or more `owl:imports`** — one `owl:imports` triple per entry in
   `directly_imports_documents`, in order.
4. **Anonymous ontology (no IRI at all)** — the type triple lands on a fresh blank node (found via
   `header_node`), not on any IRI; still emits `owl:imports` triples off that blank node if any
   are declared (this is the case the spec explicitly calls out — the blank node is reused as the
   subject of every header triple, not just the type declaration).
5. **Combination**: named ontology with `versionIRI` *and* multiple imports *and* an ontology-level
   annotation, all together — checks the triples don't interfere/overwrite each other.
6. **Bare anonymous ontology** (no IRI, no imports, no annotations) — emits *nothing*: no header
   triple at all, `triples_added` stays 0. This is the deviation from the literal spec base case
   described above.

## Scope boundary

Ontology-level annotations are translated as **plain, non-reified** triples (see above) — nested
annotations *on* an ontology annotation (`Annotation(Annotation(...) AP av)`) are out of scope
here, same as the existing axiom-annotation machinery only reifies one level.
