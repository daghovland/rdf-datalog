/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! API-level performance integration tests.
//!
//! These tests exercise the full pipeline:
//!   Turtle parse → RDF→OWL translation → Datalog rule generation
//!   → materialisation → SPARQL query
//!
//! They require large ontology files that are **not** committed to the
//! repository. Download them first:
//!
//! ```bash
//! bash scripts/download_test_ontologies.sh
//! ```
//!
//! Then run with:
//! ```bash
//! cargo test --test performance -- --ignored --nocapture
//! ```
//!
//! All tests in this file are marked `#[ignore]` so they are skipped by the
//! normal `cargo test` run.

use dag_rdf::Datastore;
use datalog::{DatalogProgram, RulePartitioner, evaluate_rules};
use datalog_parser::parse_file as parse_datalog_file;
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use sparql_parser::{ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;
use turtle::{parse_ntriples, parse_turtle};

/// Read current resident set size from /proc/self/status (Linux only).
fn rss_mb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            let kb: u64 = line
                .split_whitespace()
                .nth(1)
                .unwrap_or("0")
                .parse()
                .unwrap_or(0);
            return kb / 1024;
        }
    }
    0
}

fn report_mem(label: &str, rss: u64) {
    println!("  {:<40} {:>8} MB RSS", label, rss);
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Path to a test data file relative to the workspace root.
fn test_data(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

/// Return `true` when the test data file exists; `false` to skip.
fn ensure_test_data(path: &Path) -> bool {
    if path.exists() {
        true
    } else {
        eprintln!(
            "\n[SKIP] Test data not found: {}\n\
             Run `bash scripts/download_test_ontologies.sh` to download it.\n",
            path.display()
        );
        false
    }
}

/// Print a labelled timing line.
fn report(label: &str, elapsed_ms: u128) {
    println!("  {:<40} {:>8} ms", label, elapsed_ms);
}

// ── Full pipeline helper ──────────────────────────────────────────────────────

struct PipelineResult {
    triple_count: usize,
    axiom_count: usize,
    rule_count: usize,
    pre_reasoning_quad_count: usize,
    post_reasoning_quad_count: usize,
}

/// Run the full parse → translate → reason pipeline on `path`, printing
/// per-phase timings.  Returns counts for assertion.
fn run_pipeline(path: &Path) -> (Datastore, PipelineResult) {
    println!("\n  File: {}", path.display());
    println!("  {}", "-".repeat(60));

    // ── Phase 1: Parse Turtle ────────────────────────────────────────────────
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(path).expect("test data file must be readable");
    parse_turtle(&mut datastore, BufReader::new(file)).expect("Turtle parse must succeed");
    let triple_count = datastore.named_graphs.quad_count;
    report("Turtle parse", t0.elapsed().as_millis());
    println!("    triples loaded:        {}", triple_count);

    // ── Phase 2: RDF → OWL translation ──────────────────────────────────────
    let t1 = Instant::now();
    let ontology_doc = rdf2owl(&mut datastore);
    let ontology = &ontology_doc.ontology;
    let axiom_count = ontology.axioms.len();
    report("RDF → OWL translation", t1.elapsed().as_millis());
    println!("    OWL axioms extracted:  {}", axiom_count);

    // ── Phase 3: OWL → Datalog rules ────────────────────────────────────────
    let t2 = Instant::now();
    let rules = owl2datalog(&mut datastore.resources, ontology);
    let rule_count = rules.len();
    report("OWL → Datalog rules", t2.elapsed().as_millis());
    println!("    Datalog rules:         {}", rule_count);

    let pre_reasoning_quad_count = datastore.named_graphs.quad_count;

    // ── Phase 4: Materialisation ─────────────────────────────────────────────
    let t3 = Instant::now();
    evaluate_rules(rules, &mut datastore);
    let post_reasoning_quad_count = datastore.named_graphs.quad_count;
    report("Datalog materialisation", t3.elapsed().as_millis());
    println!("    quads before:          {}", pre_reasoning_quad_count);
    println!("    quads after:           {}", post_reasoning_quad_count);
    println!(
        "    inferred:              {}",
        post_reasoning_quad_count.saturating_sub(pre_reasoning_quad_count)
    );

    (
        datastore,
        PipelineResult {
            triple_count,
            axiom_count,
            rule_count,
            pre_reasoning_quad_count,
            post_reasoning_quad_count,
        },
    )
}

/// Run a SPARQL SELECT query against `datastore` and return the result rows.
fn run_sparql(
    datastore: &Datastore,
    query_str: &str,
) -> Vec<HashMap<String, dag_rdf::GraphElement>> {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let query = parse_query(query_str, &mut ctx)
        .expect("SPARQL query must parse")
        .1;
    match execute(&query, datastore).expect("SPARQL execution must succeed") {
        QueryResult::Select(r) => r.rows,
        QueryResult::Ask(_) | QueryResult::Construct(_) | QueryResult::Describe(_) => {
            panic!("expected SELECT result")
        }
    }
}

// ── Gene Ontology memory-profile diagnostic ──────────────────────────────────

/// Memory profile for the Gene Ontology pipeline up to (but not including)
/// materialisation.  Reports RSS after each phase so we can identify which
/// phase causes the OOM.
///
/// Safe to run: no materialisation means no quad explosion.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_memory_profile() {
    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — memory profile (no materialisation) ===");
    println!("  {}", "-".repeat(60));

    let rss0 = rss_mb();
    report_mem("baseline", rss0);

    // Phase 1: parse
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).expect("readable");
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse ok");
    let triple_count = datastore.named_graphs.quad_count;
    let rss1 = rss_mb();
    report("Turtle parse", t0.elapsed().as_millis());
    report_mem("after parse", rss1);
    println!("    triples: {}", triple_count);

    // Phase 2: RDF → OWL
    let t1 = Instant::now();
    let ontology_doc = rdf2owl(&mut datastore);
    let ontology = &ontology_doc.ontology;
    let axiom_count = ontology.axioms.len();
    let rss2 = rss_mb();
    report("RDF → OWL", t1.elapsed().as_millis());
    report_mem("after rdf2owl", rss2);
    println!("    axioms: {}", axiom_count);

    // Phase 3: OWL → Datalog rules
    let t2 = Instant::now();
    let rules = owl2datalog(&mut datastore.resources, ontology);
    let rule_count = rules.len();
    let rss3 = rss_mb();
    report("OWL → Datalog rules", t2.elapsed().as_millis());
    report_mem("after rule gen", rss3);
    println!("    rules: {}", rule_count);

    // Phase 4: stratification
    let t3 = Instant::now();
    let stratifier = RulePartitioner::new(rules);
    let stratification = stratifier.order_rules();
    let strata_count = stratification.len();
    let rss4 = rss_mb();
    report("Stratification", t3.elapsed().as_millis());
    report_mem("after stratification", rss4);
    println!("    strata: {}", strata_count);

    // Phase 5: build rule_map (DatalogProgram::new) — no materialisation
    let t4 = Instant::now();
    let mut programs: Vec<DatalogProgram> = Vec::new();
    for partition in stratification {
        programs.push(DatalogProgram::new(partition));
    }
    let rss5 = rss_mb();
    report("DatalogProgram::new (rule_map)", t4.elapsed().as_millis());
    report_mem("after rule_map build", rss5);

    println!();
    println!("  Memory deltas:");
    println!("    parse          {:>+6} MB", rss1 as i64 - rss0 as i64);
    println!("    rdf2owl        {:>+6} MB", rss2 as i64 - rss1 as i64);
    println!("    rule gen       {:>+6} MB", rss3 as i64 - rss2 as i64);
    println!("    stratification {:>+6} MB", rss4 as i64 - rss3 as i64);
    println!("    rule_map build {:>+6} MB", rss5 as i64 - rss4 as i64);
    println!("    TOTAL          {:>+6} MB", rss5 as i64 - rss0 as i64);
    println!();

    assert!(triple_count > 100_000);
    assert!(axiom_count > 0);
    assert!(rule_count > 0);
    // Hold programs alive so their memory is attributed above.
    drop(programs);
}

