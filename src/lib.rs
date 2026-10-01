/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! dagalog library — pipeline functions for loading RDF data, applying OWL-RL
//! reasoning, and executing SPARQL queries.
//!
//! The CLI binary (`main.rs`) is a thin wrapper around this library.

use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use owl2rl2datalog::{assert_abox, owl2datalog};
use rdf_owl_translator::rdf2owl;
use sparql_parser::{
    NetworkPolicy, ParserContext, QueryResult, SelectResult, execute_with_base, parse_query,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

// ── User-guide doctests ───────────────────────────────────────────────────────
//
// The Rust code fences in `docs/user/*.md` are wired up as real rustdoc
// doctests by re-exposing each file's contents as extra doc comments on this
// module, gated to `cfg(doctest)` so they never appear in normal `cargo doc`
// output and never affect ordinary builds. `dagalog` is the include point
// because it depends (directly or via `[dev-dependencies]`) on every crate
// these guides call out to — `jsonld_parser`, `ottr`, `rml`, `sparql_endpoint`,
// etc. — so the doctests can resolve every symbol they use.
//
// This module holds no code; it exists purely to carry the `doc` attributes.
// See [#167](https://github.com/daghovland/rdf-datalog/issues/167).
#[cfg_attr(doctest, doc = include_str!("../docs/user/deployment.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/formats.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/reasoning.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/rml-mapping.md"))]
#[cfg_attr(doctest, doc = include_str!("../docs/user/ottr-templates.md"))]
mod user_guide_doctests {}

// ── Output format ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Table,
    Csv,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "csv" => Ok(OutputFormat::Csv),
            "json" => Ok(OutputFormat::Json),
            other => Err(format!(
                "unknown format '{}': expected table, csv, or json",
                other
            )),
        }
    }
}

// ── Stats returned by apply_ontologies ────────────────────────────────────────

pub struct ReasoningStats {
    pub axiom_count: usize,
    pub rule_count: usize,
    pub triples_before: usize,
    pub triples_after: usize,
    /// Descriptions of ABox assertions that could not be materialised as a
    /// single ground triple (non-atomic class/property expressions) and were
    /// therefore skipped rather than reasoned over. Empty in the common case.
    /// Surfacing this (instead of only a `log::warn!`) is
    /// [#366](https://github.com/daghovland/rdf-datalog/issues/366); actually
    /// encoding such expressions as RDF is the separate, larger
    /// [#373](https://github.com/daghovland/rdf-datalog/issues/373).
    pub abox_skipped: Vec<String>,
}

// ── Data loading ──────────────────────────────────────────────────────────────

/// Load one RDF file into `datastore`.
///
/// Format is inferred from the file extension:
/// - `.trig` → TriG
/// - `.nt` → N-Triples
/// - `.nq` → N-Quads
/// - `.omn` → OWL 2 Manchester Syntax (ABox only — see below)
/// - `.ofn` → OWL 2 Functional-Style Syntax (ABox only — see below)
/// - `.owx` → OWL 2 XML Serialization (ABox only — see below)
/// - `.owl` → OWL 2 XML Serialization, but **only** when content-sniffing
///   (see [`frame_ontology_ext`]) confirms an `<Ontology>` XML root;
///   otherwise Turtle, since this repository's own test fixtures use `.owl`
///   for Turtle-serialized ontologies (the extension alone is ambiguous —
///   see [#609](https://github.com/daghovland/rdf-datalog/issues/609))
/// - everything else → Turtle
///
/// ## `.omn`/`.ofn`/`.owx`/`.owl` handling
///
/// A Manchester Syntax, Functional-Style Syntax, or OWL/XML Serialization
/// document is parsed into an [`owl_ontology::Ontology`] and only its ABox
/// assertions are materialised into `datastore` as ground quads, via
/// [`owl2rl2datalog::assert_abox`]. TBox axioms (`SubClassOf:`/
/// `SubClassOf(...)`/`<SubClassOf>`, property domain/range, …) are **not**
/// compiled to Datalog rules here — `load_file`'s contract elsewhere is "add
/// quads to the store," and running a full OWL-RL materialisation pass as a
/// side effect of a data load would be a surprise, especially since other
/// files in the same batch (loaded later, e.g. via a `--data` list) wouldn't
/// yet be visible to it. Callers that want the TBox reasoned over should
/// pass the file via [`apply_ontologies`] instead, which special-cases these
/// extensions to also call [`owl2datalog`] and evaluate the resulting rules
/// together with every other ontology source in one batch. See
/// [#161](https://github.com/daghovland/rdf-datalog/issues/161) (`.omn`),
/// [#633](https://github.com/daghovland/rdf-datalog/issues/633) (`.ofn`),
/// and [#609](https://github.com/daghovland/rdf-datalog/issues/609)
/// (`.owx`/`.owl`). `owl_xml_parser` does not yet parse ABox assertion
/// axioms ([#608](https://github.com/daghovland/rdf-datalog/issues/608) is
/// open), so a `.owx`/`.owl` document containing any will fail to parse with
/// a clear error rather than silently dropping them.
pub fn load_file(datastore: &mut Datastore, path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if let Some(frame_ext) = frame_ontology_ext(path)? {
        let ontology = parse_frame_ontology_file(frame_ext, path)?;
        let report = assert_abox(datastore, &ontology);
        // `load_file`'s signature is depended on by ~50 call sites across the
        // repo, so widening its return type to carry a skip report is out of
        // proportion to this fix. Print directly instead: this is
        // unconditional (not gated behind a verbose flag) because it flags
        // actual data loss, and unlike `log::warn!` (already emitted inside
        // `assert_abox`) it's observable by any caller capturing stderr
        // without configuring logging. See
        // [#366](https://github.com/daghovland/rdf-datalog/issues/366);
        // actually encoding the skipped constructs as RDF is the separate,
        // larger [#373](https://github.com/daghovland/rdf-datalog/issues/373).
        if !report.skipped.is_empty() {
            eprintln!(
                "warning: {} ABox assertion{} in {} skipped (not materialisable as a single \
                 ground triple; see issue #373): {}",
                report.skipped.len(),
                if report.skipped.len() == 1 { "" } else { "s" },
                path.display(),
                report.skipped.join("; ")
            );
        }
        return Ok(());
    }
    let file = File::open(path).map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    match ext {
        "trig" => turtle::parse_trig(datastore, reader)
            .map_err(|e| format!("TriG parse error in {}: {}", path.display(), e)),
        "nt" => turtle::parse_ntriples(datastore, reader)
            .map_err(|e| format!("N-Triples parse error in {}: {}", path.display(), e)),
        "nq" => turtle::parse_nquads(datastore, reader)
            .map_err(|e| format!("N-Quads parse error in {}: {}", path.display(), e)),
        _ => turtle::parse_turtle(datastore, reader)
            .map_err(|e| format!("Turtle parse error in {}: {}", path.display(), e)),
    }
}

