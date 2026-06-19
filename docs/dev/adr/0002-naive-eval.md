# ADR-0002: naive forward-chaining materialisation

**Status:** Accepted  
**Date:** 2025

## Context

Datalog evaluation has two main strategies:

- **Top-down / backward chaining** — evaluate rules lazily from the query goal; only
  derives facts needed to answer the current query; standard in Prolog.
- **Bottom-up / forward chaining** — eagerly materialise all derivable facts; subsequent
  queries are answered by scanning the materialised store; standard for OWL reasoning.

Within forward chaining there are further choices:

- **Naive:** re-evaluate all rules from scratch each iteration until fixed point.
- **Semi-naive:** track the "delta" (newly added facts) and only re-fire rules whose
  body could match new facts. Much more efficient for large datasets.
- **Incremental:** maintain the materialisation across data changes, propagating deltas
  forward without full re-materialisation.

## Decision

Use **naive forward-chaining** for the initial implementation.

## Rationale

- **Simplest correct implementation:** naive evaluation is easy to reason about,
  easy to test, and easy to extend (e.g., to add stratified negation).
- **OWL-RL use case:** the primary use case is materialising OWL-RL rules over a
  dataset that is loaded once and then queried many times. The materialisation cost
  is paid once at load time; query time is fast (the inferred triples are in the store).
- **Port fidelity:** this is a Rust port of DagSemTools (F#/.NET), which also uses
  naive forward-chaining. Matching the behaviour simplifies validation.

## Trade-offs

- Naive evaluation is O(|rules| × |facts|) per iteration. For large datasets with many
  rules, semi-naive or incremental evaluation would be significantly faster.
- Incremental maintenance across data mutations (e.g., SPARQL Update) is not possible
  with naive evaluation — a full re-materialisation is required.

## Future direction

Semi-naive and incremental Datalog maintenance are planned. See
[`docs/plans/PERSISTENCE_PLAN.md`](../../plans/PERSISTENCE_PLAN.md) for the roadmap.
The naive implementation will remain as a reference and fallback.
