/*
Copyright (C) 2025 Dag Hovland
This program is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version.
This program is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
You should have received a copy of the GNU General Public License along with this program. If not, see <https://www.gnu.org/licenses/>.
Contact: hovlanddag@gmail.com
*/

//! Integration tests for the Datalog parser and rule application pipeline.
//!
//! These tests exercise `datalog_parser::parse` / `parse_file` and
//! `dagalog::apply_rules` through the public library API.
//!
//! Test data files mirror the DagSemTools DatalogParser.Unit.Tests/TestData/
//! directory and are stored in tests/testdata/.

use dag_rdf::{Datastore, GraphElement, RdfLiteral};
use dagalog::{apply_rules, graph_element_display, load_file, run_sparql_query};
use datalog::types::{RuleAtom, RuleHead};
use sparql_parser::ast::{BinaryOp, Expression};
use std::path::Path;

fn testdata(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("testdata")
        .join(name)
}

fn ds() -> Datastore {
    Datastore::new(100_000)
}

// ── Parser correctness (translated from DagSemTools TestParser) ───────────────

#[test]
fn parse_single_rule() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("rule1.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1, "rule1.datalog should produce 1 rule");
}

#[test]
fn parse_rule_with_and() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("ruleand.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        rules[0].body.len(),
        2,
        "ruleand.datalog body should have 2 atoms"
    );
}

#[test]
fn parse_two_rules() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("tworules.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].body.len(), 2);
}

#[test]
fn parse_negation() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("rulenot.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].body.len(), 2);
    assert!(
        matches!(rules[0].body[1], RuleAtom::NotPattern(_)),
        "second body atom should be NOT"
    );
}

#[test]
fn parse_type_atom() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("ruletypeatom.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].body.len(), 2);
}

#[test]
fn parse_prefixes() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("prefixes.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].body.len(), 1);
    // The body predicate should be the expanded ex2:predicate2 IRI
    if let RuleAtom::PositivePattern(ref pat) = rules[0].body[0]
        && let dag_rdf::Term::Resource(id) = &pat.predicate
    {
        let iri = ds
            .resources
            .get_named_resource(*id)
            .expect("should be an IRI");
        assert!(
            iri.0.contains("predicate2"),
            "body predicate should contain 'predicate2', got {}",
            iri.0
        );
    }
}

#[test]
fn parse_all_variables_with_rdf_range() {
    // properties.datalog: [?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c]
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("properties.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].body.len(), 2);
    // Head must be a NormalHead
    assert!(matches!(rules[0].head, RuleHead::NormalHead(_)));
    // Head predicate must be rdf:type
    if let RuleHead::NormalHead(ref pat) = rules[0].head
        && let dag_rdf::Term::Resource(id) = &pat.predicate
    {
        let iri = ds.resources.get_named_resource(*id).unwrap();
        assert!(
            iri.0.ends_with("rdf-syntax-ns#type"),
            "head predicate should be rdf:type, got {}",
            iri.0
        );
    }
}

#[test]
fn parse_type_atom2() {
    // typeatom2.datalog: ex:type [?new_node] :- ex:type [?node] .
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("typeatom2.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].body.len(), 1);
}

#[test]
fn parse_contradiction() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("contradiction.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    assert!(
        matches!(rules[0].head, RuleHead::Contradiction),
        "head should be Contradiction"
    );
}

#[test]
fn parse_named_graph_rule() {
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("namedgraph.datalog"), &mut ds).unwrap();
    assert_eq!(rules.len(), 1);
    // Both head and body atom should have a graph variable
    if let RuleHead::NormalHead(ref pat) = rules[0].head {
        assert!(
            matches!(&pat.graph, dag_rdf::Term::Variable(v) if v == "graph"),
            "head graph should be variable 'graph'"
        );
    } else {
        panic!("expected NormalHead");
    }
    if let RuleAtom::PositivePattern(ref pat) = rules[0].body[0] {
        assert!(
            matches!(&pat.graph, dag_rdf::Term::Variable(v) if v == "graph"),
            "body graph should be variable 'graph'"
        );
    }
}

