# #588: `ObjectPropertyDomain`/`ObjectPropertyRange` missing `Vec<Annotation>`

Follow-up from [#514](https://github.com/daghovland/rdf-datalog/issues/514)
(owl2rdf annotation axioms and axiom annotations). See
[decision point in `provenance/summaries/pr-571.ttl`](../../provenance/summaries/pr-571.ttl)
for why this was split out: `owl_ontology::ObjectPropertyAxiom::ObjectPropertyDomain`
and `::ObjectPropertyRange` are the only two axiom variants in the whole
`owl_ontology` axiom hierarchy without a leading `Vec<Annotation>` field, so
#514's `owl2rdf` reification hook had to keep the old unannotated `triple_p`
call for exactly these two arms.

## Scope

1. `owl_ontology/src/axioms.rs`: add `Vec<Annotation>` as the first tuple
   field of `ObjectPropertyAxiom::ObjectPropertyDomain` and
   `::ObjectPropertyRange`, matching every sibling variant (e.g.
   `DataPropertyAxiom::DataPropertyDomain(Vec<Annotation>, DataProperty,
   ClassExpression)`).
2. `owl2rl2datalog/src/owl_to_rdf.rs` (`Translator::object_property_axiom`):
   switch both arms from `triple_p` to `triple_p_annotated`, remove the
   now-stale comment explaining the gap, add a test mirroring
   `subclassof_with_annotation_is_reified_via_owl_axiom` for
   `ObjectPropertyDomain`.
3. `owl2rl2datalog/src/lib.rs` (`object_property_axiom2datalog`): update the
   match patterns to bind (and ignore, `_`) the new leading field, same as
   the existing `SubObjectPropertyOf(_, ..)` arms already do.
4. `rdf_owl_translator/src/axiom_parser.rs`: thread the already-computed
   `axiom_anns` into the `ObjectPropertyDomain`/`ObjectPropertyRange`
   constructors inside the `rdfs:domain`/`rdfs:range` closures (the data
   property sibling arms already do this — the object-property closures
   just weren't passing it).
5. `manchester_parser/src/frame.rs` (`object_property_frame`): the
   `Domain:`/`Range:` sections already parse per-item `Annotations:` blocks
   into `(Vec<Annotation>, ClassExpression)` pairs but discard the
   annotations (`_anns`) because the target type had nowhere to put them —
   thread them through now, mirroring `DataPropertySection::Domain`/`Range`
   handling immediately below it.
6. `manchester_parser/src/serialize.rs`
   (`classify_object_property_axiom`): pass the axiom's real annotations to
   `obj_prop_frame_line` instead of the hardcoded `&[]`.
7. Update every test/call site that constructs these two variants
   positionally: `owl2rl2datalog/src/owl_to_rdf.rs` tests,
   `owl2rl2datalog/src/lib.rs` test, `manchester_parser/tests/manchester_syntax.rs`,
   `tests/manchester_roundtrip.rs`'s AST-normalizing helper.

## Non-goals

- No change to the RDF mapping/reification mechanism itself (`triple_p_annotated`,
  `emit_axiom_annotations`) — those already exist from #514 and are reused as-is.
- No new Manchester Syntax grammar productions — `Domain:`/`Range:`
  `Annotations:` sub-blocks are already parsed; this just stops discarding
  the result.

## Prior provenance checked

- `owl_ontology/src/axioms.rs` and `crate:owl_ontology`: query tooling
  (`provenance/queries/run.sh`) was attempted but the shared
  `cargo-shared-target` build was contended by concurrent worktree builds
  and did not return in time. The relevant history was found directly via
  `provenance/summaries/pr-571.ttl` (#514's own summary), whose
  `session:pr571Decision` explicitly names this exact gap and issue #588 as
  the filed follow-up — this is the authoritative prior art for this PR.
