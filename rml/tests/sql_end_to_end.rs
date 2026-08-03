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

// ── Join composition: same-connection SQL/SQL join → SQL pushdown ──────────
//
// Child and parent are both `rml:tableName` sources on the *same* SQLite
// file, so `choose_join_algorithm` (see `translate.rs`, #354) selects
// `JoinAlgorithm::SqlPushdown`: the join runs as one synthesized SQL query
// (`sources::sql::build_pushdown_query`) rather than through
// `RML_JOIN_PLAN.md`'s Rust-side hash join. This test proves the pushdown
// path produces the same triples the CSV rmltc0009a_join fixture does — the
// genuine hash-join *fallback* case (SQL child, non-SQL parent) is exercised
// separately below by `sql_child_and_csv_parent_join_falls_back_to_hash_join`.
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
fn sql_child_and_sql_parent_same_connection_join_uses_sql_pushdown() {
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

// ── Join fallback: SQL child + non-SQL parent → hash join, per #354 ────────
//
// The child is a SQL source, but the parent is a plain CSV file — not SQL on
// the same connection — so `choose_join_algorithm` must keep
// `JoinAlgorithm::HashJoin`, and the join runs through
// `RML_JOIN_PLAN.md`'s existing engine (`execute_join`'s non-pushdown path).
// This is the "cross-source join that correctly stays on hash-join" case
// #354 asks to be proven explicitly, distinct from the pushdown test above.
const SQL_CHILD_CSV_PARENT_JOIN_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix foaf: <http://xmlns.com/foaf/0.1/> .
@prefix ex: <http://example.com/> .

<http://example.com/SportMap>
    a rml:TriplesMap ;
    rml:logicalSource [ rml:source "sport.csv" ; rml:referenceFormulation rml:CSV ] ;
    rml:subjectMap [ rml:template "http://example.com/sport/{id}" ] ;
    rml:predicateObjectMap [
        rml:predicate rdfs:label ;
        rml:objectMap [ rml:reference "name" ]
    ] .

<http://example.com/StudentMap>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.sqlite" ;
        rml:tableName "student"
    ] ;
    rml:subjectMap [ rml:template "http://example.com/student/{id}" ] ;
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
fn sql_child_and_csv_parent_join_falls_back_to_hash_join() {
    let dir = temp_dir("sql_csv_fallback_join");
    std::fs::write(dir.join("mapping.ttl"), SQL_CHILD_CSV_PARENT_JOIN_MAPPING).unwrap();
    std::fs::write(dir.join("sport.csv"), "id,name\n100,Tennis\n").unwrap();
    let conn = rusqlite::Connection::open(dir.join("students.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE student (id INTEGER, name TEXT, sport_id INTEGER);
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

    let demi = ds.add_resource(iri("http://example.com/student/20"));
    assert_eq!(
        ds.get_triples_with_subject(demi)
            .filter(|t| t.predicate == p)
            .count(),
        0
    );
}

// ── Empty-string join keys: pushdown and hash-join must agree ──────────────
//
// `SourceRow::get_str` (`CsvRow`, and the `RawRow` SQL rows share the same
// shape) treats an empty string as "no value", so the hash join never even
// builds a key for an empty-string join column — same as a `NULL`. A naive
// SQL `ON c.x = p.y` would happily match `'' = ''`, though, which would make
// pushdown and hash-join *disagree* on this case. `build_pushdown_query`
// guards against this with an explicit `<> ''` per condition (see
// `sources/sql.rs`) — this test proves both tiers land on the same answer:
// zero join triples for an empty-string join key, in both the pushdown
// (same-connection SQL/SQL) and hash-join (SQL/CSV) configurations.
#[test]
fn sql_pushdown_and_hash_join_agree_on_empty_string_join_key() {
    // Pushdown: both sides SQL, same connection.
    let dir = temp_dir("empty_key_pushdown");
    std::fs::write(dir.join("mapping.ttl"), SQL_JOIN_MAPPING).unwrap();
    let conn = rusqlite::Connection::open(dir.join("school.sqlite")).unwrap();
    conn.execute_batch(
        "CREATE TABLE sport (id TEXT, name TEXT);
         INSERT INTO sport (id, name) VALUES ('100', 'Tennis');
         INSERT INTO sport (id, name) VALUES ('', 'Empty');
         CREATE TABLE student (id INTEGER, name TEXT, sport_id TEXT);
         INSERT INTO student (id, name, sport_id) VALUES (10, 'Venus Williams', '100');
         INSERT INTO student (id, name, sport_id) VALUES (20, 'Demi Moore', '');",
    )
    .unwrap();
    drop(conn);

    let mut ds = Datastore::new(100);
    apply_rml_mapping(&dir.join("mapping.ttl"), &dir, &mut ds).unwrap();
    let p = ds.add_resource(iri("http://example.com/practises"));
    let demi = ds.add_resource(iri("http://example.com/student/20"));
    assert_eq!(
        ds.get_triples_with_subject(demi)
            .filter(|t| t.predicate == p)
            .count(),
        0,
        "pushdown: empty-string join key must not match empty-string parent key"
    );

    // Hash-join fallback: SQL child, CSV parent, same empty-string shape.
    let dir2 = temp_dir("empty_key_hash_join");
    std::fs::write(dir2.join("mapping.ttl"), SQL_CHILD_CSV_PARENT_JOIN_MAPPING).unwrap();
    std::fs::write(dir2.join("sport.csv"), "id,name\n100,Tennis\n,Empty\n").unwrap();
    let conn2 = rusqlite::Connection::open(dir2.join("students.sqlite")).unwrap();
    conn2
        .execute_batch(
            "CREATE TABLE student (id INTEGER, name TEXT, sport_id TEXT);
         INSERT INTO student (id, name, sport_id) VALUES (10, 'Venus Williams', '100');
         INSERT INTO student (id, name, sport_id) VALUES (20, 'Demi Moore', '');",
        )
        .unwrap();
    drop(conn2);

    let mut ds2 = Datastore::new(100);
    apply_rml_mapping(&dir2.join("mapping.ttl"), &dir2, &mut ds2).unwrap();
    let p2 = ds2.add_resource(iri("http://example.com/practises"));
    let demi2 = ds2.add_resource(iri("http://example.com/student/20"));
    assert_eq!(
        ds2.get_triples_with_subject(demi2)
            .filter(|t| t.predicate == p2)
            .count(),
        0,
        "hash join: empty-string join key must not match empty-string parent key"
    );
}