#[test]
#[ignore = "requires large.datalog — run `bash scripts/download_test_ontologies.sh` first"]
fn parse_large_file() {
    let path = testdata("large.datalog");
    if !path.exists() {
        eprintln!("[SKIP] large.datalog not found — run scripts/download_test_ontologies.sh");
        return;
    }
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&path, &mut ds).unwrap();
    assert!(
        rules.len() > 100,
        "large.datalog should produce >100 rules, got {}",
        rules.len()
    );
}

// ── End-to-end: rules applied to data, then queried via SPARQL ────────────────

#[test]
fn datalog_rules_infer_new_triples() {
    // Load family data (only Alice=Person, Bob=Employee, Charlie=Person)
    let mut ds = ds();
    load_file(&mut ds, &testdata("family.ttl")).unwrap();
    let triples_before = ds.named_graphs.quad_count;

    // Apply infer_employee.datalog: every Employee is a Person
    let rule_count = apply_rules(&mut ds, &[testdata("infer_employee.datalog")]).unwrap();
    assert!(rule_count > 0, "should have loaded at least one rule");

    let triples_after = ds.named_graphs.quad_count;
    assert!(
        triples_after > triples_before,
        "rules should have added triples (before={}, after={})",
        triples_before,
        triples_after
    );

    // Bob (Employee) should now be queryable as a Person
    let sparql = "PREFIX ex: <http://example.org/family#> SELECT ?p WHERE { ?p a ex:Person . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let persons: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("p"))
        .map(graph_element_display)
        .collect();

    assert!(
        persons.contains(&"<http://example.org/family#Alice>".to_string()),
        "Alice should be a Person"
    );
    assert!(
        persons.contains(&"<http://example.org/family#Bob>".to_string()),
        "Bob should be inferred as a Person via Datalog rules; got: {:?}",
        persons
    );
}

#[test]
fn datalog_rules_with_rdfs_range() {
    // properties.datalog: [?x, a, ?c] :- [?x, ?p, ?y], [?p, rdfs:range, ?c]
    // We need data that contains triples with rdfs:range declarations
    let ttl = r#"
@prefix ex: <https://example.com/data#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .

ex:hasAge rdfs:range ex:AgeValue .
ex:Alice ex:hasAge "30" .
"#;
    let mut ds = ds();
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let rules = datalog_parser::parse_file(&testdata("properties.datalog"), &mut ds).unwrap();
    datalog::evaluate_rules(rules, &mut ds);

    // SPARQL: Alice should now be typed as AgeValue (via range inference)
    let sparql = r#"
PREFIX ex: <https://example.com/data#>
SELECT ?x WHERE { ?x a ex:AgeValue . }
"#;
    let result = run_sparql_query(&ds, sparql).unwrap();
    assert!(
        !result.rows.is_empty(),
        "range inference should have added type triples; no ex:AgeValue instances found"
    );
}

#[test]
fn apply_rules_from_inline_string() {
    // Test parse() directly with inline Datalog
    let src = r#"
prefix ex: <https://example.com/test#>
ex:Mammal[?x] :- ex:Dog[?x] .
ex:Mammal[?x] :- ex:Cat[?x] .
"#;
    let ttl = r#"
@prefix ex: <https://example.com/test#> .
ex:Fido a ex:Dog .
ex:Whiskers a ex:Cat .
ex:Nobody a ex:Fish .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let rules = datalog_parser::parse(src, &mut ds).unwrap();
    assert_eq!(rules.len(), 2);
    datalog::evaluate_rules(rules, &mut ds);

    let sparql = "PREFIX ex: <https://example.com/test#> SELECT ?x WHERE { ?x a ex:Mammal . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let mammals: Vec<_> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();
    assert!(
        mammals.contains(&"<https://example.com/test#Fido>".to_string()),
        "Fido should be a Mammal"
    );
    assert!(
        mammals.contains(&"<https://example.com/test#Whiskers>".to_string()),
        "Whiskers should be a Mammal"
    );
    assert!(
        !mammals.contains(&"<https://example.com/test#Nobody>".to_string()),
        "Nobody (Fish) should NOT be a Mammal"
    );
}