// ── Gene Ontology performance test ────────────────────────────────────────────

/// Full pipeline over the Gene Ontology.
///
/// The Gene Ontology (go.ttl, ~89 MB, ~1.7 M triples) is the canonical
/// large-scale benchmark for OWL reasoners and SPARQL endpoints.
///
/// Pipeline:
/// 1. Parse Turtle              — measures I/O + parsing throughput
/// 2. RDF → OWL translation    — measures axiom extraction
/// 3. OWL → Datalog rules      — measures rule generation
/// 4. Materialisation           — measures forward-chaining inference
/// 5. SPARQL queries            — measures query answering over inferred data
///
/// To download go.ttl:
/// ```bash
/// bash scripts/download_test_ontologies.sh
/// ```
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_full_pipeline() {
    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — full pipeline ===");
    let t_total = Instant::now();

    let (datastore, stats) = run_pipeline(&path);

    // ── Phase 5: SPARQL queries ──────────────────────────────────────────────
    println!();

    // 5a. Count all rdfs:subClassOf triples (direct + inferred)
    let q_subclass = r#"PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?child ?parent
WHERE { ?child rdfs:subClassOf ?parent }
LIMIT 1000"#;
    let t4 = Instant::now();
    let rows = run_sparql(&datastore, q_subclass);
    report("SPARQL: subClassOf (LIMIT 1000)", t4.elapsed().as_millis());
    println!("    rows returned:         {}", rows.len());

    // 5b. Find direct subclasses of GO:0008150 (biological_process)
    let q_bio = r#"PREFIX obo: <http://purl.obolibrary.org/obo/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?subclass
WHERE { ?subclass rdfs:subClassOf obo:GO_0008150 }"#;
    let t5 = Instant::now();
    let bio_rows = run_sparql(&datastore, q_bio);
    report(
        "SPARQL: subclasses of biological_process",
        t5.elapsed().as_millis(),
    );
    println!("    subclasses found:      {}", bio_rows.len());

    // 5c. Find direct subclasses of GO:0003674 (molecular_function)
    let q_mol = r#"PREFIX obo: <http://purl.obolibrary.org/obo/>
PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?subclass
WHERE { ?subclass rdfs:subClassOf obo:GO_0003674 }"#;
    let t6 = Instant::now();
    let mol_rows = run_sparql(&datastore, q_mol);
    report(
        "SPARQL: subclasses of molecular_function",
        t6.elapsed().as_millis(),
    );
    println!("    subclasses found:      {}", mol_rows.len());

    // 5d. Retrieve GO terms with labels (rdfs:label)
    let q_labels = r#"PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>
SELECT ?term ?label
WHERE { ?term rdfs:label ?label }
LIMIT 500"#;
    let t7 = Instant::now();
    let label_rows = run_sparql(&datastore, q_labels);
    report(
        "SPARQL: terms with labels (LIMIT 500)",
        t7.elapsed().as_millis(),
    );
    println!("    rows returned:         {}", label_rows.len());

    // ── Summary ──────────────────────────────────────────────────────────────
    println!();
    report("TOTAL", t_total.elapsed().as_millis());

    // ── Assertions ───────────────────────────────────────────────────────────
    // The GO has > 40 000 terms, so we should have a lot of triples.
    assert!(
        stats.triple_count > 100_000,
        "expected >100k triples, got {}",
        stats.triple_count
    );
    // OWL axioms should be extracted.
    assert!(
        stats.axiom_count > 0,
        "expected OWL axioms to be extracted, got 0"
    );
    // Rules should be generated.
    assert!(
        stats.rule_count > 0,
        "expected Datalog rules to be generated, got 0"
    );
    // Reasoning should have inferred at least some new triples.
    assert!(
        stats.post_reasoning_quad_count >= stats.pre_reasoning_quad_count,
        "reasoning must not reduce the quad count"
    );
    // SPARQL must return results.
    assert!(
        !rows.is_empty(),
        "expected subClassOf triples to be present after parsing"
    );
    // Biological process should have subclasses.
    assert!(
        !bio_rows.is_empty(),
        "expected subclasses of GO_0008150 (biological_process)"
    );
}

