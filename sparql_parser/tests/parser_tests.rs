use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use sparql_parser::{ast::*, execute, parse_query, ParserContext, QueryResult};
use std::collections::HashMap;

#[test]
fn test_parse_simple_select() {
    let sparql = "SELECT ?title WHERE { <http://example.org/book/book1> <http://purl.org/dc/elements/1.1/title> ?title }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse");

    let Query::Select {
        projection,
        where_clause,
        ..
    } = query
    else {
        panic!("expected Select query");
    };

    assert_eq!(projection.len(), 1);
    assert_eq!(
        projection[0],
        ProjectionElement::Variable("title".to_string())
    );

    assert_eq!(where_clause.len(), 1);
    if let QueryComponent::BGP(triples) = &where_clause[0] {
        assert_eq!(triples.len(), 1);
        assert_eq!(triples[0].object, Term::Variable("title".to_string()));
    } else {
        panic!("Expected BGP");
    }
}

#[test]
fn test_parse_with_prefix() {
    let sparql = "
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name
        WHERE {
          ?person foaf:name ?name .
        }
    ";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse");

    let Query::Select {
        projection,
        where_clause,
        ..
    } = query
    else {
        panic!("expected Select query");
    };

    assert_eq!(projection.len(), 1);
    if let ProjectionElement::Variable(v) = &projection[0] {
        assert_eq!(v, "name");
    }

    if let QueryComponent::BGP(triples) = &where_clause[0] {
        assert_eq!(triples.len(), 1);
        if let Term::Constant(dag_rdf::GraphElement::NodeOrEdge(dag_rdf::RdfResource::Iri(iri))) =
            &triples[0].predicate
        {
            assert_eq!(iri.0, "http://xmlns.com/foaf/0.1/name");
        } else {
            panic!("Expected IRI predicate");
        }
    }
}

#[test]
fn test_parse_sparql12_graph_clause_with_iri() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name WHERE {
          GRAPH <http://example.org/graph/people> {
            ?person foaf:name ?name .
          }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 GRAPH clause");
    let Query::Select { where_clause, .. } = query else {
        panic!("expected Select query");
    };

    assert_eq!(where_clause.len(), 1);
    match &where_clause[0] {
        QueryComponent::Graph(_, inner) => {
            assert_eq!(inner.len(), 1);
            assert!(matches!(inner[0], QueryComponent::BGP(_)));
        }
        other => panic!("Expected Graph component, got: {:?}", other),
    }
}

#[test]
fn test_parse_sparql12_graph_clause_with_variable_graph_name() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?g ?name WHERE {
          GRAPH ?g {
            ?person foaf:name ?name .
          }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx)
        .expect("Should parse SPARQL 1.2 GRAPH clause with variable graph name");
    let Query::Select {
        projection,
        where_clause,
        ..
    } = query
    else {
        panic!("expected Select query");
    };

    assert_eq!(projection.len(), 2);
    assert_eq!(where_clause.len(), 1);
    match &where_clause[0] {
        QueryComponent::Graph(Term::Variable(v), inner) => {
            assert_eq!(v, "g");
            assert_eq!(inner.len(), 1);
            assert!(matches!(inner[0], QueryComponent::BGP(_)));
        }
        other => panic!(
            "Expected Graph component with variable graph name, got: {:?}",
            other
        ),
    }
}