#[test]
fn apply_rules_via_lib_api() {
    // Test the dagalog::apply_rules() library function
    let src = r#"prefix ex: <https://example.com/test#>
ex:Big[?x] :- ex:VeryBig[?x] ."#;
    let ttl = r#"@prefix ex: <https://example.com/test#> .
ex:Elephant a ex:VeryBig ."#;

    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let tmp = std::env::temp_dir().join("dagalog_test_rules.datalog");
    std::fs::write(&tmp, src).unwrap();
    let count = apply_rules(&mut ds, &[tmp]).unwrap();
    assert_eq!(count, 1);

    let sparql = "PREFIX ex: <https://example.com/test#> SELECT ?x WHERE { ?x a ex:Big . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    assert!(
        !result.rows.is_empty(),
        "Elephant should be inferred as Big"
    );
}

#[test]
fn contradiction_rule_parsed_but_does_not_panic() {
    // Contradiction rules generate no new triples; they're used for consistency checking.
    let mut ds = ds();
    let rules = datalog_parser::parse_file(&testdata("contradiction.datalog"), &mut ds).unwrap();
    // Should not panic during materialisation
    let ttl = r#"@prefix ex: <https://example.com/> . ex:Alice a ex:ValidClass ."#;
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();
    datalog::evaluate_rules(rules, &mut ds); // must not panic
}

// ── Stratified negation ───────────────────────────────────────────────────────

/// A program with a positive recursive cycle (ancestor) plus a negation of that
/// derived predicate (unrelated) must be accepted and produce correct results.
/// The negated rule must land in a strictly later stratum than the ancestor rules.
#[test]
fn stratified_negation_with_positive_recursion() {
    let src = r#"
prefix ex: <http://example.org/>
ex:ancestor[?x, ?y] :- ex:parent[?x, ?y] .
ex:ancestor[?x, ?z] :- ex:ancestor[?x, ?y], ex:parent[?y, ?z] .
ex:unrelated[?x, ?y] :- ex:person[?x], ex:person[?y], NOT ex:ancestor[?x, ?y] .
"#;
    let data = r#"
@prefix ex: <http://example.org/> .
ex:a ex:parent ex:b .
ex:b ex:parent ex:c .
ex:a a ex:person .
ex:b a ex:person .
ex:c a ex:person .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, data.as_bytes()).unwrap();
    let rules = datalog_parser::parse(src, &mut ds).unwrap();

    // Verify stratification: the unrelated rule (negates IDB) must be in a later stratum.
    let partitioner = datalog::RulePartitioner::new(rules.clone());
    let strata = partitioner.order_rules();
    assert!(
        strata.len() >= 2,
        "should produce ≥2 strata (ancestor strata then unrelated stratum), got {}",
        strata.len()
    );

    datalog::evaluate_rules(rules, &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x ?y WHERE { ?x ex:unrelated ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let pairs: Vec<(String, String)> = result
        .rows
        .iter()
        .filter_map(|r| {
            Some((
                graph_element_display(r.get("x")?),
                graph_element_display(r.get("y")?),
            ))
        })
        .collect();

    // b is not an ancestor of a → should be unrelated
    assert!(
        pairs.contains(&(
            "<http://example.org/b>".into(),
            "<http://example.org/a>".into()
        )),
        "b is not an ancestor of a → should be unrelated; got: {:?}",
        pairs
    );
    // a IS a direct parent of b → must NOT be unrelated
    assert!(
        !pairs.contains(&(
            "<http://example.org/a>".into(),
            "<http://example.org/b>".into()
        )),
        "a IS a direct parent of b → must NOT be unrelated; got: {:?}",
        pairs
    );
    // a IS an ancestor of c (requires recursive ancestor derivation) → must NOT be unrelated
    assert!(
        !pairs.contains(&(
            "<http://example.org/a>".into(),
            "<http://example.org/c>".into()
        )),
        "a IS an ancestor of c via b (recursive rule) → must NOT be unrelated; got: {:?}",
        pairs
    );
}

// ── FilterAtom: SPARQL expressions as Datalog guards ─────────────────────────
//
// These tests verify Phase E2 of EXPRESSION_PLAN.md: RuleAtom::FilterAtom holds
// a sparql_parser::ast::Expression and filters substitutions during rule evaluation.
// Un-ignore in order: E2 first (engine), E5 last (parser).

/// Rule with a numeric comparison guard: derive violation(x) only when age < 18.
/// Data: ex:alice ex:age 25; ex:bob ex:age 15.
/// Expected: only ex:bob in the violation set.
#[test]
fn filter_numeric_comparison() {
    use dag_rdf::Term as DagTerm;
    use datalog::types::Rule;
    use ingress::{IriReference, RdfResource, XSD_INTEGER};

    let ttl = r#"
@prefix ex: <http://example.org/> .
ex:alice ex:age 25 .
ex:bob   ex:age 15 .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let ex_age = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/age".to_string(),
        )));
    let ex_violation = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/violation".to_string(),
        )));
    let default_graph = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;

    // Use TypedLiteral for the constant 18 — compare_graph_elements converts
    // xsd:integer TypedLiterals to f64 for numeric comparison.
    let const_18 = Expression::Constant(GraphElement::GraphLiteral(RdfLiteral::TypedLiteral {
        type_iri: IriReference(XSD_INTEGER.to_string()),
        literal: "18".to_string(),
    }));

    // Rule: ex:violation[?x] :- [?x, ex:age, ?a], FILTER(?a < 18)
    let rule = Rule {
        head: RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: DagTerm::Resource(default_graph),
            subject: DagTerm::Variable("x".to_string()),
            predicate: DagTerm::Resource(ex_violation),
            object: DagTerm::Resource(ex_violation),
        }),
        body: vec![
            RuleAtom::PositivePattern(dag_rdf::QuadPattern {
                graph: DagTerm::Resource(default_graph),
                subject: DagTerm::Variable("x".to_string()),
                predicate: DagTerm::Resource(ex_age),
                object: DagTerm::Variable("a".to_string()),
            }),
            RuleAtom::FilterAtom(Expression::Binary(
                Box::new(Expression::Variable("a".to_string())),
                BinaryOp::Lt,
                Box::new(const_18),
            )),
        ],
    };

    datalog::evaluate_rules(vec![rule], &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x WHERE { ?x ex:violation ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let violators: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        violators.contains(&"<http://example.org/bob>".to_string()),
        "bob (age 15) should violate; got: {:?}",
        violators
    );
    assert!(
        !violators.contains(&"<http://example.org/alice>".to_string()),
        "alice (age 25) should NOT violate; got: {:?}",
        violators
    );
}