/// Parse-only benchmark for the Gene Ontology.
///
/// Isolates Turtle parsing throughput without the reasoning overhead, useful
/// for tracking parser regressions independently.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_parse_only() {
    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — parse only ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).unwrap();
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse must succeed");
    let elapsed = t0.elapsed();
    let triples = datastore.named_graphs.quad_count;
    println!("  triples: {}", triples);
    println!("  elapsed: {} ms", elapsed.as_millis());
    println!(
        "  throughput: {:.0} triples/sec",
        triples as f64 / elapsed.as_secs_f64()
    );
    assert!(triples > 100_000);
}

/// OWL-axiom extraction and Datalog rule generation benchmark.
///
/// Focuses on the `rdf_owl_translator` + `owl2rl2datalog` phases, after
/// parsing has already completed.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_axiom_extraction() {
    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — axiom extraction ===");
    let file = File::open(&path).unwrap();
    let mut datastore = Datastore::new(2_000_000);
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse must succeed");
    println!("  triples loaded: {}", datastore.named_graphs.quad_count);

    let t1 = Instant::now();
    let ontology_doc = rdf2owl(&mut datastore);
    let ontology = &ontology_doc.ontology;
    println!(
        "  OWL axioms:     {} ({} ms)",
        ontology.axioms.len(),
        t1.elapsed().as_millis()
    );

    let t2 = Instant::now();
    let rules = owl2datalog(&mut datastore.resources, ontology);
    println!(
        "  Datalog rules:  {} ({} ms)",
        rules.len(),
        t2.elapsed().as_millis()
    );

    assert!(!ontology.axioms.is_empty(), "expected OWL axioms");
    assert!(!rules.is_empty(), "expected Datalog rules");

    // Print the first 10 rules and the first 5 quads so we can diagnose graph/predicate matching.
    println!("  --- first 10 rules ---");
    for r in rules.iter().take(10) {
        println!("    {}", r);
    }
    println!("  --- first 5 input quads (via resource IDs) ---");
    for q in datastore.named_graphs.quad_list.iter().take(5) {
        println!(
            "    g={} s={} p={} o={}",
            q.triple_id, q.subject, q.predicate, q.obj
        );
    }
}