#[test]
fn test_parse_semicolon_comma_and_property_path() {
    let sparql = r#"
        PREFIX ex: <https://example.com/>
        SELECT ?s ?o1 ?o2 WHERE {
          ?s ex:p ?o1, ?o2 ;
             ex:q/ex:r ?z .
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse shorthand and path");
    let Query::Select { where_clause, .. } = query else {
        panic!("expected Select query");
    };

    // The sequence path ex:q/ex:r is now emitted as a PathPattern component at runtime
    // rather than being expanded to bridge-variable triples at parse time.
    // Expected: BGP([?s ex:p ?o1, ?s ex:p ?o2]) + PathPattern(?s, Sequence(ex:q,ex:r), ?z)
    assert_eq!(where_clause.len(), 2);
    match &where_clause[0] {
        QueryComponent::BGP(triples) => {
            assert_eq!(
                triples.len(),
                2,
                "two simple predicate triples from comma list"
            );
        }
        other => panic!("Expected BGP at [0], got: {:?}", other),
    }
    match &where_clause[1] {
        QueryComponent::PathPattern(_, path, _) => {
            assert!(
                matches!(path.as_ref(), PropertyPath::Sequence(_)),
                "ex:q/ex:r should be a Sequence PathPattern, got: {:?}",
                path
            );
        }
        other => panic!("Expected PathPattern at [1], got: {:?}", other),
    }
}

#[test]
fn test_parse_sparql12_optional_with_bound_filter() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        SELECT ?name WHERE {
          ?x foaf:name ?name .
          OPTIONAL { ?x foaf:mbox ?mbox . }
          FILTER(!BOUND(?mbox))
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) =
        parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 OPTIONAL/FILTER example");
    let Query::Select { where_clause, .. } = query else {
        panic!("expected Select query");
    };

    assert_eq!(where_clause.len(), 3);
    assert!(matches!(where_clause[0], QueryComponent::BGP(_)));
    assert!(matches!(where_clause[1], QueryComponent::Optional(_)));
    assert!(matches!(where_clause[2], QueryComponent::Filter(_)));
}

#[test]
fn test_parse_sparql12_union_example() {
    let sparql = r#"
        PREFIX ex: <http://example.org/>
        SELECT ?x WHERE {
          { ?x ex:givenName "Alice" . }
          UNION
          { ?x ex:givenName "Bob" . }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };

    let (_, query) = parse_query(sparql, &mut ctx).expect("Should parse SPARQL 1.2 UNION example");
    let Query::Select { where_clause, .. } = query else {
        panic!("expected Select query");
    };

    assert_eq!(where_clause.len(), 1);
    assert!(matches!(where_clause[0], QueryComponent::Union(_, _)));
}

/// Wildcard CONSTRUCT: `CONSTRUCT {?s ?p ?o} WHERE { ?s ?p ?o }` is valid SPARQL
/// and should parse as a full-form Construct with 1 template triple and 1 WHERE component.
#[test]
fn construct_wildcard_spo_parses() {
    let sparql = "CONSTRUCT { ?s ?p ?o } WHERE { ?s ?p ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse wildcard CONSTRUCT");
    let Query::Construct {
        template,
        where_clause,
    } = query
    else {
        panic!("expected Construct query");
    };
    assert_eq!(template.len(), 1, "one template triple");
    assert_eq!(where_clause.len(), 1, "one WHERE component");
    let tp = &template[0];
    assert!(
        matches!(tp.subject, Term::Variable(_)),
        "subject is variable"
    );
    assert!(
        matches!(tp.predicate, Term::Variable(_)),
        "predicate is variable"
    );
    assert!(matches!(tp.object, Term::Variable(_)), "object is variable");
}

// ── CONSTRUCT parser tests ────────────────────────────────────────────────────

/// W3C SPARQL 1.1 §10.2.1: full form with IRI template.
#[test]
fn construct_full_form_parses() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        PREFIX vcard: <http://www.w3.org/2006/vcard/ns#>
        CONSTRUCT { <http://example.org/person#Alice> vcard:FN ?name }
        WHERE     { ?x foaf:name ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let Query::Construct {
        template,
        where_clause,
    } = query
    else {
        panic!("expected Construct query");
    };
    assert_eq!(template.len(), 1);
    assert_eq!(where_clause.len(), 1);
}

/// W3C SPARQL 1.1 §10.2.4: short form — CONSTRUCT WHERE { ... }.
#[test]
fn construct_short_form_parses() {
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        CONSTRUCT WHERE { ?s foaf:name ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let Query::Construct { template, .. } = query else {
        panic!("expected Construct");
    };
    assert!(
        template.is_empty(),
        "short form must produce empty template in AST"
    );
}

/// Full form with multiple template triples and blank nodes.
#[test]
fn construct_blank_node_template_parses() {
    let sparql = r#"
        PREFIX foaf:  <http://xmlns.com/foaf/0.1/>
        PREFIX vcard: <http://www.w3.org/2006/vcard/ns#>
        CONSTRUCT {
            ?x  vcard:N _:v .
            _:v vcard:givenName ?gname .
            _:v vcard:familyName ?fname
        }
        WHERE { ?x foaf:firstName ?gname ; foaf:lastName ?fname }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("should parse");
    let Query::Construct { template, .. } = query else {
        panic!("expected Construct");
    };
    assert_eq!(template.len(), 3);
}

// ── CONSTRUCT executor tests ──────────────────────────────────────────────────

fn make_iri(iri: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(iri.to_string())))
}

