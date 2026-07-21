/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Criterion benchmarks for the Gene Ontology pipeline.
//!
//! Requires `tests/testdata/go.ttl`.  Obtain it with:
//!
//! ```bash
//! bash scripts/download_test_ontologies.sh
//! ```
//!
//! Then run:
//! ```bash
//! cargo bench --bench gene_ontology
//! ```
//!
//! Compare against a saved baseline:
//! ```bash
//! cargo bench --bench gene_ontology -- --save-baseline before
//! # … make your change …
//! cargo bench --bench gene_ontology -- --baseline before
//! ```

use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
    measurement::WallTime,
};
use dag_rdf::Datastore;
use owl2rl2datalog::owl2datalog;
use rdf_owl_translator::rdf2owl;
use sparql_parser::{NetworkPolicy, ParserContext, QueryResult, execute, parse_query};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use turtle::parse_turtle;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn test_data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

/// Return `true` if the file exists; print a skip message and return `false` otherwise.
fn check_data(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    eprintln!(
        "\n[SKIP] Benchmark data not found: {}\n\
         Run `bash scripts/download_test_ontologies.sh` first.\n",
        path.display()
    );
    false
}

/// Parse `go.ttl` into a fresh `Datastore`.
fn load_go() -> Datastore {
    let path = test_data("go.ttl");
    let mut ds = Datastore::new(2_000_000);
    let file = File::open(&path).expect("go.ttl must be readable");
    parse_turtle(&mut ds, BufReader::new(file)).expect("Turtle parse must succeed");
    ds
}

fn run_sparql_query(ds: &Datastore, query_str: &str) -> usize {
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
        base: None,
    };
    let query = parse_query(query_str, &mut ctx)
        .expect("query must parse")
        .1;
    match execute(&query, ds, NetworkPolicy::Deny).expect("query must execute") {
        QueryResult::Select(r) => r.rows.len(),
        _ => panic!("expected SELECT result"),
    }
}

// ── Parse benchmark ───────────────────────────────────────────────────────────

fn bench_parse(c: &mut Criterion) {
    let path = test_data("go.ttl");
    if !check_data(&path) {
        return;
    }

    c.bench_function("gene_ontology/parse", |b| {
        b.iter(|| {
            let mut ds = Datastore::new(2_000_000);
            let file = File::open(&path).unwrap();
            parse_turtle(&mut ds, BufReader::new(file)).unwrap();
            ds
        });
    });
}

// ── RDF → OWL benchmark ───────────────────────────────────────────────────────

fn bench_rdf2owl(c: &mut Criterion) {
    let path = test_data("go.ttl");
    if !check_data(&path) {
        return;
    }

    // Parse once; clone before each measurement so rdf2owl starts from a fresh store.
    let base_ds = load_go();

    c.bench_function("gene_ontology/rdf2owl", |b| {
        b.iter_batched(
            || base_ds.clone(),
            |mut ds| rdf2owl(&mut ds),
            BatchSize::LargeInput,
        );
    });
}

// ── OWL → Datalog rules benchmark ─────────────────────────────────────────────

fn bench_owl2datalog(c: &mut Criterion) {
    let path = test_data("go.ttl");
    if !check_data(&path) {
        return;
    }

    // Parse + extract axioms once.  Only the GraphElementManager needs cloning
    // per iteration because owl2datalog takes &mut resources.
    let mut base_ds = load_go();
    let ontology_doc = rdf2owl(&mut base_ds);

    c.bench_function("gene_ontology/owl2datalog", |b| {
        b.iter_batched(
            || base_ds.resources.clone(),
            |mut resources| owl2datalog(&mut resources, &ontology_doc.ontology),
            BatchSize::LargeInput,
        );
    });
}

// ── SPARQL query benchmarks ────────────────────────────────────────────────────
//
// Queries run on the parsed (non-materialised) store, so these measure the
// SPARQL executor rather than the reasoner.

fn bench_sparql(c: &mut Criterion) {
    let path = test_data("go.ttl");
    if !check_data(&path) {
        return;
    }

    let ds = load_go();

    let queries: &[(&str, &str)] = &[
        (
            "select_limit10",
            "SELECT ?s ?p ?o WHERE { ?s ?p ?o } LIMIT 10",
        ),
        (
            "subClassOf_limit100",
            "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?a ?b WHERE { ?a rdfs:subClassOf ?b } LIMIT 100",
        ),
        (
            "subclasses_of_biological_process",
            "PREFIX obo: <http://purl.obolibrary.org/obo/>\n\
             PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?c WHERE { ?c rdfs:subClassOf obo:GO_0008150 }",
        ),
        (
            "labels_limit200",
            "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
             SELECT ?t ?l WHERE { ?t rdfs:label ?l } LIMIT 200",
        ),
    ];

    let mut group: BenchmarkGroup<WallTime> = c.benchmark_group("gene_ontology/sparql");
    for (name, query_str) in queries {
        group.bench_with_input(BenchmarkId::new("query", name), query_str, |b, qs| {
            b.iter(|| run_sparql_query(&ds, qs));
        });
    }
    group.finish();
}

// ── Criterion wiring ──────────────────────────────────────────────────────────

criterion_group!(
    gene_ontology,
    bench_parse,
    bench_rdf2owl,
    bench_owl2datalog,
    bench_sparql
);
criterion_main!(gene_ontology);
