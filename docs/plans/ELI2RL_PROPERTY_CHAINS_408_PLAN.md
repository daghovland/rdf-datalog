# Plan: implement property chains / inverse-in-hasValue in ELI→datalog translation

Issue: [#408](https://github.com/daghovland/rdf-datalog/issues/408)

## Background

PR #407 stopped `eli/src/eli2rl.rs` from panicking on three legal-but-unimplemented
constructs by returning `None` (skip the axiom, `log::warn!`) instead. This
PR implements them for real. Read `eli/src/eli2rl.rs` in full before starting
— this plan describes the target shape, but the current code (post-#407) is
the ground truth for exact signatures/call sites.

## Current shape (as of #407)

- `get_obj_prop_pattern(resources, prop, subject_var, object_var) -> Option<dag_rdf::QuadPattern>` — produces **one** quad pattern `(subject_var, role, object_var)` for a simple/named/anonymous property, recurses (swapping vars) for `InverseObjectProperty`, returns `None` for `ObjectPropertyChain`.
- `get_obj_value_pattern(resources, prop, subject_var, individual) -> Option<dag_rdf::QuadPattern>` — produces one quad pattern `(subject_var, role, <individual>)`, returns `None` for both `InverseObjectProperty` and `ObjectPropertyChain`.
- Callers of `get_obj_prop_pattern`: `translate_eli`'s `SomeValuesFrom` case, `get_universal_normalized_rule`, `get_at_most_one_normalized_rule` (twice, plus once more using the fixed `owl:sameAs` property in **head** position), `get_at_most_zero_normalized_rule`.
- Caller of `get_obj_value_pattern`: `get_object_has_value_normalized_rule` (head position).

## Target design

### `get_obj_prop_pattern` → returns a **chain of quad patterns**

Change return type to `Option<Vec<dag_rdf::QuadPattern>>` — an ordered list
of one or more join atoms connecting `subject_var` to `object_var`:

- `NamedObjectProperty`/`AnonymousObjectProperty` (unchanged behavior): `Some(vec![get_role_pattern(role_id, subject_var, object_var)])` — a chain of length 1.
- `InverseObjectProperty(inner)`: recurse with `subject_var`/`object_var` swapped, as today — now propagates a `Vec` instead of a single pattern. (Inverting a nested chain correctly would mean reversing chain order and inverting each link — out of scope; if `inner` is itself an `ObjectPropertyChain`, keep returning `None` with the existing `log::warn!`, i.e. `InverseObjectProperty(ObjectPropertyChain(...))` stays unimplemented. Only `InverseObjectProperty(NamedObjectProperty(...))`/`InverseObjectProperty(AnonymousObjectProperty(...))` need to work, which they already do via the existing recursion.)
- `ObjectPropertyChain(props)` — **new**: for a chain `p1, p2, ..., pn` connecting `subject_var` to `object_var`, introduce `n - 1` fresh intermediate variables and build one join atom per property: `subject_var -p1-> v1 -p2-> v2 -> ... -> v(n-1) -pn-> object_var`. Fresh variable names must not collide with existing rule variables — use a scheme like `format!("{subject_var}_{object_var}_chain{i}")` or similar (check what naming convention `translate_eli`'s existing `format!("{}_{}", var_name, clause)` uses for fresh-variable hygiene and stay consistent). Each individual link `pi` may itself be `NamedObjectProperty`/`AnonymousObjectProperty`/`InverseObjectProperty` (recurse via `get_obj_prop_pattern` for each link — reuse the function recursively rather than duplicating the match) — if any link fails to translate (e.g. a nested chain, which OWL 2 doesn't actually allow inside `ObjectPropertyChain`'s element list anyway per spec, but be defensive), the whole chain translation returns `None`. Concatenate all links' pattern-lists in order.
- Empty chain (`ObjectPropertyChain(vec![])`, a degenerate/malformed case): return `None` (nothing sensible to join).

### `get_obj_value_pattern` → also returns a **chain of quad patterns**

Change return type to `Option<Vec<dag_rdf::QuadPattern>>`, with the **last**
pattern's object fixed to `individual` instead of a free variable:

- `NamedObjectProperty`/`AnonymousObjectProperty` (unchanged): `Some(vec![get_role_value_pattern(resources, role_id, subject_var, individual)])`.
- `InverseObjectProperty(inner)` — **new**: `ObjectHasValue(ObjectInverseOf(P), a)` as a class expression means "the set of x such that a P x" (the individual is the **subject**, the class member is the **object**) — i.e. produce `get_default_graph_pattern(Term::Resource(<individual's id>), Term::Resource(<inner's role id>), Term::Variable(subject_var))` for a simple `inner`. Only handle `inner` being `NamedObjectProperty`/`AnonymousObjectProperty` (mirroring the chain-nesting restriction above) — `InverseObjectProperty(ObjectPropertyChain(...))` stays `None`.
- `ObjectPropertyChain(props)` — **new**: for `p1, ..., pn`, build the same free-variable join chain as `get_obj_prop_pattern` would for `subject_var` to a fresh final variable, **except** the last link's object is `individual` (fixed) instead of a fresh variable — i.e. reuse `get_obj_prop_pattern(resources, p_i, ..., ...)` for links `1..n-1` (free-variable joins) and `get_role_value_pattern`-style fixed-object construction for link `n` (or, more simply: build the full chain via `get_obj_prop_pattern`-equivalent logic ending at a fresh final var `vn`, then afterward can't just "fix" a pattern already built with a Term::Variable — better to write the last link's pattern directly with `Term::Resource(individual_id)` as its object rather than trying to patch a variable pattern after the fact). Consider extracting a shared private helper `build_chain_patterns(resources, props, subject_var, final_object: Term) -> Option<Vec<QuadPattern>>` used by BOTH `get_obj_prop_pattern`'s chain case (`final_object = Term::Variable(object_var)`) and `get_obj_value_pattern`'s chain case (`final_object = Term::Resource(individual_id)`) — this avoids duplicating the fresh-variable-chain-building logic twice. Empty chain → `None`.

### Callers — thread the `Vec` through

- `translate_eli`'s `SomeValuesFrom` case: currently `std::iter::once(role_triple).chain(concept_triples).collect()` assuming one role pattern — change to `role_triples.into_iter().chain(concept_triples).collect()` (role_triples is now the `Vec`).
- `get_universal_normalized_rule`, `get_at_most_zero_normalized_rule`: currently build one `RuleAtom::PositivePattern(get_obj_prop_pattern(...)?)` — change to map every pattern in the returned `Vec` into its own `RuleAtom::PositivePattern` and extend the body with all of them, not just one.
- `get_at_most_one_normalized_rule`'s two body-position calls (`p1`, `p2`): same treatment — each becomes a `Vec` of atoms pushed into the body, not a single atom.
- `get_at_most_one_normalized_rule`'s **head**-position call (`get_obj_prop_pattern(resources, &same_as, "Y1", "Y2")`): `same_as` is always a fixed, synthetic `NamedObjectProperty` (never a chain), so this always returns exactly one pattern — but a datalog rule head is structurally a single atom, so don't thread a `Vec` through the head position at all. Replace this one call with a direct `get_role_pattern(same_as_role_id, "Y1", "Y2")` (intern the `owl:sameAs` IRI directly, as this call site already does today via `let same_as_iri = ...`), sidestepping the need to unwrap a single-element `Vec` in head position entirely.
- `get_object_has_value_normalized_rule`'s head-position call to `get_obj_value_pattern`: **this one is different** — with chains/inverse now supported, the result can genuinely be a multi-pattern chain, but a rule head can only be one atom. Restructure `get_object_has_value_normalized_rule` similarly to `get_universal_normalized_rule`: split `get_obj_value_pattern`'s result into "all but the last pattern" (pushed into the rule **body** as `RuleAtom::PositivePattern`s, introducing the chain's intermediate variables) and "the last pattern" (used as the rule **head** — this is the one whose object is fixed to `individual`). Concretely: for a simple (non-chain, non-inverse) property this means body stays as just `sub_conjunction`'s type atoms (unchanged from today) and head is the single returned pattern (unchanged from today); for a chain of length > 1, the first `n-1` links become body atoms and the last (fixed-object) link becomes the head.

## Un-skip

Remove the `log::warn!` + early-`None` arms that are no longer reachable
(named/anonymous property chain and non-nested inverse cases) — keep
`log::warn!` + `None` only for the two still-genuinely-unsupported edge
cases (`InverseObjectProperty` wrapping an `ObjectPropertyChain`, in both
functions) and the degenerate empty-chain case, updating their messages/doc
comments accordingly and keeping the `issues/363` references only where
still accurate (add a note that this is the *remaining* unsupported case,
not link to #363 as if the whole cluster were still open — #363's eli2rl
sub-cluster is closed, tracked by this issue instead where relevant).

## Tests (TDD)

Update the existing `get_obj_prop_pattern_property_chain_returns_none` /
`get_obj_prop_pattern_inverse_of_chain_returns_none` /
`get_obj_value_pattern_inverse_returns_none` /
`get_obj_value_pattern_property_chain_returns_none` tests: three of these
four should now assert `Some(...)` with the *correct* pattern content
(chain length, variable names, subject/object placement) instead of `None`
— only the inverse-of-chain case should keep asserting `None`. Don't just
flip the assertion blindly; verify the actual pattern content is right
(e.g. a 2-link chain `p1∘p2` from X to Y should produce exactly 2 patterns:
`(X, p1, <fresh>)` and `(<fresh>, p2, Y)`, with the same fresh variable
name used as both the first pattern's object and the second pattern's
subject).

Add **materialisation-level** integration tests (not just "translation
returns `Some`") proving the generated rules actually derive correct facts
— this is the real correctness bar per the issue's own acceptance
criterion ("regression tests confirming rules are actually generated and
materialise correctly"):

- An ontology with `SubClassOf(A, ObjectSomeValuesFrom(ObjectPropertyChain(p1 p2) B))` (or equivalent Manchester/functional-syntax construction matching however this crate's existing eli2rl tests build `Formula`/`ComplexConcept` values by hand — check existing tests in this file and in `tests/owl_integration.rs`), materialised over data with `a p1 m`, `m p2 b`, `b rdf:type B`, asserting `a rdf:type A` is derived (proves the 2-hop join chain works end-to-end through the datalog reasoner, not just that translation type-checks).
- An ontology with `SubClassOf(A, ObjectHasValue(ObjectInverseOf(p), i))`, materialised over data with `i p a`, asserting `a rdf:type A` is derived.
- An ontology with `SubClassOf(A, ObjectHasValue(ObjectPropertyChain(p1 p2), i))`, materialised over data with `a p1 m`, `m p2 i`, asserting `a rdf:type A` is derived.
- A negative-control case for at least one of the above (data that does NOT satisfy the chain/inverse pattern) confirming the class membership is NOT derived — proves the join is actually selective, not vacuously true.
- Confirm the still-unsupported nested case (`InverseObjectProperty(ObjectPropertyChain(...))`) still returns `None`/logs a warning and doesn't panic, as a regression check that the narrowing of scope didn't accidentally break the safety property PR #407 established.

## Out of scope

`InverseObjectProperty` wrapping an `ObjectPropertyChain` (in either
function) stays unimplemented — track as a further follow-up if it turns
out to matter in practice (OWL 2's RDF mapping doesn't structurally permit
`ObjectPropertyChain` to appear as the direct target of `ObjectInverseOf`
in `SubObjectPropertyOf`/property-chain axioms in the first place, so this
combination may not be constructible from real Turtle-encoded OWL at all —
worth a one-line note in the code confirming/discussing this rather than
silently leaving a TODO with no explanation).