fn make_literal(s: &str) -> GraphElement {
    GraphElement::GraphLiteral(RdfLiteral::LiteralString(s.to_string()))
}

fn add_iri(ds: &mut Datastore, iri: &str) -> dag_rdf::GraphElementId {
    ds.add_node_resource(RdfResource::Iri(IriReference(iri.to_string())))
}

fn add_literal(ds: &mut Datastore, s: &str) -> dag_rdf::GraphElementId {
    ds.add_literal_resource(RdfLiteral::LiteralString(s.to_string()))
}

/// W3C SPARQL 1.1 §10.2.1: simple IRI template maps data to vcard triples.
#[test]
fn construct_iri_template_produces_correct_triples() {
    let mut ds = Datastore::new(64);
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let foaf_name = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let name_val = add_literal(&mut ds, "Alice");
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: foaf_name,
        obj: name_val,
    });

    let sparql = r#"
        PREFIX foaf:  <http://xmlns.com/foaf/0.1/>
        PREFIX vcard: <http://www.w3.org/2006/vcard/ns#>
        CONSTRUCT { <http://example.org/person#Alice> vcard:FN ?name }
        WHERE     { <http://example.org/alice> foaf:name ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(triples.len(), 1);
    assert_eq!(
        triples[0].subject,
        make_iri("http://example.org/person#Alice")
    );
    assert_eq!(
        triples[0].predicate,
        make_iri("http://www.w3.org/2006/vcard/ns#FN")
    );
    assert_eq!(triples[0].object, make_literal("Alice"));
}

/// W3C SPARQL 1.1 §10.2.1: blank-node template generates fresh nodes per solution.
///
/// Two solutions (Alice and Bob) produce distinct blank nodes for the same template label.
#[test]
fn construct_blank_node_template_fresh_per_solution() {
    let mut ds = Datastore::new(64);
    let foaf_first = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/firstName");
    let foaf_last = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/lastName");
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let alice_first = add_literal(&mut ds, "Alice");
    let alice_last = add_literal(&mut ds, "Smith");
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: foaf_first,
        obj: alice_first,
    });
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: foaf_last,
        obj: alice_last,
    });
    let bob = add_iri(&mut ds, "http://example.org/bob");
    let bob_first = add_literal(&mut ds, "Bob");
    let bob_last = add_literal(&mut ds, "Jones");
    ds.add_triple(dag_rdf::Triple {
        subject: bob,
        predicate: foaf_first,
        obj: bob_first,
    });
    ds.add_triple(dag_rdf::Triple {
        subject: bob,
        predicate: foaf_last,
        obj: bob_last,
    });

    let sparql = r#"
        PREFIX foaf:  <http://xmlns.com/foaf/0.1/>
        PREFIX vcard: <http://www.w3.org/2006/vcard/ns#>
        CONSTRUCT {
            ?x  vcard:N _:v .
            _:v vcard:givenName ?gname .
            _:v vcard:familyName ?fname
        }
        WHERE { ?x foaf:firstName ?gname ; foaf:lastName ?fname }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(triples.len(), 6, "2 solutions × 3 template triples = 6");

    // Collect the blank-node IDs used as objects of vcard:N triples
    let vcard_n = make_iri("http://www.w3.org/2006/vcard/ns#N");
    let bnode_ids: Vec<u32> = triples
        .iter()
        .filter_map(|t| {
            if t.predicate == vcard_n {
                if let GraphElement::NodeOrEdge(RdfResource::AnonymousBlankNode(id)) = &t.object {
                    return Some(*id);
                }
            }
            None
        })
        .collect();
    assert_eq!(bnode_ids.len(), 2, "one vcard:N triple per person");
    assert_ne!(
        bnode_ids[0], bnode_ids[1],
        "blank nodes must be distinct across solutions"
    );
}