/// SPARQL query performance over a fully materialised Gene Ontology.
///
/// Measures query latency for several representative query patterns.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_sparql_queries() {
    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — SPARQL query benchmark ===");

    // Build the materialised store once, then run multiple queries.
    let (datastore, _stats) = run_pipeline(&path);
    println!();

    let queries: &[(&str, &str)] = &[
        (
            "SELECT * (LIMIT 10)",
            "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10",
        ),
        (
            "subClassOf (LIMIT 100)",
            "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?a ?b WHERE { ?a rdfs:subClassOf ?b } LIMIT 100",
        ),
        (
            "type owl:Class (LIMIT 100)",
            "PREFIX owl: <http://www.w3.org/2002/07/owl#>\n\
             SELECT ?c WHERE { ?c a owl:Class } LIMIT 100",
        ),
        (
            "subclasses of biological_process",
            "PREFIX obo: <http://purl.obolibrary.org/obo/>\n\
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?c WHERE { ?c rdfs:subClassOf obo:GO_0008150 }",
        ),
        (
            "subclasses of cellular_component",
            "PREFIX obo: <http://purl.obolibrary.org/obo/>\n\
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?c WHERE { ?c rdfs:subClassOf obo:GO_0005575 }",
        ),
        (
            "labels (LIMIT 200)",
            "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?t ?l WHERE { ?t rdfs:label ?l } LIMIT 200",
        ),
    ];

    println!("  {:<45} {:>10}  Rows", "Query", "Time (ms)");
    println!("  {}", "-".repeat(65));
    for (label, query_str) in queries {
        let t = Instant::now();
        let rows = run_sparql(&datastore, query_str);
        println!(
            "  {:<45} {:>10}  {}",
            label,
            t.elapsed().as_millis(),
            rows.len()
        );
    }
}