/// Determine which (if any) frame-based/XML OWL parser should handle `path`,
/// returning a normalized extension tag (`"omn"`, `"ofn"`, or `"owx"`) for
/// [`parse_frame_ontology_file`], or `None` for RDF-native extensions that
/// `load_file`'s Turtle/TriG/N-Triples/N-Quads branches handle instead.
///
/// `.owl` is content-sniffed via [`owl_xml_parser::looks_like_owl_xml`]
/// rather than trusted outright: this repository's own test fixtures
/// (`tests/testdata/equality.owl` and friends) are Turtle-serialized
/// ontologies using the `.owl` extension, so routing every `.owl` file to
/// `owl_xml_parser` unconditionally would break them. A `.owl` file is only
/// treated as OWL/XML (normalized to `"owx"`, since both parse identically
/// once resolved) when its root XML element is literally `<Ontology>`. See
/// [#609](https://github.com/daghovland/rdf-datalog/issues/609).
fn frame_ontology_ext(path: &Path) -> Result<Option<&'static str>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    match ext {
        "omn" => Ok(Some("omn")),
        "ofn" => Ok(Some("ofn")),
        "owx" => Ok(Some("owx")),
        "owl" => {
            let src = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
            Ok(if owl_xml_parser::looks_like_owl_xml(&src) {
                Some("owx")
            } else {
                None
            })
        }
        _ => Ok(None),
    }
}

/// Read and parse a `.omn` (OWL 2 Manchester Syntax), `.ofn` (OWL 2
/// Functional-Style Syntax), or `.owx`/`.owl` (OWL 2 XML Serialization) file
/// into an [`owl_ontology::Ontology`]. `ext` must be `"omn"`, `"ofn"`, or
/// `"owx"` (as returned by [`frame_ontology_ext`]); all three parsers
/// produce the same `Ontology` type, so callers can treat any source
/// uniformly once parsed.
fn parse_frame_ontology_file(ext: &str, path: &Path) -> Result<owl_ontology::Ontology, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot open {}: {}", path.display(), e))?;
    match ext {
        "omn" => manchester_parser::parse(&src)
            .map_err(|e| format!("Manchester Syntax parse error in {}: {}", path.display(), e)),
        "ofn" => owl_functional_parser::parse(&src).map_err(|e| {
            format!(
                "OWL 2 Functional-Style Syntax parse error in {}: {}",
                path.display(),
                e
            )
        }),
        "owx" => owl_xml_parser::parse(&src).map_err(|e| {
            format!(
                "OWL 2 XML Serialization parse error in {}: {}",
                path.display(),
                e
            )
        }),
        other => unreachable!("parse_frame_ontology_file called with unsupported ext {other:?}"),
    }
}

// ── OWL reasoning ─────────────────────────────────────────────────────────────

