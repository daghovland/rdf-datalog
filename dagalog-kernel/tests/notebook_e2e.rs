//! Headless integration tests that drive the real `dagalog-kernel` binary
//! over the Jupyter wire protocol, replaying `notebooks/dagalog_intro.ipynb`.
//! See docs/plans/NOTEBOOK_INTEGRATION_TEST_PLAN.md.

mod support;

use support::{KernelHarness, notebook_code_cells};

fn repo_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dagalog-kernel has a parent directory")
        .to_path_buf()
}

const TURTLE_CELL: &str = r#"%%turtle
@prefix ex:   <http://example.com/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .

ex:Alice a foaf:Person ; foaf:name "Alice" ; foaf:age 30 ; ex:worksFor ex:Acme .
ex:Bob   a foaf:Person ; foaf:name "Bob"   ; foaf:age 25 ; ex:worksFor ex:Acme .
ex:Acme  a ex:Company  ; foaf:name "Acme Corp" .
"#;

const SPARQL_SELECT_CELL: &str = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
PREFIX ex:   <http://example.com/>

SELECT ?person ?name ?age WHERE {
    ?person a foaf:Person ;
            foaf:name ?name ;
            foaf:age  ?age .
}
ORDER BY ?name
"#;

const RML_CELL: &str = "%%rml tests/testdata/rml_persons_mapping.ttl";

const REASON_CELL: &str = "%%reason";