// ── IMF ontology end-to-end pipeline ─────────────────────────────────────────
//
// The IMF (Industrial Modeling Framework) ontology is used internally and its
// OWL-RL rules exercise the full pipeline with a real-world industrial ontology.
//
// Download the ontology first:
//   bash scripts/download_test_ontologies.sh
//
// Then run:
//   cargo test --test performance imf -- --ignored --nocapture

/// Full pipeline over the IMF ontology:
///   Turtle parse → RDF→OWL translation → Datalog rule generation
///   → materialisation → SPARQL query
///
/// This replaces checking in a pre-generated large.datalog snapshot.
/// Testing the whole pipeline catches bugs in any stage, not just the parser.
#[test]
fn imf_ontology_full_pipeline() {
    let path = test_data("imf.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== IMF Ontology — full pipeline ===");
    let t_total = Instant::now();

    let (datastore, stats) = run_pipeline(&path);

    assert!(
        stats.rule_count > 100,
        "expected >100 Datalog rules from IMF ontology, got {}",
        stats.rule_count
    );
    assert!(
        stats.post_reasoning_quad_count >= stats.pre_reasoning_quad_count,
        "reasoning must not reduce the quad count"
    );

    // Verify known IMF class hierarchy is present after reasoning.
    let q_descriptor = r#"PREFIX imf: <http://ns.imfid.org/imf#>
SELECT ?x WHERE { ?x a imf:Descriptor }"#;
    let t = Instant::now();
    let rows = run_sparql(&datastore, q_descriptor);
    report("SPARQL: imf:Descriptor instances", t.elapsed().as_millis());
    println!("    rows returned:         {}", rows.len());

    report("TOTAL", t_total.elapsed().as_millis());
    println!();
}

