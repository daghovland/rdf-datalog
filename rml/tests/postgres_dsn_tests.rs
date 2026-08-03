/// Unit tests for PostgreSQL DSN resolution (`loader::resolve_sql_connection`,
/// private — exercised here through `load_mapping_from_str`) and its
/// credential-safety requirement: `rml:source` must be a `"${VAR}"`
/// environment-variable reference, never a literal connection string. See
/// `docs/plans/RML_SQL_PLAN.md`'s "Credentials" section and
/// [#354](https://github.com/daghovland/rdf-datalog/issues/354).
///
/// No live PostgreSQL server is required or used here — these tests only
/// exercise the loader's string-classification/env-var-resolution logic,
/// which needs no connection at all. Tests that would need an actual
/// PostgreSQL connection (`SqlConn::open_for_join`/`SqlSource::rows` against
/// `SqlConnection::Postgres`) are deliberately not included: this repo has
/// no hermetic embedded-Postgres option (unlike SQLite's
/// `rusqlite`-bundled/in-memory story), and requiring a live external
/// Postgres server in CI would violate this project's established
/// CI-friendly, no-external-service-dependency test preference (see
/// `RML_SQL_PLAN.md`'s "Test plan" note on SQLite in-memory DBs). A future
/// fixture using a real or embedded PostgreSQL instance is tracked as
/// follow-up work, not required for this PR's scope.
use rml::RmlError;
use rml::ast::{LogicalSourceRef, SqlConnection};
use rml::loader::load_mapping_from_str;

fn mapping_with_source(source: &str) -> String {
    format!(
        r#"
@prefix rml: <http://w3id.org/rml/> .
@prefix ex: <http://example.com/> .

<http://example.com/TM>
    a rml:TriplesMap ;
    rml:logicalSource [
        rml:source "{source}" ;
        rml:tableName "student"
    ] ;
    rml:subjectMap [ rml:template "http://example.com/Student/{{id}}" ] .
"#
    )
}

#[test]
fn env_var_reference_resolves_to_postgres_connection_holding_only_the_var_name() {
    // SAFETY: unique var name, not shared with other tests running in
    // parallel in this binary.
    unsafe {
        std::env::set_var("RML_TEST_PG_DSN_OK", "postgres://user:hunter2@localhost/db");
    }
    let mapping = load_mapping_from_str(&mapping_with_source("${RML_TEST_PG_DSN_OK}")).unwrap();
    let source = &mapping.triples_maps[0].logical_source.source;
    match source {
        LogicalSourceRef::Sql(sql_ref) => {
            assert_eq!(
                sql_ref.connection,
                SqlConnection::Postgres("RML_TEST_PG_DSN_OK".to_string()),
                "SqlConnection::Postgres must hold the variable name, not the resolved DSN"
            );
        }
        other => panic!("expected LogicalSourceRef::Sql, got {other:?}"),
    }
    unsafe {
        std::env::remove_var("RML_TEST_PG_DSN_OK");
    }
}

#[test]
fn missing_env_var_is_rejected_at_load_time() {
    // Deliberately not set.
    unsafe {
        std::env::remove_var("RML_TEST_PG_DSN_MISSING");
    }
    let err =
        load_mapping_from_str(&mapping_with_source("${RML_TEST_PG_DSN_MISSING}")).unwrap_err();
    match err {
        RmlError::MissingEnvVar(var) => assert_eq!(var, "RML_TEST_PG_DSN_MISSING"),
        other => panic!("expected RmlError::MissingEnvVar, got {other:?}"),
    }
}

#[test]
fn literal_uri_scheme_connection_string_is_rejected_not_treated_as_sqlite_path() {
    let err = load_mapping_from_str(&mapping_with_source("postgres://user:hunter2@localhost/db"))
        .unwrap_err();
    match err {
        RmlError::InsecureSqlSource { property } => {
            assert_eq!(property, "rml:source");
        }
        other => panic!("expected RmlError::InsecureSqlSource, got {other:?}"),
    }
}

#[test]
fn literal_libpq_keyword_connection_string_with_password_is_rejected() {
    let err = load_mapping_from_str(&mapping_with_source(
        "host=localhost dbname=db user=admin password=hunter2",
    ))
    .unwrap_err();
    match err {
        RmlError::InsecureSqlSource { property } => {
            assert_eq!(property, "rml:source");
        }
        other => panic!("expected RmlError::InsecureSqlSource, got {other:?}"),
    }
}

#[test]
fn error_message_never_echoes_the_rejected_literal_credential() {
    let secret_dsn = "postgres://admin:supersecretpassword@localhost/db";
    let err = load_mapping_from_str(&mapping_with_source(secret_dsn)).unwrap_err();
    let message = err.to_string();
    assert!(
        !message.contains("supersecretpassword"),
        "error message must not echo the rejected literal credential, got: {message}"
    );
}

#[test]
fn plain_sqlite_file_path_is_unaffected_by_dsn_resolution() {
    let mapping = load_mapping_from_str(&mapping_with_source("students.sqlite")).unwrap();
    let source = &mapping.triples_maps[0].logical_source.source;
    match source {
        LogicalSourceRef::Sql(sql_ref) => {
            assert_eq!(
                sql_ref.connection,
                SqlConnection::Sqlite("students.sqlite".into())
            );
        }
        other => panic!("expected LogicalSourceRef::Sql, got {other:?}"),
    }
}