/// Load OWL ontology files and apply OWL-RL materialisation to `datastore`.
///
/// Ontology triples are loaded into the same datastore as the data, then the
/// full RDF→OWL→Datalog→materialise pipeline is executed.
///
/// ## `.omn`/`.ofn`/`.owx`/`.owl` (Manchester / Functional-Style / OWL/XML) paths
///
/// Unlike [`load_file`] (which only materialises such a file's ABox),
/// `apply_ontologies` special-cases these extensions ([`frame_ontology_ext`]
/// decides which, content-sniffing `.owl`) so their TBox is actually
/// reasoned over: each is parsed once, its ABox is materialised via
/// [`owl2rl2datalog::assert_abox`], and its TBox is compiled to rules via
/// [`owl2datalog`] — accumulated alongside the rules compiled from every
/// RDF-native ontology file (Turtle/RDF-XML/JSON-LD, extracted via
/// [`rdf2owl`]) and evaluated together in one batch, after all paths have
/// been processed. This ordering matters: a frame-based/s-expression/XML
/// TBox axiom never becomes an RDF triple (that's [#177](https://github.com/daghovland/rdf-datalog/issues/177),
/// not yet done), so it can never be recovered from `datastore` by `rdf2owl`
/// after the fact — it must be compiled to rules at parse time or it is lost
/// entirely. See [#161](https://github.com/daghovland/rdf-datalog/issues/161)
/// (`.omn`), [#633](https://github.com/daghovland/rdf-datalog/issues/633)
/// (`.ofn`), and [#609](https://github.com/daghovland/rdf-datalog/issues/609)
/// (`.owx`/`.owl`).
///
/// Returns reasoning statistics (axiom count, rule count, triple delta) —
/// counts include both the Manchester and RDF-native ontology sources.
pub fn apply_ontologies(
    datastore: &mut Datastore,
    paths: &[std::path::PathBuf],
) -> Result<ReasoningStats, String> {
    let triples_before = datastore.named_graphs.quad_count;

    let compilation = compile_ontology_rules(datastore, paths)?;
    let rule_count = compilation.rules.len();

    datalog::evaluate_rules(compilation.rules, datastore).map_err(|e| e.to_string())?;

    let triples_after = datastore.named_graphs.quad_count;

    Ok(ReasoningStats {
        axiom_count: compilation.axiom_count,
        rule_count,
        triples_before,
        triples_after,
        abox_skipped: compilation.abox_skipped,
    })
}

/// Result of [`compile_ontology_rules`]: ABox triples have already been
/// materialised into the datastore (they're extensional input data), but the
/// TBox-derived Datalog rules are returned uncompiled/unevaluated so the
/// caller can decide how to materialise them.
pub struct OntologyCompilation {
    /// Total OWL axiom count (Manchester + Functional-Style + RDF-native
    /// ontology sources).
    pub axiom_count: usize,
    /// Datalog rules compiled from the ontologies' TBox axioms via
    /// [`owl2datalog`]. Not yet evaluated against the datastore.
    pub rules: Vec<datalog::Rule>,
    /// Descriptions of ABox assertions skipped by [`owl2rl2datalog::assert_abox`]
    /// across every `.omn`/`.ofn` source path (non-atomic class/property
    /// expressions that don't correspond to a single ground triple). Empty in
    /// the common case. See
    /// [#366](https://github.com/daghovland/rdf-datalog/issues/366) and
    /// [#373](https://github.com/daghovland/rdf-datalog/issues/373).
    pub abox_skipped: Vec<String>,
}

/// Load ontology files and compile their TBox axioms to Datalog rules,
/// WITHOUT materialising those rules against `datastore`.
///
/// ABox assertions ARE applied directly to `datastore` as ground quads (via
/// [`load_file`] for RDF-native sources and [`owl2rl2datalog::assert_abox`]
/// for `.omn`/`.ofn` sources) — they are extensional input data, not rule-derived,
/// so there is nothing to defer about them. Only the rule-COMPILATION step
/// (`owl2datalog`) is separated from evaluation here.
///
/// This lets a caller merge the returned rules into a larger `Vec<Rule>`
/// (e.g. alongside directly-supplied `.datalog`-file rules) and hand them
/// all to a single reasoner in one pass, rather than materialising the
/// ontology-derived triples through a separate, untracked
/// [`datalog::evaluate_rules`] call. That split was the root cause of
/// [#319](https://github.com/daghovland/rdf-datalog/issues/319): a
/// contradiction-triggered rebuild that only knows about one reasoner's own
/// tracked rules would silently discard intensional quads produced by an
/// earlier, separate materialisation pass it never registered.
///
/// [`apply_ontologies`] is the eager counterpart: it calls this function and
/// then immediately evaluates the returned rules, for callers (the one-shot,
/// non-`--serve` CLI path) that have no reasoner to unify with.
pub fn compile_ontology_rules(
    datastore: &mut Datastore,
    paths: &[std::path::PathBuf],
) -> Result<OntologyCompilation, String> {
    let mut frame_ontology_axiom_count = 0usize;
    let mut all_rules = Vec::new();
    let mut abox_skipped = Vec::new();

    for path in paths {
        if let Some(frame_ext) = frame_ontology_ext(path)? {
            let ontology = parse_frame_ontology_file(frame_ext, path)?;
            let report = assert_abox(datastore, &ontology);
            abox_skipped.extend(report.skipped);
            frame_ontology_axiom_count += ontology.axioms.len();
            all_rules.extend(owl2datalog(&mut datastore.resources, &ontology));
        } else {
            load_file(datastore, path)?;
        }
    }

    let ontology_doc = rdf2owl(datastore).map_err(|e| e.to_string())?;
    let ontology = &ontology_doc.ontology;
    let axiom_count = frame_ontology_axiom_count + ontology.axioms.len();

    all_rules.extend(owl2datalog(&mut datastore.resources, ontology));

    Ok(OntologyCompilation {
        axiom_count,
        rules: all_rules,
        abox_skipped,
    })
}