/// Rule with a string-length guard: derive violation(x) when strlen(label) < 3.
/// Data: ex:a ex:label "hi" (len 2); ex:b ex:label "hello" (len 5).
/// Expected: only ex:a violates (STRLEN(?v) < 3).
#[test]
fn filter_strlen_guard() {
    use dag_rdf::Term as DagTerm;
    use datalog::types::Rule;
    use ingress::{IriReference, RdfResource, XSD_INTEGER};

    let ttl = r#"
@prefix ex: <http://example.org/> .
ex:a ex:label "hi" .
ex:b ex:label "hello" .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let ex_label = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/label".to_string(),
        )));
    let ex_viol = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/violation".to_string(),
        )));
    let dg = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;

    let filter = Expression::Binary(
        Box::new(Expression::FunctionCall(
            "STRLEN".to_string(),
            vec![Expression::Variable("v".to_string())],
        )),
        BinaryOp::Lt,
        Box::new(Expression::Constant(GraphElement::GraphLiteral(
            RdfLiteral::TypedLiteral {
                type_iri: IriReference(XSD_INTEGER.to_string()),
                literal: "3".to_string(),
            },
        ))),
    );

    let rule = Rule {
        head: RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: DagTerm::Resource(dg),
            subject: DagTerm::Variable("x".to_string()),
            predicate: DagTerm::Resource(ex_viol),
            object: DagTerm::Resource(ex_viol),
        }),
        body: vec![
            RuleAtom::PositivePattern(dag_rdf::QuadPattern {
                graph: DagTerm::Resource(dg),
                subject: DagTerm::Variable("x".to_string()),
                predicate: DagTerm::Resource(ex_label),
                object: DagTerm::Variable("v".to_string()),
            }),
            RuleAtom::FilterAtom(filter),
        ],
    };

    datalog::evaluate_rules(vec![rule], &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x WHERE { ?x ex:violation ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let violators: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        violators.contains(&"<http://example.org/a>".to_string()),
        "ex:a (label 'hi', len 2) should violate STRLEN < 3; got: {:?}",
        violators
    );
    assert!(
        !violators.contains(&"<http://example.org/b>".to_string()),
        "ex:b (label 'hello', len 5) should NOT violate; got: {:?}",
        violators
    );
}