/// Parse-only benchmark for the IMF ontology (no reasoning).
#[test]
fn imf_ontology_parse_only() {
    let path = test_data("imf.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== IMF Ontology — parse only ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(500_000);
    let file = File::open(&path).unwrap();
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse must succeed");
    let elapsed = t0.elapsed();
    let triples = datastore.named_graphs.quad_count;
    println!("  triples: {}", triples);
    println!("  elapsed: {} ms", elapsed.as_millis());

    assert!(triples > 0, "expected triples from IMF ontology");
}

/// Rule generation benchmark: OWL→Datalog only (no reasoning, no parsing).
#[test]
fn imf_ontology_rule_generation() {
    let path = test_data("imf.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== IMF Ontology — rule generation ===");
    let file = File::open(&path).unwrap();
    let mut datastore = Datastore::new(500_000);
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse must succeed");
    println!("  triples loaded: {}", datastore.named_graphs.quad_count);

    let t1 = Instant::now();
    let ontology_doc = rdf2owl(&mut datastore);
    let ontology = &ontology_doc.ontology;
    println!(
        "  OWL axioms:     {} ({} ms)",
        ontology.axioms.len(),
        t1.elapsed().as_millis()
    );

    let t2 = Instant::now();
    let rules = owl2datalog(&mut datastore.resources, ontology);
    println!(
        "  Datalog rules:  {} ({} ms)",
        rules.len(),
        t2.elapsed().as_millis()
    );

    assert!(
        !ontology.axioms.is_empty(),
        "expected OWL axioms from IMF ontology"
    );
    assert!(
        rules.len() > 100,
        "expected >100 Datalog rules, got {}",
        rules.len()
    );
}

/// Round-trip test: OWL→Datalog rule generation, then parse the generated
/// rules back through the Datalog parser.  Verifies that the rules our
/// pipeline produces are valid Datalog syntax.
///
/// This is the conceptual replacement for storing large.datalog in the repo:
/// we generate the rules on-the-fly and immediately verify they parse cleanly.
#[test]
fn imf_rules_generation_and_parsing_round_trip() {
    let path = test_data("imf.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== IMF Ontology — rules generation → Datalog parser round-trip ===");

    // Stage 1: generate rules from the ontology
    let mut gen_store = Datastore::new(500_000);
    let file = File::open(&path).unwrap();
    parse_turtle(&mut gen_store, BufReader::new(file)).expect("parse must succeed");
    let ontology_doc = rdf2owl(&mut gen_store);
    let generated_rules = owl2datalog(&mut gen_store.resources, &ontology_doc.ontology);
    println!("  generated {} Datalog rules", generated_rules.len());
    assert!(
        generated_rules.len() > 100,
        "expected >100 rules, got {}",
        generated_rules.len()
    );

    // Stage 2: serialise the generated rules to Datalog text
    let datalog_text: String = generated_rules.iter().map(|r| format!("{}\n", r)).collect();

    // Stage 3: parse the serialised text back and verify rule count matches
    let mut parse_store = Datastore::new(500_000);
    let parsed_rules = parse_datalog_file(
        // Write to a temp file so parse_file can read it
        {
            let tmp = std::env::temp_dir().join("imf_roundtrip.datalog");
            std::fs::write(&tmp, &datalog_text).expect("write temp file");
            tmp
        }
        .as_path(),
        &mut parse_store,
    )
    .expect("round-trip Datalog must parse without errors");

    println!("  re-parsed  {} Datalog rules", parsed_rules.len());
    assert_eq!(
        generated_rules.len(),
        parsed_rules.len(),
        "round-trip rule count must match"
    );
}

/// Materialisation progress diagnostic for the Gene Ontology.
///
/// Runs the pipeline up to materialisation, then executes at most MAX_ITER
/// semi-naive iterations, printing per-iteration stats (delta size, inferred
/// quad count, RSS, elapsed).  Stops early on iteration limit so the test
/// always returns in bounded time even if full convergence would OOM.
///
/// Use this test to understand how much inference each iteration produces
/// before committing to running the full pipeline.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn gene_ontology_materialise_progress() {
    const MAX_ITER: usize = 5;

    let path = test_data("go.ttl");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Gene Ontology — materialisation progress ({MAX_ITER} iterations max) ===");
    println!("  {}", "-".repeat(60));

    // ── Build up to DatalogProgram::new (same as memory_profile) ────────────
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).expect("readable");
    parse_turtle(&mut datastore, BufReader::new(file)).expect("parse ok");
    let ontology_doc = rdf2owl(&mut datastore);
    let rules = owl2datalog(&mut datastore.resources, &ontology_doc.ontology);
    let rule_count = rules.len();

    let stratifier = RulePartitioner::new(rules);
    let stratification = stratifier.order_rules();

    let mut programs: Vec<DatalogProgram> = stratification
        .into_iter()
        .map(DatalogProgram::new)
        .collect();

    report_mem("before materialisation", rss_mb());
    println!("    rules: {rule_count}  programs: {}", programs.len());
    println!();
    println!(
        "  {:>4}  {:>12}  {:>12}  {:>8}  {:>8}",
        "iter", "delta_in", "inferred", "store", "RSS MB"
    );
    println!("  {}", "-".repeat(60));

    // ── Iterate materialisation one step at a time ────────────────────────────
    // Seeds ground facts for each stratum first.
    for quad in programs.iter().flat_map(|p| p.materialise_seed_facts()) {
        datastore.named_graphs.add_quad(quad);
    }

    let mut delta_start: usize = 0;
    let mut total_inferred: usize = 0;

    // We only have one stratum (pure-positive GO), so use programs[0].
    let program = match programs.first_mut() {
        Some(p) => p,
        None => {
            println!("  (no rules — nothing to materialise)");
            return;
        }
    };

    for iter in 0..MAX_ITER {
        let t = Instant::now();
        let delta_in = datastore
            .named_graphs
            .quad_count
            .saturating_sub(delta_start);

        match program.materialise_one_iteration(&mut datastore, delta_start) {
            None => {
                println!("  Fixpoint reached after {iter} iterations.");
                break;
            }
            Some((new_start, inferred)) => {
                total_inferred += inferred;
                println!(
                    "  {:>4}  {:>12}  {:>12}  {:>8}  {:>8}  ({} ms)",
                    iter,
                    delta_in,
                    inferred,
                    datastore.named_graphs.quad_count,
                    rss_mb(),
                    t.elapsed().as_millis()
                );
                delta_start = new_start;
            }
        }
    }

    println!();
    println!("  Total inferred after {MAX_ITER} iterations: {total_inferred}");
    report_mem("after partial materialisation", rss_mb());
}