/// Run OWL-RL materialisation over the triples already in `datastore`.
///
/// Extracts OWL axioms from the current triple set, converts them to Datalog
/// rules, and runs naive forward-chaining to closure.  Returns the number of
/// triples added by the reasoning step.
///
/// Returns `Err` if the data is genuinely, correctly-derived inconsistent
/// (a `RuleHead::Contradiction` rule fires) instead of panicking — see
/// [#301](https://github.com/daghovland/rdf-datalog/issues/301).
pub fn run_owlrl_reasoning(datastore: &mut Datastore) -> Result<usize, String> {
    let before = datastore.named_graphs.quad_count;
    let ontology_doc = rdf2owl(datastore).map_err(|e| e.to_string())?;
    let rules = owl2datalog(&mut datastore.resources, &ontology_doc.ontology);
    datalog::evaluate_rules(rules, datastore).map_err(|e| e.to_string())?;
    Ok(datastore.named_graphs.quad_count - before)
}

// ── RML mapping ───────────────────────────────────────────────────────────────

/// Apply one or more RML mapping files to `datastore`.
///
/// For each mapping file, the source files referenced inside it are resolved
/// relative to that mapping file's parent directory. Mappings are applied in
/// order; triples from all mappings accumulate in the same datastore.
pub fn apply_rml_mappings(datastore: &mut Datastore, paths: &[PathBuf]) -> Result<(), String> {
    for path in paths {
        let base_dir = path
            .parent()
            .ok_or_else(|| format!("cannot determine parent directory of {}", path.display()))?;
        rml::apply_rml_mapping(path, base_dir, datastore)
            .map_err(|e| format!("RML mapping error in {}: {}", path.display(), e))?;
    }
    Ok(())
}

// ── OTTR templates ────────────────────────────────────────────────────────────

/// Expand one or more OTTR files (templates and/or instances) into `datastore`.
///
/// Each file is either stOTTR text syntax or wOTTR (RDF/Turtle), dispatched by
/// extension via [`ottr::load_ottr_file`] (`.ttl`/`.turtle`/`.trig` → wOTTR,
/// everything else → stOTTR). Templates and instances may be split across
/// files (of either format, mixed) or combined in a single file; all
/// documents are parsed, then merged and expanded together via
/// [`ottr::expand_documents`], so a template defined in one file can be
/// instantiated by instances in another.
pub fn apply_ottr_templates(datastore: &mut Datastore, paths: &[PathBuf]) -> Result<(), String> {
    let mut docs = Vec::with_capacity(paths.len());
    for path in paths {
        let doc = ottr::load_ottr_file(path)
            .map_err(|e| format!("OTTR error in {}: {}", path.display(), e))?;
        docs.push(doc);
    }
    ottr::expand_documents(&docs, datastore).map_err(|e| format!("OTTR expansion error: {}", e))
}

// ── Datalog rules ─────────────────────────────────────────────────────────────

/// Parse and apply Datalog rules from one or more `.datalog` files.
///
/// IRIs are interned into `datastore`; rules are then evaluated by naive
/// forward-chaining materialisation.  Returns the number of rules applied.
pub fn apply_rules(datastore: &mut Datastore, paths: &[PathBuf]) -> Result<usize, String> {
    let mut all_rules = Vec::new();
    for path in paths {
        let mut rules = datalog_parser::parse_file(path, datastore)?;
        all_rules.append(&mut rules);
    }
    let rule_count = all_rules.len();
    datalog::evaluate_rules(all_rules, datastore).map_err(|e| e.to_string())?;
    Ok(rule_count)
}

/// Parse Datalog rules from one or more `.datalog` files WITHOUT applying them.
///
/// IRIs are interned into `datastore` so resource IDs in the returned rules are
/// valid in that store.  The caller is responsible for materialisation (e.g.
/// by passing the rules to [`sparql_endpoint::Config::initial_rules`] for
/// incremental reasoning via the HTTP endpoint).
///
/// For one-shot (non-serve) use, prefer [`apply_rules`] which also evaluates.
pub fn parse_rules(
    datastore: &mut Datastore,
    paths: &[PathBuf],
) -> Result<Vec<datalog::Rule>, String> {
    let mut all_rules = Vec::new();
    for path in paths {
        let mut rules = datalog_parser::parse_file(path, datastore)?;
        all_rules.append(&mut rules);
    }
    Ok(all_rules)
}

/// Ontology-axiom stats reported by [`collect_serve_rules`] for `--verbose`
/// logging, mirroring the fields of [`OntologyCompilation`] that the CLI
/// prints. `0`/`0` when `ontology_paths` was empty.
#[derive(Default)]
pub struct ServeRulesStats {
    /// Total OWL axiom count extracted from `ontology_paths` (0 if none given).
    pub ontology_axiom_count: usize,
    /// Number of Datalog rules compiled from those axioms (0 if none given).
    pub ontology_rule_count: usize,
}

