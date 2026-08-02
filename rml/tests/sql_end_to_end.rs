/// End-to-end `apply_rml_mapping` tests for the SQL `LogicalSource`. Mirrors
/// `join_end_to_end.rs`'s `rmltc0009a_join` CSV fixture (student/sport data)
/// re-expressed as SQLite tables, so results are directly comparable.
/// See `docs/plans/RML_SQL_PLAN.md` and
/// [issue #26](https://github.com/daghovland/rdf-datalog/issues/26).
use dag_rdf::ingress::Triple;
use dag_rdf::{Datastore, GraphElement, IriReference, RdfLiteral, RdfResource};
use rml::apply_rml_mapping;

fn iri(s: &str) -> GraphElement {
    GraphElement::NodeOrEdge(RdfResource::Iri(IriReference(s.to_string())))
}

fn temp_dir(case: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("rml_sql_end_to_end")
        .join(format!("{case}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SINGLE_TABLE_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix ex: <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.sqlite" ;
        rml:tableName "student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/student/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate foaf:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

#[test]
fn sql_table_source_produces_expected_triples() {
    let dir = temp_dir("single_table");
    std::fs::write(dir.join("mapping.ttl"), SINGLE_TABLE_MAPPING).unwrap();
    let conn = rusqlite::Connection::open(dir.join("students.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE student (id INTEGER, name TEXT);
         INSERT INTO student (id, name) VALUES (10, 'Venus Williams');",
    )
    .unwrap();
    drop(conn);

    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri("http://example.com/student/10"));
    let p = ds.add_resource(iri("http://xmlns.com/foaf/0.1/name"));
    let o = ds.add_literal_resource(RdfLiteral::LiteralString("Venus Williams".to_string()));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));
}

const SQL_QUERY_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix ex: <http://example.com/> .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.sqlite" ;
        rml:sqlQuery "SELECT id, name FROM student WHERE sport = 'Tennis'"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/student/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate foaf:name ;
        rml:objectMap [ rml:reference "name" ]
    ] .
"#;

#[test]
fn sql_query_source_filters_rows_via_the_select() {
    let dir = temp_dir("sql_query");
    std::fs::write(dir.join("mapping.ttl"), SQL_QUERY_MAPPING).unwrap();
    let conn = rusqlite::Connection::open(dir.join("students.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE student (id INTEGER, name TEXT, sport TEXT);
         INSERT INTO student (id, name, sport) VALUES (10, 'Venus Williams', 'Tennis');
         INSERT INTO student (id, name, sport) VALUES (20, 'Demi Moore', 'Football');",
    )
    .unwrap();
    drop(conn);

    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let venus = ds.add_resource(iri("http://example.com/student/10"));
    let demi = ds.add_resource(iri("http://example.com/student/20"));
    let name = ds.add_resource(iri("http://xmlns.com/foaf/0.1/name"));
    assert_eq!(
        ds.get_triples_with_subject(venus)
            .filter(|t| t.predicate == name)
            .count(),
        1
    );
    // Demi Moore was filtered out by the WHERE clause, so no TriplesMap ran
    // for that row at all.
    assert_eq!(
        ds.get_triples_with_subject(demi)
            .filter(|t| t.predicate == name)
            .count(),
        0
    );
}

// ── Join composition: SQL source through the existing hash-join engine ─────
//
// `RML_JOIN_PLAN.md`'s hash join is source-agnostic — it calls `scan_rows`
// on both the child and parent `LogicalScan`, whatever their source type.
// This proves a SQL child joined against a SQL parent (same SQLite file,
// two tables) produces the same triples the CSV rmltc0009a_join fixture
// does, without any join-pushdown code (deferred — see RML_SQL_PLAN.md's
// "Implementation status" note).
const SQL_JOIN_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix ex: <http://example.com/> .

<http://example.com/SportMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "school.sqlite" ;
        rml:tableName "sport"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/sport/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate rdfs:label ;
        rml:objectMap [ rml:reference "name" ]
    ] .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "school.sqlite" ;
        rml:tableName "student"
    ] ;
    rml:subjectMap [
        rml:template "http://example.com/student/{id}"
    ] ;
    rml:predicateObjectMap [
        rml:predicate foaf:name ;
        rml:objectMap [ rml:reference "name" ]
    ] ;
    rml:predicateObjectMap [
        rml:predicate ex:practises ;
        rml:objectMap [
            rml:parentTriplesMap <http://example.com/SportMap> ;
            rml:joinCondition [
                rml:child "sport_id" ;
                rml:parent "id"
            ]
        ]
    ] .
"#;

#[test]
fn sql_child_and_sql_parent_join_via_existing_hash_join_engine() {
    let dir = temp_dir("sql_join");
    std::fs::write(dir.join("mapping.ttl"), SQL_JOIN_MAPPING).unwrap();
    let conn = rusqlite::Connection::open(dir.join("school.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE sport (id INTEGER, name TEXT);
         INSERT INTO sport (id, name) VALUES (100, 'Tennis');
         CREATE TABLE student (id INTEGER, name TEXT, sport_id INTEGER);
         INSERT INTO student (id, name, sport_id) VALUES (10, 'Venus Williams', 100);
         INSERT INTO student (id, name, sport_id) VALUES (20, 'Demi Moore', NULL);",
    )
    .unwrap();
    drop(conn);

    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();

    let s = ds.add_resource(iri("http://example.com/student/10"));
    let p = ds.add_resource(iri("http://example.com/practises"));
    let o = ds.add_resource(iri("http://example.com/sport/100"));
    assert!(ds.contains_triple(&Triple {
        subject: s,
        predicate: p,
        obj: o
    }));

    // Demi Moore's NULL sport_id must not match any parent row.
    let demi = ds.add_resource(iri("http://example.com/student/20"));
    assert_eq!(
        ds.get_triples_with_subject(demi)
            .filter(|t| t.predicate == p)
            .count(),
        0
    );
}
