# ADR-0001: nom for the SPARQL and Datalog parsers

**Status:** Accepted  
**Date:** 2025

## Context

The SPARQL 1.2 grammar is large and context-sensitive in places (e.g., aggregate vs.
non-aggregate contexts, expression precedence). We needed a parser combinator or parser
generator that would let us build incrementally, test each sub-grammar in isolation,
and produce good error messages.

The main candidates were:

- **nom** — combinator library; parsers are plain Rust functions; composable; zero-copy
- **pest** — PEG grammar in a separate `.pest` file; generates a parse tree; separate
  tool in the build chain
- **LALRPOP** — LR(1) grammar file; generates a Rust module at build time; good for
  unambiguous grammars

## Decision

Use **nom** for both the SPARQL parser (`sparql_parser`) and the Datalog parser
(`datalog_parser`).

## Rationale

- **Incremental development:** nom parsers are ordinary Rust functions. We can add
  SPARQL features one at a time by writing new combinators, without touching a monolithic
  grammar file.
- **Testability:** Each sub-parser can be unit-tested independently with `assert_eq!`.
  pest and LALRPOP require running the full grammar to test a sub-rule.
- **No build-time code generation:** pest and LALRPOP add a proc-macro / build.rs step.
  nom has no build-time overhead.
- **Good fit for SPARQL's expression grammar:** SPARQL expression precedence is
  straightforward to encode with nom's recursive descent style.

## Trade-offs

- nom error messages are less readable than pest's out of the box. We mitigate this
  with `nom_language::error::VerboseError` and custom error conversion.
- The grammar lives entirely in Rust code, which is harder for non-Rust contributors to
  read than a standalone `.pest` file.