/// The `--serve`-mode counterpart of [`apply_ontologies`] + [`apply_rules`]:
/// collects ontology-derived (OWL2RL) rules and directly-supplied
/// `.datalog`-file rules into a single `Vec<Rule>`, WITHOUT materialising
/// either against `datastore`.
///
/// This is the fix for
/// [#319](https://github.com/daghovland/rdf-datalog/issues/319): previously,
/// the `--serve` CLI path ran ontology rule compilation through
/// [`apply_ontologies`] (which eagerly, untracked-ly evaluates them via a
/// standalone [`datalog::evaluate_rules`] call) and handed only the
/// `.datalog`-file rules to `IncrementalReasoner::new`. A
/// contradiction-triggered `rebuild_from_base` inside the server only
/// replays the reasoner's own tracked rules, so it silently dropped the
/// untracked OWL-RL-derived quads. By returning both rule sources merged
/// into one `Vec<Rule>` here, the caller hands them to a single
/// `IncrementalReasoner`/`Config::initial_rules` in one pass, so both share
/// one tracked `derived_from` index and survive rebuilds together.
///
/// ABox assertions from `ontology_paths` are still applied eagerly as ground
/// quads (see [`compile_ontology_rules`]) — only rule *evaluation* is
/// deferred to the caller.
pub fn collect_serve_rules(
    datastore: &mut Datastore,
    ontology_paths: &[PathBuf],
    rule_paths: &[PathBuf],
) -> Result<(Vec<datalog::Rule>, ServeRulesStats), String> {
    let mut serve_rules = Vec::new();
    let mut stats = ServeRulesStats::default();

    if !ontology_paths.is_empty() {
        let compilation = compile_ontology_rules(datastore, ontology_paths)?;
        stats.ontology_axiom_count = compilation.axiom_count;
        stats.ontology_rule_count = compilation.rules.len();
        serve_rules.extend(compilation.rules);
    }

    if !rule_paths.is_empty() {
        let mut rules = parse_rules(datastore, rule_paths)?;
        serve_rules.append(&mut rules);
    }

    Ok((serve_rules, stats))
}

// ── SPARQL ────────────────────────────────────────────────────────────────────

/// Execute a SPARQL SELECT query string against `datastore`.
pub fn run_sparql_query(datastore: &Datastore, sparql: &str) -> Result<SelectResult, String> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let (_, query) =
        parse_query(sparql, &mut ctx).map_err(|e| format!("SPARQL parse error: {:?}", e))?;
    // Pass the effective base (a `BASE <...>` directive, if any) through so
    // that `IRI()`/`URI()` can resolve a runtime string argument against it
    // at evaluation time, not just IRIs written directly in query syntax
    // (`ParserContext::base` handles those at parse time — #217). See #346.
    match execute_with_base(
        &query,
        datastore,
        NetworkPolicy::Deny,
        ctx.base.as_deref(),
        None,
    )
    .map_err(|e| e.to_string())?
    {
        QueryResult::Select(r) => Ok(r),
        QueryResult::Ask(_) => {
            Err("ASK queries are not supported via run_sparql_query".to_string())
        }
        QueryResult::Construct(_) => {
            Err("CONSTRUCT queries are not supported via run_sparql_query".to_string())
        }
        QueryResult::Describe(_) => {
            Err("DESCRIBE queries are not supported via run_sparql_query".to_string())
        }
    }
}

// ── Output formatting ─────────────────────────────────────────────────────────

/// Format SPARQL results as a string in the requested output format.
pub fn format_results(result: &SelectResult, format: &OutputFormat) -> String {
    match format {
        OutputFormat::Table => format_table(result),
        OutputFormat::Csv => format_csv(result),
        OutputFormat::Json => format_json(result),
    }
}

/// Render a `GraphElement` as a human-readable string.
pub fn graph_element_display(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => format!("<{}>", iri.0),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => format!("_:b{}", id),
        GraphElement::GraphLiteral(lit) => rdf_literal_display(lit),
        // Triple terms: display using interned IDs; full RDF 1.2 display tracked in #143.
        GraphElement::TripleTerm(k) => format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj),
    }
}

fn rdf_literal_display(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => format!("\"{}\"", s),
        RdfLiteral::LangLiteral { literal, lang } => format!("\"{}\"@{}", literal, lang),
        RdfLiteral::TypedLiteral { literal, type_iri } => {
            format!("\"{}\"^^<{}>", literal, type_iri.0)
        }
        RdfLiteral::BooleanLiteral(b) => b.to_string(),
        RdfLiteral::IntegerLiteral(n) => n.to_string(),
        RdfLiteral::DoubleLiteral(d) => d.to_string(),
        RdfLiteral::DecimalLiteral(d) => d.to_string(),
        RdfLiteral::FloatLiteral(f) => f.to_string(),
        RdfLiteral::DurationLiteral(d) => format!("{:?}", d),
        RdfLiteral::DateTimeLiteral(dt) => dt.to_string(),
        RdfLiteral::TimeLiteral(t) => t.to_string(),
        RdfLiteral::DateLiteral(d) => d.to_string(),
    }
}

fn format_table(result: &SelectResult) -> String {
    if result.variables.is_empty() {
        return "(no variables)\n".to_string();
    }

    let mut widths: Vec<usize> = result.variables.iter().map(|v| v.len() + 1).collect();
    for row in &result.rows {
        for (i, var) in result.variables.iter().enumerate() {
            let val = row
                .get(var)
                .map(graph_element_display)
                .unwrap_or_else(|| "(unbound)".to_string());
            widths[i] = widths[i].max(val.len());
        }
    }

    let mut out = String::new();

    for (i, var) in result.variables.iter().enumerate() {
        out.push_str(&format!(
            "{:<width$}  ",
            format!("?{}", var),
            width = widths[i]
        ));
    }
    out.push('\n');

    for w in &widths {
        out.push_str(&"-".repeat(w + 2));
    }
    out.push('\n');

    if result.rows.is_empty() {
        out.push_str("(no results)\n");
    } else {
        for row in &result.rows {
            for (i, var) in result.variables.iter().enumerate() {
                let val = row
                    .get(var)
                    .map(graph_element_display)
                    .unwrap_or_else(|| "(unbound)".to_string());
                out.push_str(&format!("{:<width$}  ", val, width = widths[i]));
            }
            out.push('\n');
        }
    }

    out
}