/// W3C SPARQL 1.1 §10.2.4: short form returns the WHERE pattern triples.
#[test]
fn construct_short_form_returns_all_triples() {
    let mut ds = Datastore::new(64);
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let foaf_name = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let name_val = add_literal(&mut ds, "Alice");
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: foaf_name,
        obj: name_val,
    });

    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        CONSTRUCT WHERE { ?s foaf:name ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(triples.len(), 1);
    assert_eq!(triples[0].subject, make_iri("http://example.org/alice"));
    assert_eq!(
        triples[0].predicate,
        make_iri("http://xmlns.com/foaf/0.1/name")
    );
    assert_eq!(triples[0].object, make_literal("Alice"));
}

/// No matching solutions → empty output (no error).
#[test]
fn construct_no_solutions_produces_empty_result() {
    let ds = Datastore::new(64);
    let sparql = "CONSTRUCT { <http://example.org/s> <http://example.org/p> <http://example.org/o> } WHERE { ?s <http://example.org/missing> ?o }";
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };
    assert!(triples.is_empty());
}

/// Unbound variable in template → triple is silently skipped (spec §10.2.1).
#[test]
fn construct_unbound_template_variable_is_skipped() {
    let mut ds = Datastore::new(64);
    let s = add_iri(&mut ds, "http://example.org/s");
    let p = add_iri(&mut ds, "http://example.org/p");
    let o = add_literal(&mut ds, "val");
    ds.add_triple(dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o,
    });

    let sparql = r#"
        CONSTRUCT { ?s <http://example.org/q> ?unbound }
        WHERE     { ?s <http://example.org/p> ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };
    assert!(
        triples.is_empty(),
        "unbound variable must cause triple to be skipped"
    );
}

/// CONSTRUCT { ?s ?p ?o } WHERE { GRAPH <iri> { ?s ?p ?o } } — the IRecordBackend pattern.
#[test]
fn construct_graph_clause_returns_named_graph_triples() {
    let mut ds = Datastore::new(64);
    let g = add_iri(&mut ds, "http://example.org/graph1");
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let foaf_name = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let name_val = add_literal(&mut ds, "Alice");
    ds.add_named_graph_triple(
        g,
        dag_rdf::Triple {
            subject: alice,
            predicate: foaf_name,
            obj: name_val,
        },
    );

    let sparql = r#"
        CONSTRUCT { ?s ?p ?o }
        WHERE { GRAPH <http://example.org/graph1> { ?s ?p ?o } }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(triples.len(), 1);
    assert_eq!(triples[0].subject, make_iri("http://example.org/alice"));
    assert_eq!(
        triples[0].predicate,
        make_iri("http://xmlns.com/foaf/0.1/name")
    );
    assert_eq!(triples[0].object, make_literal("Alice"));
}

/// Literal in subject position of an instantiated template triple → silently skipped (§10.2.1).
#[test]
fn construct_literal_in_subject_is_skipped() {
    let mut ds = Datastore::new(64);
    let s = add_iri(&mut ds, "http://example.org/s");
    let p = add_iri(&mut ds, "http://example.org/p");
    let o = add_literal(&mut ds, "val");
    ds.add_triple(dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o,
    });

    // ?name resolves to a literal; using it as subject must be silently dropped
    let sparql = r#"
        CONSTRUCT { ?name <http://example.org/q> <http://example.org/r> }
        WHERE     { ?s <http://example.org/p> ?name }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };
    assert!(
        triples.is_empty(),
        "literal subject must be silently skipped"
    );
}

// ── CONSTRUCT WHERE recursion bug (ignored — see docs/plans/construct-where-recursion.md) ──

/// Regression: CONSTRUCT WHERE { OPTIONAL { … } } should use the full WHERE
/// pattern as the template. Currently `collect_bgps_from_components` only
/// collects top-level BGP nodes and misses OPTIONAL triples, producing an
/// empty result instead of the correct triples.
#[test]
fn construct_where_with_optional_includes_optional_triples() {
    let mut ds = Datastore::new(64);
    let s = add_iri(&mut ds, "http://example.org/alice");
    let p = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let o = add_literal(&mut ds, "Alice");
    ds.add_triple(dag_rdf::Triple {
        subject: s,
        predicate: p,
        obj: o,
    });

    // The short form should produce the same triples as the matching WHERE pattern.
    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        CONSTRUCT WHERE {
            OPTIONAL { ?s foaf:name ?name }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(
        triples.len(),
        1,
        "CONSTRUCT WHERE should include triples from OPTIONAL block; got {triples:?}"
    );
    assert_eq!(triples[0].subject, make_iri("http://example.org/alice"));
    assert_eq!(
        triples[0].predicate,
        make_iri("http://xmlns.com/foaf/0.1/name")
    );
    assert_eq!(triples[0].object, make_literal("Alice"));
}

/// Regression: CONSTRUCT WHERE { … UNION … } should use both branches as
/// the template. Currently UNION branches are not collected.
#[test]
fn construct_where_with_union_includes_all_branches() {
    let mut ds = Datastore::new(64);
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let name_p = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let age_p = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/age");
    let name_val = add_literal(&mut ds, "Alice");
    let age_val = add_literal(&mut ds, "30");
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: name_p,
        obj: name_val,
    });
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: age_p,
        obj: age_val,
    });

    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        CONSTRUCT WHERE {
            { ?s foaf:name ?name }
            UNION
            { ?s foaf:age ?age }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(
        triples.len(),
        2,
        "CONSTRUCT WHERE should include triples from both UNION branches; got {triples:?}"
    );
}

/// Regression: CONSTRUCT WHERE { GRAPH <iri> { … } } should use the inner triples
/// as the template. `collect_bgps_from_components` must recurse into Graph nodes.
#[test]
fn construct_where_with_graph_includes_named_graph_triples() {
    let mut ds = Datastore::new(64);
    let g = add_iri(&mut ds, "http://example.org/graph1");
    let alice = add_iri(&mut ds, "http://example.org/alice");
    let foaf_name = add_iri(&mut ds, "http://xmlns.com/foaf/0.1/name");
    let name_val = add_literal(&mut ds, "Alice");
    ds.add_named_graph_triple(
        g,
        dag_rdf::Triple {
            subject: alice,
            predicate: foaf_name,
            obj: name_val,
        },
    );

    let sparql = r#"
        PREFIX foaf: <http://xmlns.com/foaf/0.1/>
        CONSTRUCT WHERE {
            GRAPH <http://example.org/graph1> { ?s foaf:name ?name }
        }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };

    assert_eq!(
        triples.len(),
        1,
        "CONSTRUCT WHERE should include triples from GRAPH block; got {triples:?}"
    );
    assert_eq!(triples[0].subject, make_iri("http://example.org/alice"));
    assert_eq!(
        triples[0].predicate,
        make_iri("http://xmlns.com/foaf/0.1/name")
    );
    assert_eq!(triples[0].object, make_literal("Alice"));
}

