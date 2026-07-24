# Manchester Syntax wiring: CLI + kernel plan

Tracking issue: [#161](https://github.com/daghovland/rdf-datalog/issues/161)
(follow-up to parser [#139](https://github.com/daghovland/rdf-datalog/issues/139),
serializer [#160](https://github.com/daghovland/rdf-datalog/issues/160), ABox
wiring [#159](https://github.com/daghovland/rdf-datalog/issues/159)/PR #175).

## Constraint that shapes everything below

Manchester TBox axioms (`SubClassOf:`, domain/range, etc.) never become RDF
triples today — only `owl2rl2datalog::assert_abox` materialises the ABox
portion of a parsed `Ontology` into `Datastore` quads. General TBox→RDF
materialisation is [#177](https://github.com/daghovland/rdf-datalog/issues/177)
(open, not started). Consequence: `rdf_owl_translator::rdf2owl(datastore)` can
**never** recover a Manchester ontology's TBox from the store, because it was
never written there as triples. Any load path for `.omn` that doesn't call
`owl2rl2datalog::owl2datalog` on the freshly-parsed `Ontology` at parse time
loses that TBox permanently — it can't be "reasoned later" the way Turtle-
sourced ontologies can (their axioms live as triples `rdf2owl` re-extracts on
every `run_owlrl_reasoning`/`%%reason` call).

## Scope for this PR

1. **`dagalog::load_file`** — add a `.omn` branch that parses the file via
   `manchester_parser::parse` and calls `owl2rl2datalog::assert_abox` to add
   its ABox as quads. This covers `.omn` arriving via `--data`/`load_file`'s
   general contract ("add quads to the store") without silently running a
   reasoning pass inside a function whose contract is just loading. TBox is
   *not* compiled to rules here — a `.omn` loaded as plain data with no `-o`
   ontology flag gets ABox facts only, same tier of support as any RDF file
   loaded without `-o`.

2. **`dagalog::apply_ontologies`** — special-case `.omn` paths inside the
   existing loop instead of routing them through `load_file`: parse once,
   `assert_abox` the ABox, and accumulate `owl2datalog(&mut datastore.resources,
   &ontology)` rules into a `Vec<Rule>`. Non-`.omn` paths keep using
   `load_file` (quads only) as today. After the loop: extract axioms via
   `rdf2owl` (covers RDF-native ontology files), compute their rules,
   concatenat with the accumulated Manchester rules, and call
   `datalog::evaluate_rules` **once** over the combined set. `ReasoningStats`
   sums axiom/rule counts from both sources. This ordering guarantee is what
   makes cross-file interaction correct (e.g. a Manchester TBox constraining
   ABox facts asserted by a separately-loaded Turtle file, or vice versa).

3. **`dagalog-kernel` `%%manchester`** — new cell magic, file-path form only
   (mirrors `%%rml <path>`/`%%ottr <path>`; no inline-source form since
   Manchester documents are normally whole-ontology files, not one-line
   snippets — can be added later if wanted). Executor: parse, `assert_abox`,
   `owl2datalog`, `evaluate_rules`, all at once — same reasoning as above:
   `%%reason` run later cannot recover the TBox, so the magic must apply
   reasoning immediately, unlike `%%turtle` (whose triples *are* visible to a
   later `%%reason`). Comment this asymmetry inline.

## Deferred to a follow-up issue (unlabeled, filed separately)

- **`sparql_endpoint` Graph Store Protocol content negotiation for
  Manchester.** GSP's model is "upload/download the triples of a graph."
  Uploading `.omn` today can only ever produce the ABox as quads (TBox has
  nowhere to live as RDF until #177). That makes a PUT → GET round trip
  *lossy*: the schema silently disappears. Wiring this in now would either
  (a) silently drop TBox axioms on every Manchester upload with no signal to
  the client, or (b) require inventing an ad-hoc side-channel for TBox rules
  outside the Graph Store Protocol's data model, which isn't clearly
  specified by the issue and risks near-term rework once #177 lands and
  Manchester TBox axioms have a real RDF representation to round-trip
  through. Deferring is the more honest option per issue #161's own scope
  note allowing item 3 to be dropped if "significantly more
  speculative/underspecified."

## Test plan

- `dagalog` integration test: a `.omn` fixture with both a `SubClassOf:` TBox
  axiom and a `ClassAssertion`-producing `Individual: ... Types: ...` ABox
  frame, loaded via `apply_ontologies`. Assert the store contains the
  **inferred** triple (individual typed as the *superclass*, not just the
  asserted subclass) — this is the only fact that can only appear if both
  `assert_abox` (ABox → quads) and `owl2datalog` + `evaluate_rules` (TBox →
  rules → materialisation) ran end to end.
- `dagalog` unit test for `load_file` with a `.omn` path (no `-o`): asserts
  only the ABox quad is present (no TBox-derived inference, since `load_file`
  doesn't compile rules).
- `dagalog-kernel` test for `detect_cell_type` recognising `%%manchester
  <path>`, plus an executor test (mirroring `ottr.rs`'s tests) that loads a
  fixture and asserts the inferred triple is present after one cell.