const DATALOG_CELL_BRACKET: &str = r#"%%datalog
[?x, <http://example.com/colleague>, ?y] :-
    [?x, <http://example.com/worksFor>, ?org],
    [?y, <http://example.com/worksFor>, ?org] .
"#;

const COLLEAGUE_QUERY_CELL: &str = r#"PREFIX ex: <http://example.com/>

SELECT ?person ?colleague WHERE {
    ?person ex:colleague ?colleague .
    FILTER (?person != ?colleague)
}
ORDER BY ?person ?colleague
"#;

#[tokio::test]
async fn test_kernel_info_handshake() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_turtle_cell_loads_ten_triples() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let outcome = kernel.execute(TURTLE_CELL).await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("Loaded 10 triples."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_sparql_select_cell_returns_two_rows() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.execute(TURTLE_CELL).await;
    let outcome = kernel.execute(SPARQL_SELECT_CELL).await;
    assert_eq!(outcome.status, "ok");
    let rich = outcome
        .rich
        .expect("SELECT cell should produce rich output");
    assert_eq!(
        rich.get("text/plain").map(String::as_str),
        Some("2 result(s).")
    );
    let html = rich
        .get("text/html")
        .expect("SELECT cell should include text/html");
    assert!(html.contains("Alice"), "html should mention Alice: {html}");
    assert!(html.contains("Bob"), "html should mention Bob: {html}");
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_rml_cell_loads_six_triples() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.execute(TURTLE_CELL).await;
    let outcome = kernel.execute(RML_CELL).await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("Loaded 6 triples."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_reason_cell_adds_zero_triples() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.execute(TURTLE_CELL).await;
    kernel.execute(RML_CELL).await;
    let outcome = kernel.execute(REASON_CELL).await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(
        outcome.stream.as_deref(),
        Some("Reasoning complete. 0 triples added.")
    );
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_datalog_bracket_cell_materializes_colleagues() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.execute(TURTLE_CELL).await;
    let datalog_outcome = kernel.execute(DATALOG_CELL_BRACKET).await;
    assert_eq!(datalog_outcome.status, "ok");
    assert_eq!(datalog_outcome.stream.as_deref(), Some("Applied 1 rule."));

    let query_outcome = kernel.execute(COLLEAGUE_QUERY_CELL).await;
    let rich = query_outcome
        .rich
        .expect("SELECT cell should produce rich output");
    assert_eq!(
        rich.get("text/plain").map(String::as_str),
        Some("2 result(s).")
    );
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_validate_cell_reports_violations() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel
        .execute("%%load tests/testdata/shacl_s1_intro_data.ttl")
        .await;
    let outcome = kernel
        .execute("%%validate tests/testdata/shacl_s1_intro_shapes.ttl")
        .await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("4 violation(s)."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_validate_cell_reports_conforms() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel
        .execute("%%load tests/testdata/shacl_s2_target_subjects_data.ttl")
        .await;
    let outcome = kernel
        .execute("%%validate tests/testdata/shacl_s2_target_subjects_shapes.ttl")
        .await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("Conforms. 0 violation(s)."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_complete_request_over_zmq() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let reply = kernel
        .request(
            "complete_request",
            serde_json::json!({ "code": "SEL", "cursor_pos": 3 }),
        )
        .await;
    let matches: Vec<String> = reply["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    assert!(matches.contains(&"SELECT".to_string()));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_inspect_request_over_zmq() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let reply = kernel
        .request(
            "inspect_request",
            serde_json::json!({ "code": "REGEX", "cursor_pos": 2, "detail_level": 0 }),
        )
        .await;
    assert_eq!(reply["found"], serde_json::json!(true));
    let text = reply["data"]["text/plain"]
        .as_str()
        .expect("text/plain doc");
    assert!(text.contains("regular expression"));
    kernel.shutdown().await;
}

const OTTR_INLINE_CELL: &str = r#"%%ottr
@prefix ex:   <http://example.com/> .
@prefix ottr: <http://ns.ottr.xyz/0.4/> .
@prefix rdf:  <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix xsd:  <http://www.w3.org/2001/XMLSchema#> .

ex:Person [ ottr:IRI ?person, xsd:string ?name ] :: {
  ottr:Triple (?person, rdf:type,  foaf:Person),
  ottr:Triple (?person, foaf:name, ?name)
} .

ex:Person(<http://example.com/alice>, "Alice") .
ex:Person(<http://example.com/bob>,   "Bob") .
"#;

const OTTR_QUERY_CELL: &str = r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
SELECT ?person ?name WHERE {
    ?person a foaf:Person ;
            foaf:name ?name .
}
ORDER BY ?name
"#;

#[tokio::test]
async fn test_ottr_inline_cell_expands_triples() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    let outcome = kernel.execute(OTTR_INLINE_CELL).await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("Expanded 4 triples."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_ottr_inline_then_sparql_query() {
    let mut kernel = KernelHarness::start(&repo_root()).await;
    kernel.execute(OTTR_INLINE_CELL).await;
    let outcome = kernel.execute(OTTR_QUERY_CELL).await;
    assert_eq!(outcome.status, "ok");
    let rich = outcome
        .rich
        .expect("SELECT cell should produce rich output");
    assert_eq!(
        rich.get("text/plain").map(String::as_str),
        Some("2 result(s).")
    );
    let html = rich
        .get("text/html")
        .expect("SELECT cell should include text/html");
    assert!(html.contains("Alice"), "html should mention Alice: {html}");
    assert!(html.contains("Bob"), "html should mention Bob: {html}");
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_ottr_file_cell_expands_triples() {
    let root = repo_root();
    let mut kernel = KernelHarness::start(&root).await;
    let outcome = kernel
        .execute("%%ottr tests/testdata/person_ottr.stottr")
        .await;
    assert_eq!(outcome.status, "ok");
    assert_eq!(outcome.stream.as_deref(), Some("Expanded 4 triples."));
    kernel.shutdown().await;
}

#[tokio::test]
async fn test_full_notebook_replay() {
    let root = repo_root();
    let cells = notebook_code_cells(&root);

    // Code cells, in notebook order:
    //   0  step1-turtle          1  step2-sparql
    //   2  step3-rml             3  step4-owl-ontology
    //   4  step4-reason          5  step4-inferred-query
    //   6  step5-sparql          7  step6-datalog
    //   8  step6-query           9  step7-ottr
    //  10  step7-query
    let expected_streams: &[(usize, &str)] = &[
        (0, "Loaded 10 triples."),
        (2, "Loaded 6 triples."),
        (3, "Loaded 4 triples."),
        (4, "Reasoning complete. 1 triple added."),
        (7, "Applied 1 rule."),
        (9, "Expanded 4 triples."),
    ];
    let expected_rich_plain: &[(usize, &str)] = &[
        (1, "2 result(s)."),
        (5, "3 result(s)."),
        (6, "3 result(s)."),
        (8, "6 result(s)."),
        (10, "5 result(s)."),
    ];

    let mut kernel = KernelHarness::start(&root).await;
    let mut outcomes = Vec::with_capacity(cells.len());
    for cell in &cells {
        outcomes.push(kernel.execute(cell).await);
    }
    kernel.shutdown().await;

    for outcome in &outcomes {
        assert_eq!(
            outcome.status, "ok",
            "every cell should succeed: {outcome:?}"
        );
    }
    for &(idx, expected) in expected_streams {
        assert_eq!(
            outcomes[idx].stream.as_deref(),
            Some(expected),
            "cell {idx} stream mismatch"
        );
    }
    for &(idx, expected) in expected_rich_plain {
        let rich = outcomes[idx]
            .rich
            .as_ref()
            .unwrap_or_else(|| panic!("cell {idx} should produce rich output"));
        assert_eq!(
            rich.get("text/plain").map(String::as_str),
            Some(expected),
            "cell {idx} text/plain mismatch"
        );
    }
}