/// Rule with isIRI() type test guard: derive violation(x) when value is not an IRI.
/// Data: ex:a ex:p ex:iri_val (IRI); ex:b ex:p "literal_val" (literal).
/// Expected: only ex:b violates (!isIRI(?v)).
#[test]
fn filter_is_iri_guard() {
    use dag_rdf::Term as DagTerm;
    use datalog::types::Rule;
    use ingress::{IriReference, RdfResource};
    use sparql_parser::ast::UnaryOp;

    let ttl = r#"
@prefix ex: <http://example.org/> .
ex:a ex:p ex:iri_val .
ex:b ex:p "literal_val" .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let ex_p = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/p".to_string(),
        )));
    let ex_viol = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/violation".to_string(),
        )));
    let dg = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;

    let filter = Expression::Unary(
        UnaryOp::Not,
        Box::new(Expression::FunctionCall(
            "isIRI".to_string(),
            vec![Expression::Variable("v".to_string())],
        )),
    );

    let rule = Rule {
        head: RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: DagTerm::Resource(dg),
            subject: DagTerm::Variable("x".to_string()),
            predicate: DagTerm::Resource(ex_viol),
            object: DagTerm::Resource(ex_viol),
        }),
        body: vec![
            RuleAtom::PositivePattern(dag_rdf::QuadPattern {
                graph: DagTerm::Resource(dg),
                subject: DagTerm::Variable("x".to_string()),
                predicate: DagTerm::Resource(ex_p),
                object: DagTerm::Variable("v".to_string()),
            }),
            RuleAtom::FilterAtom(filter),
        ],
    };

    datalog::evaluate_rules(vec![rule], &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x WHERE { ?x ex:violation ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let violators: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        violators.contains(&"<http://example.org/b>".to_string()),
        "ex:b (literal value) should violate !isIRI; got: {:?}",
        violators
    );
    assert!(
        !violators.contains(&"<http://example.org/a>".to_string()),
        "ex:a (IRI value) should NOT violate; got: {:?}",
        violators
    );
}

/// Rule with DATATYPE guard: derive violation(x) when value has wrong datatype.
/// Data: ex:a ex:p 42 (xsd:integer); ex:b ex:p "abc"^^xsd:string.
/// FILTER(DATATYPE(?v) != xsd:integer) → ex:b violates.
#[test]
fn filter_datatype_guard() {
    use dag_rdf::Term as DagTerm;
    use datalog::types::Rule;
    use ingress::{IriReference, RdfResource, XSD_INTEGER};

    let ttl = r#"
@prefix ex: <http://example.org/> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
ex:a ex:p 42 .
ex:b ex:p "abc"^^xsd:string .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let ex_p = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/p".to_string(),
        )));
    let ex_viol = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/violation".to_string(),
        )));
    let dg = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;

    let filter = Expression::Binary(
        Box::new(Expression::FunctionCall(
            "DATATYPE".to_string(),
            vec![Expression::Variable("v".to_string())],
        )),
        BinaryOp::Ne,
        Box::new(Expression::Constant(GraphElement::NodeOrEdge(
            ingress::RdfResource::Iri(IriReference(XSD_INTEGER.to_string())),
        ))),
    );

    let rule = Rule {
        head: RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: DagTerm::Resource(dg),
            subject: DagTerm::Variable("x".to_string()),
            predicate: DagTerm::Resource(ex_viol),
            object: DagTerm::Resource(ex_viol),
        }),
        body: vec![
            RuleAtom::PositivePattern(dag_rdf::QuadPattern {
                graph: DagTerm::Resource(dg),
                subject: DagTerm::Variable("x".to_string()),
                predicate: DagTerm::Resource(ex_p),
                object: DagTerm::Variable("v".to_string()),
            }),
            RuleAtom::FilterAtom(filter),
        ],
    };

    datalog::evaluate_rules(vec![rule], &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x WHERE { ?x ex:violation ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let violators: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        violators.contains(&"<http://example.org/b>".to_string()),
        "ex:b (xsd:string) should violate DATATYPE != xsd:integer; got: {:?}",
        violators
    );
    assert!(
        !violators.contains(&"<http://example.org/a>".to_string()),
        "ex:a (integer 42) should NOT violate; got: {:?}",
        violators
    );
}

