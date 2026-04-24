---
name: dagalog CLI tool
description: Design and status of the dagalog CLI — what flags exist, what's wired, what's pending
type: project
---

# dagalog CLI (`src/main.rs` + `src/lib.rs`)

The root `dagalog` crate doubles as both a library (`src/lib.rs`) and a binary (`src/main.rs`).

## Pipeline

Load data → (optionally load OWL ontologies + apply datalog reasoning) → run SPARQL query → print results

## CLI flags (clap derive)

| Flag | Short | Description |
|------|-------|-------------|
| `--data <FILE>` | `-d` | Turtle/TriG data file(s) — repeatable |
| `--ontology <FILE>` | `-o` | OWL ontology Turtle file(s) — repeatable; triggers reasoning |
| `--rules <FILE>` | `-r` | Datalog rules file(s) — **not yet supported** (datalog_parser stub only) |
| `--query-file <FILE>` | `-q` | SPARQL query file |
| `--query <SPARQL>` | `-Q` | Inline SPARQL query string |
| `--format <FMT>` | `-f` | Output format: `table` (default), `csv`, `json` |
| `--verbose` | `-v` | Print pipeline statistics to stderr |

## Library API (`src/lib.rs`)

- `load_turtle(datastore, path)` — load Turtle/TriG into datastore
- `load_and_reason(datastore, ontology_paths)` — load OWL ontologies and materialise
- `run_sparql(datastore, query_str) -> SelectResult` — execute SPARQL SELECT
- `format_table(result) -> String` — ASCII table output
- `format_csv(result) -> String` — CSV output
- `format_json(result) -> String` — SPARQL results JSON

## Status (2026-04-24)

- Data loading (Turtle): done
- OWL ontology loading + reasoning: done
- SPARQL SELECT: done
- `--rules` flag: present but immediately returns error (datalog_parser is a stub)
- TriG named-graph loading: wired via `parse_trig` in `turtle_parser`
- Integration tests: `tests/cli_integration.rs`

## Datalog parser notes (future work)

The `datalog_parser` crate is a stub (`parse()` always errors).
To implement: translate `DagSemTools.Datalog.Parser` from F# similarly to how
the SPARQL parser and turtle parser were translated.  The grammar is in
`grammars/datalog/Datalog.g4` (if it exists in the F# repo).
The parser should produce `Vec<datalog::types::Rule>` from a text input.

**Why:** datalog rules extend the OWL-RL reasoning with custom inference, useful for domain-specific reasoning beyond OWL-RL.
**How to apply:** when implementing `datalog_parser`, wire it into `src/lib.rs`'s `load_rules_file()` function and the `--rules` CLI flag.