/// Return the raw lexical value of an element for plain-text contexts (CSV).
///
/// IRIs are returned without angle brackets, literals without RDF quoting.
/// This follows the SPARQL 1.1 CSV/TSV results format convention.
fn graph_element_raw_value(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(iri)) => iri.0.clone(),
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => format!("_:b{}", id),
        GraphElement::GraphLiteral(lit) => match lit {
            RdfLiteral::LiteralString(s) => s.clone(),
            RdfLiteral::LangLiteral { literal, .. } => literal.clone(),
            RdfLiteral::TypedLiteral { literal, .. } => literal.clone(),
            RdfLiteral::BooleanLiteral(b) => b.to_string(),
            RdfLiteral::IntegerLiteral(n) => n.to_string(),
            RdfLiteral::DoubleLiteral(d) => d.to_string(),
            RdfLiteral::DecimalLiteral(d) => d.to_string(),
            RdfLiteral::FloatLiteral(f) => f.to_string(),
            RdfLiteral::DurationLiteral(d) => format!("{:?}", d),
            RdfLiteral::DateTimeLiteral(dt) => dt.to_string(),
            RdfLiteral::TimeLiteral(t) => t.to_string(),
            RdfLiteral::DateLiteral(d) => d.to_string(),
        },
        // Triple terms: raw value is the display form (#143).
        GraphElement::TripleTerm(k) => format!("<<( {} {} {} )>>", k.subject, k.predicate, k.obj),
    }
}

fn format_csv(result: &SelectResult) -> String {
    let mut out = String::new();

    out.push_str(&result.variables.join(","));
    out.push('\n');

    for row in &result.rows {
        let values: Vec<String> = result
            .variables
            .iter()
            .map(|var| {
                let val = row
                    .get(var)
                    .map(graph_element_raw_value)
                    .unwrap_or_default();
                csv_escape(&val)
            })
            .collect();
        out.push_str(&values.join(","));
        out.push('\n');
    }

    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn format_json(result: &SelectResult) -> String {
    let vars: Vec<String> = result
        .variables
        .iter()
        .map(|v| format!("\"{}\"", json_escape(v)))
        .collect();

    let bindings: Vec<String> = result
        .rows
        .iter()
        .map(|row| {
            let pairs: Vec<String> = result
                .variables
                .iter()
                .filter_map(|var| {
                    let el = row.get(var)?;
                    Some(format!("\"{}\":{}", json_escape(var), element_to_json(el)))
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        })
        .collect();

    format!(
        "{{\"head\":{{\"vars\":[{}]}},\"results\":{{\"bindings\":[{}]}}}}",
        vars.join(","),
        bindings.join(",")
    )
}

fn element_to_json(el: &GraphElement) -> String {
    match el {
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri))) => {
            format!("{{\"type\":\"uri\",\"value\":\"{}\"}}", json_escape(iri))
        }
        GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) => {
            format!("{{\"type\":\"bnode\",\"value\":\"b{}\"}}", id)
        }
        GraphElement::GraphLiteral(lit) => literal_to_json(lit),
        // Triple terms: JSON representation tracked in #143.
        GraphElement::TripleTerm(k) => {
            format!(
                "{{\"type\":\"triple\",\"value\":\"<<( {} {} {} )>>\"}}",
                k.subject, k.predicate, k.obj
            )
        }
    }
}

