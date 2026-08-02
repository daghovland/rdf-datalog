/// Unit tests for the SQL `LogicalSource` (`SqlSource`, `rusqlite`/SQLite).
/// See `docs/plans/RML_SQL_PLAN.md` and
/// [issue #26](https://github.com/daghovland/rdf-datalog/issues/26).
use rml::ast::{SqlConnection, SqlQuery};
use rml::sources::sql::SqlSource;

/// Build a temp-file SQLite database (not in-memory, since `SqlSource`
/// connects by path — matching how a real mapping's `rml:source` would
/// point at a `.sqlite` file) with one `student` table, seeded with the
/// same shape of data as `RML_JOIN_PLAN.md`'s CSV join fixtures.
fn seeded_db(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE student (id INTEGER, name TEXT, sport TEXT);
         INSERT INTO student (id, name, sport) VALUES (10, 'Venus Williams', 'Tennis');
         INSERT INTO student (id, name, sport) VALUES (20, 'Demi Moore', NULL);",
    )
    .unwrap();
    path
}

fn temp_dir(case: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("rml_sql_tests")
        .join(format!("{case}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn sql_source_table_scan_yields_expected_rows() {
    let dir = temp_dir("table_scan");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Table("student".to_string()),
    );
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "10");
    assert_eq!(rows[0]["name"], "Venus Williams");
    assert_eq!(rows[0]["sport"], "Tennis");
}

#[test]
fn sql_source_null_column_is_omitted_not_empty_string() {
    let dir = temp_dir("null_column");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Table("student".to_string()),
    );
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();

    let demi = rows.iter().find(|r| r["name"] == "Demi Moore").unwrap();
    // NULL sport must be entirely absent, matching CsvRow's treatment of
    // missing/empty references as "no value" (SourceRow::get_str -> None).
    assert!(!demi.contains_key("sport"));
}

#[test]
fn sql_source_sql_query_arbitrary_select_yields_expected_rows() {
    let dir = temp_dir("sql_query");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Query("SELECT id, name FROM student WHERE sport = 'Tennis'".to_string()),
    );
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "10");
    assert_eq!(rows[0]["name"], "Venus Williams");
    // The projection excludes `sport` entirely from the SELECT list, so it
    // must not appear in the row at all (distinct from being NULL).
    assert!(!rows[0].contains_key("sport"));
}

#[test]
fn sql_source_missing_table_yields_error_not_panic() {
    let dir = temp_dir("missing_table");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Table("no_such_table".to_string()),
    );
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

#[test]
fn sql_source_malformed_sql_query_yields_error_not_panic() {
    let dir = temp_dir("malformed_sql");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Query("SELEKT * FROM student".to_string()),
    );
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

#[test]
fn sql_source_missing_database_file_yields_error_not_panic() {
    let dir = temp_dir("missing_db");
    let db_path = dir.join("does_not_exist.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Table("student".to_string()),
    );
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

#[test]
fn sql_source_table_name_with_double_quote_is_safely_quoted() {
    // A table name containing a double quote must not allow SQL injection
    // via string interpolation into `SELECT * FROM <name>`.
    let dir = temp_dir("quoted_table");
    let path = dir.join("weird.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(r#"CREATE TABLE "weird""name" (id INTEGER);"#)
        .unwrap();
    conn.execute(r#"INSERT INTO "weird""name" (id) VALUES (1)"#, [])
        .unwrap();
    drop(conn);

    let source = SqlSource::new(
        SqlConnection::Sqlite(path.clone()),
        SqlQuery::Table("weird\"name".to_string()),
    );
    let rows: Vec<_> = source.rows().collect::<Result<_, _>>().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "1");
}

#[test]
fn sql_source_row_limit_is_enforced() {
    let dir = temp_dir("row_limit");
    let db_path = seeded_db(&dir, "students.sqlite");

    let source = SqlSource::new(
        SqlConnection::Sqlite(db_path.clone()),
        SqlQuery::Table("student".to_string()),
    )
    .with_row_limit(1);
    let result: Result<Vec<_>, _> = source.rows().collect();
    assert!(result.is_err());
}

// ── Loader: rml:tableName / rml:sqlQuery discrimination ─────────────────────

mod loader_tests {
    use rml::ast::{LogicalSourceRef, SqlConnection, SqlQuery};
    use rml::loader::load_mapping_from_str;

    const TABLE_NAME_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.sqlite" ;
        rml:tableName "student"
    ] ;
    rml:subjectMap [ rml:template "http://example.com/Student/{id}" ] .
"#;

    const SQL_QUERY_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.sqlite" ;
        rml:sqlQuery "SELECT * FROM student WHERE sport = 'Tennis'"
    ] ;
    rml:subjectMap [ rml:template "http://example.com/Student/{id}" ] .
"#;

    const FILE_MAPPING: &str = r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "students.csv" ;
        rml:referenceFormulation rml:CSV
    ] ;
    rml:subjectMap [ rml:template "http://example.com/Student/{id}" ] .
"#;

    #[test]
    fn table_name_produces_sql_table_source() {
        let mapping = load_mapping_from_str(TABLE_NAME_MAPPING).unwrap();
        let source = &mapping.triples_maps[0].logical_source.source;
        match source {
            LogicalSourceRef::Sql(sql_ref) => {
                assert_eq!(
                    sql_ref.connection,
                    SqlConnection::Sqlite("students.sqlite".into())
                );
                assert_eq!(sql_ref.query, SqlQuery::Table("student".to_string()));
            }
            other => panic!("expected LogicalSourceRef::Sql, got {other:?}"),
        }
    }

    #[test]
    fn sql_query_produces_sql_query_source() {
        let mapping = load_mapping_from_str(SQL_QUERY_MAPPING).unwrap();
        let source = &mapping.triples_maps[0].logical_source.source;
        match source {
            LogicalSourceRef::Sql(sql_ref) => {
                assert_eq!(
                    sql_ref.query,
                    SqlQuery::Query("SELECT * FROM student WHERE sport = 'Tennis'".to_string())
                );
            }
            other => panic!("expected LogicalSourceRef::Sql, got {other:?}"),
        }
    }

    #[test]
    fn neither_table_name_nor_sql_query_falls_back_to_file() {
        let mapping = load_mapping_from_str(FILE_MAPPING).unwrap();
        let source = &mapping.triples_maps[0].logical_source.source;
        match source {
            LogicalSourceRef::File(path) => {
                assert_eq!(path, std::path::Path::new("students.csv"));
            }
            other => panic!("expected LogicalSourceRef::File, got {other:?}"),
        }
    }
}