/// Rule with REGEX guard: derive violation(x) when label does not match pattern.
/// Data: ex:a ex:label "foo123"; ex:b ex:label "bar".
/// FILTER(!REGEX(?v, "foo")) → ex:b violates.
#[test]
fn filter_regex_guard() {
    use dag_rdf::Term as DagTerm;
    use datalog::types::Rule;
    use ingress::{IriReference, RdfResource};
    use sparql_parser::ast::UnaryOp;

    let ttl = r#"
@prefix ex: <http://example.org/> .
ex:a ex:label "foo123" .
ex:b ex:label "bar" .
"#;
    let mut ds = Datastore::new(10_000);
    turtle::parse_turtle(&mut ds, ttl.as_bytes()).unwrap();

    let ex_label = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/label".to_string(),
        )));
    let ex_viol = ds
        .resources
        .add_node_resource(RdfResource::Iri(IriReference(
            "http://example.org/violation".to_string(),
        )));
    let dg = dag_rdf::DEFAULT_GRAPH_ELEMENT_ID;

    let filter = Expression::Unary(
        UnaryOp::Not,
        Box::new(Expression::FunctionCall(
            "REGEX".to_string(),
            vec![
                Expression::Variable("v".to_string()),
                Expression::Constant(GraphElement::GraphLiteral(RdfLiteral::LiteralString(
                    "foo".to_string(),
                ))),
            ],
        )),
    );

    let rule = Rule {
        head: RuleHead::NormalHead(dag_rdf::QuadPattern {
            graph: DagTerm::Resource(dg),
            subject: DagTerm::Variable("x".to_string()),
            predicate: DagTerm::Resource(ex_viol),
            object: DagTerm::Resource(ex_viol),
        }),
        body: vec![
            RuleAtom::PositivePattern(dag_rdf::QuadPattern {
                graph: DagTerm::Resource(dg),
                subject: DagTerm::Variable("x".to_string()),
                predicate: DagTerm::Resource(ex_label),
                object: DagTerm::Variable("v".to_string()),
            }),
            RuleAtom::FilterAtom(filter),
        ],
    };

    datalog::evaluate_rules(vec![rule], &mut ds);

    let sparql = "PREFIX ex: <http://example.org/> SELECT ?x WHERE { ?x ex:violation ?y . }";
    let result = run_sparql_query(&ds, sparql).unwrap();
    let violators: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| r.get("x"))
        .map(graph_element_display)
        .collect();

    assert!(
        violators.contains(&"<http://example.org/b>".to_string()),
        "ex:b (label 'bar') should violate !REGEX(_, 'foo'); got: {:?}",
        violators
    );
    assert!(
        !violators.contains(&"<http://example.org/a>".to_string()),
        "ex:a (label 'foo123') should NOT violate; got: {:?}",
        violators
    );
}

/// Datalog parser accepts FILTER(expr) in rule body (Phase E5).
/// Input: `ex:violation[?x] :- [?x, ex:age, ?a], FILTER(?a < 18) .`
/// Expected: parses to 1 rule with body [PositivePattern, FilterAtom].
#[test]
#[ignore = "FILTER in Datalog parser not yet implemented — see EXPRESSION_PLAN.md Phase E5"]
fn parse_filter_in_datalog_rule() {
    todo!("Parse inline Datalog with FILTER guard; assert body[1] is FilterAtom")
}

/// A program where a depends negatively on b and b depends negatively on a
/// forms a negative cycle and cannot be stratified. The engine must panic.
#[test]
#[should_panic(expected = "not stratifiable")]
fn non_stratifiable_negative_cycle_panics() {
    let src = r#"
prefix ex: <http://example.org/>
ex:a[?x] :- ex:person[?x], NOT ex:b[?x] .
ex:b[?x] :- ex:person[?x], NOT ex:a[?x] .
"#;
    let mut ds = Datastore::new(10_000);
    let data = "@prefix ex: <http://example.org/> . ex:alice a ex:person .";
    turtle::parse_turtle(&mut ds, data.as_bytes()).unwrap();
    let rules = datalog_parser::parse(src, &mut ds).unwrap();
    datalog::evaluate_rules(rules, &mut ds);
}