/// Output triples are deduplicated when multiple solutions produce the same constant triple.
#[test]
fn construct_deduplicates_output_triples() {
    let mut ds = Datastore::new(64);
    let s1 = add_iri(&mut ds, "http://example.org/s1");
    let s2 = add_iri(&mut ds, "http://example.org/s2");
    let p = add_iri(&mut ds, "http://example.org/p");
    let o = add_iri(&mut ds, "http://example.org/o");
    ds.add_triple(dag_rdf::Triple {
        subject: s1,
        predicate: p,
        obj: o,
    });
    ds.add_triple(dag_rdf::Triple {
        subject: s2,
        predicate: p,
        obj: o,
    });

    let sparql = r#"
        CONSTRUCT { <http://example.org/const> <http://example.org/p2> <http://example.org/o2> }
        WHERE { ?s <http://example.org/p> <http://example.org/o> }
    "#;
    let mut ctx = ParserContext {
        prefixes: HashMap::new(),
    };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Construct(triples) = execute(&query, &ds).expect("execute") else {
        panic!("expected Construct result");
    };
    assert_eq!(
        triples.len(),
        1,
        "duplicate output triples must be collapsed to one"
    );
}

// ── [] blank-node shorthand in subject position ───────────────────────────────

fn add_iri_str(ds: &mut Datastore, iri: &str) -> dag_rdf::GraphElementId {
    ds.add_node_resource(RdfResource::Iri(IriReference(iri.to_string())))
}

/// `[] a ?c` parses and executes: finds all classes that have at least one instance.
#[test]
fn test_empty_blank_node_subject_finds_classes() {
    let mut ds = Datastore::new(100);
    let person_class = add_iri_str(&mut ds, "http://example.org/Person");
    let alice = add_iri_str(&mut ds, "http://example.org/alice");
    let rdf_type = add_iri_str(&mut ds, "http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
    ds.add_triple(dag_rdf::Triple {
        subject: alice,
        predicate: rdf_type,
        obj: person_class,
    });

    let sparql = "SELECT DISTINCT ?c WHERE { [] a ?c } LIMIT 10";
    let mut ctx = ParserContext { prefixes: HashMap::new() };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Select(result) = execute(&query, &ds).expect("execute") else {
        panic!("expected SELECT result");
    };
    assert_eq!(result.rows.len(), 1);
    assert_eq!(
        result.rows[0]["c"],
        GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(
            "http://example.org/Person".to_string()
        )))
    );
}

/// The class-picker query from the query builder — UNION of owl:Class declarations
/// and `[] a ?c` instance-based discovery.
#[test]
fn test_class_picker_union_query() {
    let mut ds = Datastore::new(200);

    // One class declared as owl:Class
    let owl_class = add_iri_str(&mut ds, "http://www.w3.org/2002/07/owl#Class");
    let rdf_type  = add_iri_str(&mut ds, "http://www.w3.org/1999/02/22-rdf-syntax-ns#type");
    let my_class  = add_iri_str(&mut ds, "http://example.org/MyClass");
    ds.add_triple(dag_rdf::Triple { subject: my_class, predicate: rdf_type, obj: owl_class });

    // One class found via instance
    let other_class = add_iri_str(&mut ds, "http://example.org/OtherClass");
    let instance    = add_iri_str(&mut ds, "http://example.org/inst1");
    ds.add_triple(dag_rdf::Triple { subject: instance, predicate: rdf_type, obj: other_class });

    let sparql =
        "SELECT DISTINCT ?c WHERE { \
           { ?c a <http://www.w3.org/2002/07/owl#Class> } \
           UNION \
           { [] a ?c } \
         } LIMIT 300";
    let mut ctx = ParserContext { prefixes: HashMap::new() };
    let (_, query) = parse_query(sparql, &mut ctx).expect("parse");
    let QueryResult::Select(result) = execute(&query, &ds).expect("execute") else {
        panic!("expected SELECT result");
    };

    let found: Vec<_> = result.rows.iter()
        .filter_map(|r| {
            if let GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s))) = &r["c"] {
                Some(s.as_str())
            } else { None }
        })
        .collect();

    assert!(found.contains(&"http://example.org/MyClass"), "MyClass via owl:Class");
    assert!(found.contains(&"http://example.org/OtherClass"), "OtherClass via instance");
}