// ── Wikidata N-Triples sample ─────────────────────────────────────────────────
//
// Tests load `tests/testdata/wikidata-sample.nt`, a partial stream of the
// Wikidata truthy N-Triples dump (~1 M triples).  Download it first:
//
//   bash scripts/download_test_ontologies.sh
//
// Then run:
//   cargo test --test performance wikidata -- --ignored --nocapture

/// Parse-only benchmark for the Wikidata N-Triples sample.
///
/// Measures N-Triples parsing throughput on a real-world large knowledge base.
/// Wikidata uses fully qualified IRIs with no blank nodes in the truthy dump.
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn wikidata_parse_only() {
    let path = test_data("wikidata-sample.nt");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Wikidata — parse only ===");
    let t0 = Instant::now();
    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).unwrap();
    parse_ntriples(&mut datastore, BufReader::new(file)).expect("N-Triples parse must succeed");
    let elapsed = t0.elapsed();
    let triples = datastore.named_graphs.quad_count;
    println!("  triples: {}", triples);
    println!("  elapsed: {} ms", elapsed.as_millis());
    println!(
        "  throughput: {:.0} triples/sec",
        triples as f64 / elapsed.as_secs_f64()
    );
    assert!(
        triples > 100_000,
        "expected >100k triples from Wikidata sample, got {}",
        triples
    );
}

/// SPARQL query benchmark over the Wikidata N-Triples sample.
///
/// Loads the sample, then executes representative queries against the
/// Wikidata data model:
///   - Enumerate items (rdf:type wikibase:Item)
///   - Find instance-of (wdt:P31) statements
///   - Retrieve arbitrary triples
#[test]
#[ignore = "large file required — run `bash scripts/download_test_ontologies.sh` first"]
fn wikidata_sparql_queries() {
    let path = test_data("wikidata-sample.nt");
    if !ensure_test_data(&path) {
        return;
    }

    println!("\n=== Wikidata — SPARQL query benchmark ===");
    let t0 = Instant::now();

    let mut datastore = Datastore::new(2_000_000);
    let file = File::open(&path).expect("test data file must be readable");
    parse_ntriples(&mut datastore, BufReader::new(file)).expect("N-Triples parse must succeed");
    let triple_count = datastore.named_graphs.quad_count;
    report("N-Triples parse", t0.elapsed().as_millis());
    println!("    triples loaded:        {}", triple_count);
    println!();

    let queries: &[(&str, &str)] = &[
        (
            "SELECT * (LIMIT 10)",
            "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10",
        ),
        (
            "wikibase:Item instances (LIMIT 100)",
            "PREFIX wikibase: <http://wikiba.se/ontology#>\n\
             SELECT ?item WHERE { ?item a wikibase:Item } LIMIT 100",
        ),
        (
            "wdt:P31 instance-of (LIMIT 100)",
            "PREFIX wdt: <http://www.wikidata.org/prop/direct/>\n\
             SELECT ?item ?class WHERE { ?item wdt:P31 ?class } LIMIT 100",
        ),
        (
            "schema:about (LIMIT 100)",
            "PREFIX schema: <http://schema.org/>\n\
             SELECT ?dataset ?entity WHERE { ?dataset schema:about ?entity } LIMIT 100",
        ),
    ];

    println!("  {:<45} {:>10}  Rows", "Query", "Time (ms)");
    println!("  {}", "-".repeat(65));
    let mut all_rows: Vec<usize> = Vec::new();
    for (label, query_str) in queries {
        let t = Instant::now();
        let rows = run_sparql(&datastore, query_str);
        println!(
            "  {:<45} {:>10}  {}",
            label,
            t.elapsed().as_millis(),
            rows.len()
        );
        all_rows.push(rows.len());
    }

    report("TOTAL", t0.elapsed().as_millis());

    assert!(
        triple_count > 100_000,
        "expected >100k triples from Wikidata sample, got {}",
        triple_count
    );
    assert!(
        all_rows[0] > 0,
        "expected at least some triples from SELECT *"
    );
    assert!(
        all_rows[1] > 0,
        "expected wikibase:Item instances in the Wikidata sample"
    );
}