fn literal_to_json(lit: &RdfLiteral) -> String {
    match lit {
        RdfLiteral::LiteralString(s) => {
            format!("{{\"type\":\"literal\",\"value\":\"{}\"}}", json_escape(s))
        }
        RdfLiteral::LangLiteral { literal, lang } => format!(
            "{{\"type\":\"literal\",\"xml:lang\":\"{}\",\"value\":\"{}\"}}",
            json_escape(lang),
            json_escape(literal)
        ),
        RdfLiteral::TypedLiteral { literal, type_iri } => format!(
            "{{\"type\":\"literal\",\"datatype\":\"{}\",\"value\":\"{}\"}}",
            json_escape(&type_iri.0),
            json_escape(literal)
        ),
        other => {
            let s = rdf_literal_display(other);
            format!("{{\"type\":\"literal\",\"value\":\"{}\"}}", json_escape(&s))
        }
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag_rdf::Datastore;

    const FAMILY_TTL: &str = r#"
@prefix ex: <http://example.org/family#> .
@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .

<http://example.org/family> a owl:Ontology .

ex:Person a owl:Class .
ex:Employee a owl:Class ;
    rdfs:subClassOf ex:Person .

ex:Alice a ex:Person ;
    ex:name "Alice" .

ex:Bob a ex:Employee ;
    ex:name "Bob" .
"#;

    fn load_family() -> Datastore {
        let mut ds = Datastore::new(10_000);
        turtle::parse_turtle(&mut ds, FAMILY_TTL.as_bytes()).expect("parse should succeed");
        ds
    }

    #[test]
    fn sparql_basic_query() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let persons: Vec<_> = result
            .rows
            .iter()
            .filter_map(|r| r.get("person"))
            .map(graph_element_display)
            .collect();
        assert!(persons.contains(&"<http://example.org/family#Alice>".to_string()));
        // Without reasoning Bob (an Employee) should NOT appear as a Person
        assert!(!persons.contains(&"<http://example.org/family#Bob>".to_string()));
    }

    #[test]
    fn sparql_with_reasoning() {
        let mut ds = Datastore::new(10_000);
        turtle::parse_turtle(&mut ds, FAMILY_TTL.as_bytes()).expect("parse should succeed");
        // Ontology IS the data file here; re-load for reasoning
        let ontology_doc = rdf2owl(&mut ds).unwrap();
        let rules = owl2datalog(&mut ds.resources, &ontology_doc.ontology);
        datalog::evaluate_rules(rules, &mut ds).unwrap();

        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let persons: Vec<_> = result
            .rows
            .iter()
            .filter_map(|r| r.get("person"))
            .map(graph_element_display)
            .collect();
        assert!(persons.contains(&"<http://example.org/family#Alice>".to_string()));
        // After reasoning: Bob is an Employee which is a subClassOf Person → Bob is a Person
        assert!(
            persons.contains(&"<http://example.org/family#Bob>".to_string()),
            "expected Bob to be inferred as a Person after OWL-RL reasoning; got: {:?}",
            persons
        );
    }

    #[test]
    fn format_table_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person ?name WHERE {
    ?person a ex:Person .
    ?person ex:name ?name .
}
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Table);
        assert!(output.contains("?person"));
        assert!(output.contains("?name"));
        assert!(output.contains("Alice"));
    }

    #[test]
    fn format_csv_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person ?name WHERE {
    ?person a ex:Person .
    ?person ex:name ?name .
}
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Csv);
        let mut lines = output.lines();
        assert_eq!(
            lines.next(),
            Some("person,name"),
            "first line should be header"
        );
        // Values should be raw (no RDF quoting): IRI without <>, literal without ""
        assert!(
            output.contains("Alice"),
            "should contain raw literal value Alice"
        );
        assert!(
            !output.contains(r#""""Alice""""#),
            "should not double-escape the literal"
        );
    }

    #[test]
    fn format_json_output() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?person WHERE { ?person a ex:Person . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        let output = format_results(&result, &OutputFormat::Json);
        assert!(
            output.starts_with("{\"head\":{\"vars\":"),
            "should be SPARQL JSON"
        );
        assert!(output.contains("\"person\""));
        assert!(output.contains("http://example.org/family#Alice"));
    }

    #[test]
    fn empty_result_table() {
        let ds = load_family();
        let sparql = r#"
PREFIX ex: <http://example.org/family#>
SELECT ?x WHERE { ?x a ex:NonExistentClass . }
"#;
        let result = run_sparql_query(&ds, sparql).expect("query should succeed");
        assert!(result.rows.is_empty());
        let output = format_results(&result, &OutputFormat::Table);
        assert!(output.contains("(no results)"));
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("hello, world"), "\"hello, world\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn json_escape_special_chars() {
        assert_eq!(json_escape("hello"), "hello");
        assert_eq!(json_escape("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(json_escape("line\nnewline"), "line\\nnewline");
    }

    // ── `.ofn` (OWL 2 Functional-Style Syntax) wiring, see #633 ─────────────

    const ANIMALS_OFN: &str = r#"
Prefix(:=<http://example.org/>)
Ontology(
    Declaration(Class(:Animal))
    Declaration(Class(:Dog))
    Declaration(NamedIndividual(:fido))
    SubClassOf(:Dog :Animal)
    ClassAssertion(:Dog :fido)
)
"#;

    fn write_ofn_fixture(dir: &std::path::Path, contents: &str) -> PathBuf {
        let p = dir.join("animals.ofn");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    fn fido_is_animal(ds: &Datastore) -> bool {
        let get = |iri: &str| {
            ds.resources
                .resource_map
                .get(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    iri.to_string(),
                ))))
                .copied()
        };
        let (fido, rdf_type, animal) = match (
            get("http://example.org/fido"),
            get("http://www.w3.org/1999/02/22-rdf-syntax-ns#type"),
            get("http://example.org/Animal"),
        ) {
            (Some(f), Some(t), Some(a)) => (f, t, a),
            _ => return false,
        };
        !ds.quads_matching(None, Some(fido), Some(rdf_type), Some(animal))
            .is_empty()
    }

    #[test]
    fn load_file_ofn_materialises_abox_only() {
        let tmp =
            std::env::temp_dir().join(format!("dagalog_ofn_load_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_ofn_fixture(&tmp, ANIMALS_OFN);

        let mut ds = Datastore::new(1_000);
        load_file(&mut ds, &path).expect("should load animals.ofn");

        // ABox (`fido a Dog`) is materialised directly...
        assert!(
            ds.resources
                .resource_map
                .contains_key(&GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
                    "http://example.org/fido".to_string()
                )))),
            "fido should be interned by load_file"
        );
        // ...but the TBox (`Dog SubClassOf Animal`) is NOT reasoned over by
        // load_file alone, so fido is not (yet) inferred as an Animal.
        assert!(
            !fido_is_animal(&ds),
            "load_file must not reason over the .ofn TBox"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn apply_ontologies_ofn_reasons_over_tbox() {
        let tmp =
            std::env::temp_dir().join(format!("dagalog_ofn_apply_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_ofn_fixture(&tmp, ANIMALS_OFN);

        let mut ds = Datastore::new(1_000);
        let stats = apply_ontologies(&mut ds, &[path]).expect("should apply .ofn ontology");
        assert!(stats.axiom_count > 0);
        assert!(stats.rule_count > 0);
        assert!(
            fido_is_animal(&ds),
            "apply_ontologies must reason over the .ofn TBox so fido is inferred as an Animal"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    // ── `.owx`/`.owl` (OWL 2 XML Serialization) wiring, see #609 ────────────
    //
    // `owl_xml_parser` does not yet parse ABox assertion axioms (#608 is
    // open), so unlike the `.ofn` fixture above there is no `ClassAssertion`
    // here — only a TBox (`Dog SubClassOf Animal`). The ABox fact (`fido a
    // Dog`) is asserted separately, directly into the datastore, so these
    // tests can still exercise "does apply_ontologies's compiled TBox rule
    // actually fire" without depending on #608.

    const ANIMALS_OWX: &str = r#"<?xml version="1.0"?>
<Ontology xmlns="http://www.w3.org/2002/07/owl#" ontologyIRI="http://example.org/animals">
    <Declaration><Class IRI="http://example.org/Animal"/></Declaration>
    <Declaration><Class IRI="http://example.org/Dog"/></Declaration>
    <SubClassOf>
        <Class IRI="http://example.org/Dog"/>
        <Class IRI="http://example.org/Animal"/>
    </SubClassOf>
</Ontology>
"#;

    fn write_owx_fixture(dir: &std::path::Path, contents: &str) -> PathBuf {
        let p = dir.join("animals.owx");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    fn write_owl_fixture(dir: &std::path::Path, contents: &str) -> PathBuf {
        let p = dir.join("animals.owl");
        std::fs::write(&p, contents).expect("write fixture");
        p
    }

    /// Insert `ex:fido a ex:Dog` directly into `ds`, bypassing any ontology
    /// parser — standing in for an ABox that (until #608) `.owx`/`.owl`
    /// cannot supply itself.
    fn assert_fido_is_dog(ds: &mut Datastore) {
        turtle::parse_turtle(
            ds,
            "@prefix ex: <http://example.org/> .\nex:fido a ex:Dog .\n".as_bytes(),
        )
        .expect("insert ABox fact");
    }

    #[test]
    fn load_file_owx_succeeds_on_tbox_only_ontology() {
        let tmp =
            std::env::temp_dir().join(format!("dagalog_owx_load_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_owx_fixture(&tmp, ANIMALS_OWX);

        let mut ds = Datastore::new(1_000);
        // Before #609, `.owx` fell through to the default Turtle branch and
        // would fail to parse this XML document at all.
        load_file(&mut ds, &path).expect("should load animals.owx");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn apply_ontologies_owx_reasons_over_tbox() {
        let tmp =
            std::env::temp_dir().join(format!("dagalog_owx_apply_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_owx_fixture(&tmp, ANIMALS_OWX);

        let mut ds = Datastore::new(1_000);
        assert_fido_is_dog(&mut ds);
        let stats = apply_ontologies(&mut ds, &[path]).expect("should apply .owx ontology");
        assert!(stats.axiom_count > 0);
        assert!(stats.rule_count > 0);
        assert!(
            fido_is_animal(&ds),
            "apply_ontologies must reason over the .owx TBox so fido is inferred as an Animal"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn apply_ontologies_owl_sniffed_as_owl_xml_reasons_over_tbox() {
        // Same fixture content as the `.owx` test above, but saved with the
        // ambiguous `.owl` extension: content-sniffing must still route it
        // to owl_xml_parser.
        let tmp = std::env::temp_dir().join(format!(
            "dagalog_owl_sniff_apply_test_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let path = write_owl_fixture(&tmp, ANIMALS_OWX);

        let mut ds = Datastore::new(1_000);
        assert_fido_is_dog(&mut ds);
        let stats = apply_ontologies(&mut ds, &[path])
            .expect("should sniff .owl as OWL/XML and apply it");
        assert!(stats.axiom_count > 0);
        assert!(stats.rule_count > 0);
        assert!(
            fido_is_animal(&ds),
            "apply_ontologies must reason over the sniffed .owl (OWL/XML) TBox"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// Regression coverage for the `.owl` ambiguity itself
    /// ([#609](https://github.com/daghovland/rdf-datalog/issues/609)):
    /// `tests/testdata/equality.owl` is a real, pre-existing Turtle-syntax
    /// fixture using the `.owl` extension. Content-sniffing must fall
    /// through to the ordinary Turtle loader for it, exactly as before this
    /// change — the new `.owx`/`.owl` OWL/XML routing must never swallow it.
    #[test]
    fn load_file_turtle_syntax_owl_fixture_still_loads_as_turtle() {
        let mut ds = Datastore::new(10_000);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/equality.owl");
        load_file(&mut ds, &path).expect("equality.owl (Turtle) should still load");
        assert!(
            ds.named_graphs.quad_count > 0,
            "equality.owl should have loaded some triples"
        );
    }
}
