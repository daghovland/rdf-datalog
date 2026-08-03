use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use super::RawRow;
use crate::RmlError;
use crate::ast::{SqlConnection, SqlQuery};
use crate::plan::JoinCondition;

/// A SQL `LogicalSource` scan: connect to a database and run one query
/// (whole-table or arbitrary SELECT), yielding rows in the same
/// column-name -> String shape CSV rows use. See
/// [`docs/plans/RML_SQL_PLAN.md`](../../../docs/plans/RML_SQL_PLAN.md) and
/// [#26](https://github.com/daghovland/rdf-datalog/issues/26).
pub struct SqlSource {
    pub connection: SqlConnection,
    pub query: SqlQuery,
    /// Override for the default MAX_SOURCE_ROWS limit (used in tests).
    pub row_limit: Option<usize>,
}

impl SqlSource {
    pub fn new(connection: SqlConnection, query: SqlQuery) -> Self {
        SqlSource {
            connection,
            query,
            row_limit: None,
        }
    }

    pub fn with_row_limit(mut self, rows: usize) -> Self {
        self.row_limit = Some(rows);
        self
    }

    /// `self.connection`'s path must already be sandbox-confined — callers
    /// resolve `SqlConnection::Sqlite`'s relative path against `base_dir`
    /// via `confine_path` before constructing a `SqlSource` (see
    /// `engine.rs::scan_rows`), so this connects to it directly rather than
    /// re-deriving or re-checking the path.
    pub fn rows(&self) -> Box<dyn Iterator<Item = Result<RawRow, RmlError>> + '_> {
        match self.collect_rows() {
            Ok(rows) => Box::new(rows.into_iter().map(Ok)),
            Err(e) => Box::new(std::iter::once(Err(e))),
        }
    }

    fn collect_rows(&self) -> Result<Vec<RawRow>, RmlError> {
        let sql = base_query_sql(&self.query);
        let row_limit = self.row_limit.unwrap_or(crate::MAX_SOURCE_ROWS);
        match &self.connection {
            SqlConnection::Sqlite(db_path) => {
                let conn = open_readonly(db_path)?;
                run_query(&conn, &sql, row_limit)
            }
            SqlConnection::Postgres(var_name) => {
                let mut client = connect_postgres(var_name)?;
                run_query_postgres(&mut client, &sql, row_limit)
            }
        }
    }
}

/// One open connection to either backend, abstracting over the differing
/// rusqlite/postgres APIs enough for join-pushdown's shared operations:
/// column-introspection and running a SELECT to completion. See
/// `engine.rs::execute_sql_pushdown_join`. Phase 5 —
/// [#354](https://github.com/daghovland/rdf-datalog/issues/354).
pub enum SqlConn {
    Sqlite(Connection),
    Postgres(postgres::Client),
}

impl SqlConn {
    /// Open the connection a `SqlPushdown` join needs. `Sqlite` resolves and
    /// sandbox-confines its relative path against `base_dir` first —
    /// pushdown bypasses `engine.rs::scan_rows`'s per-side confinement
    /// entirely, so this does it once here instead. `Postgres` has no path
    /// to confine; it just connects.
    pub fn open_for_join(
        connection: &SqlConnection,
        base_dir: &std::path::Path,
    ) -> Result<Self, RmlError> {
        match connection {
            SqlConnection::Sqlite(rel_path) => {
                let path = crate::sandbox::confine_path(base_dir, rel_path)?;
                Ok(SqlConn::Sqlite(open_readonly(&path)?))
            }
            SqlConnection::Postgres(var_name) => Ok(SqlConn::Postgres(connect_postgres(var_name)?)),
        }
    }

    pub fn discover_columns(&mut self, base_sql: &str) -> Result<Vec<String>, RmlError> {
        match self {
            SqlConn::Sqlite(conn) => discover_columns(conn, base_sql),
            SqlConn::Postgres(client) => discover_columns_postgres(client, base_sql),
        }
    }

    pub fn run_query(&mut self, sql: &str, row_limit: usize) -> Result<Vec<RawRow>, RmlError> {
        match self {
            SqlConn::Sqlite(conn) => run_query(conn, sql, row_limit),
            SqlConn::Postgres(client) => run_query_postgres(client, sql, row_limit),
        }
    }
}

/// Connect to PostgreSQL, re-reading `var_name` from the process environment
/// (never storing the resolved DSN anywhere — see
/// `ast::SqlConnection::Postgres`'s doc comment). The loader already
/// validated at mapping-load time that `var_name` is set
/// (`loader::resolve_sql_connection`), but re-checks here rather than
/// trusting that nothing changed between load and execution.
fn connect_postgres(var_name: &str) -> Result<postgres::Client, RmlError> {
    let dsn = std::env::var(var_name).map_err(|_| RmlError::MissingEnvVar(var_name.to_string()))?;
    postgres::Client::connect(&dsn, postgres::NoTls).map_err(|e| RmlError::Postgres {
        context: format!("${{{var_name}}}"),
        source: e,
    })
}

/// Column names a base query yields, via `Client::prepare` (which parses
/// and plans but does not execute) rather than SQLite's `LIMIT 0` trick —
/// `postgres::Statement` exposes column metadata directly from the prepared
/// statement.
fn discover_columns_postgres(
    client: &mut postgres::Client,
    base_sql: &str,
) -> Result<Vec<String>, RmlError> {
    let sql = format!("SELECT * FROM ({base_sql}) AS t");
    let stmt = client.prepare(&sql).map_err(|e| RmlError::Postgres {
        context: sql.clone(),
        source: e,
    })?;
    Ok(stmt
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect())
}

/// Run `sql` against an open PostgreSQL client, mapping every row into a
/// `RawRow` the same way `run_query` does for SQLite.
fn run_query_postgres(
    client: &mut postgres::Client,
    sql: &str,
    row_limit: usize,
) -> Result<Vec<RawRow>, RmlError> {
    let rows = client.query(sql, &[]).map_err(|e| RmlError::Postgres {
        context: sql.to_string(),
        source: e,
    })?;
    if rows.len() > row_limit {
        return Err(RmlError::SourceTooLarge {
            limit: row_limit as u64,
            actual: rows.len() as u64,
        });
    }
    let mut result = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut raw = RawRow::new();
        for (idx, col) in row.columns().iter().enumerate() {
            if let Some(lexical) = postgres_value_to_string(row, idx) {
                raw.insert(col.name().to_string(), lexical);
            }
        }
        result.push(raw);
    }
    Ok(result)
}

/// Render a PostgreSQL value as its string lexical form, or `None` for
/// `NULL` — same "no value" convention `sql_value_to_string` uses for
/// SQLite. Tries the common scalar types in turn (`postgres::Row::try_get`
/// fails cleanly on a type mismatch rather than panicking, unlike `get`).
fn postgres_value_to_string(row: &postgres::Row, idx: usize) -> Option<String> {
    if let Ok(v) = row.try_get::<_, Option<String>>(idx) {
        return v;
    }
    if let Ok(v) = row.try_get::<_, Option<i64>>(idx) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<_, Option<i32>>(idx) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<_, Option<f64>>(idx) {
        return v.map(|n| n.to_string());
    }
    if let Ok(v) = row.try_get::<_, Option<bool>>(idx) {
        return v.map(|b| b.to_string());
    }
    log::warn!("Postgres source: unsupported column type at index {idx}, value skipped");
    None
}

/// Open a read-only connection to a SQLite database file. Shared by plain
/// scans (`SqlSource::collect_rows`) and the SQL-pushdown join path
/// (`engine.rs::execute_sql_pushdown_join`), which opens one connection and
/// runs both introspection and the synthesized join query against it.
pub fn open_readonly(db_path: &std::path::Path) -> Result<Connection, RmlError> {
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| RmlError::Sql {
        context: db_path.display().to_string(),
        source: e,
    })
}

/// The base `SELECT` text for a `SqlQuery`, without any pushdown wrapping.
/// `SqlQuery::Query` text is trimmed of trailing whitespace and a trailing
/// `;` so it composes safely as a subquery (`FROM (<text>) AS c`) — a bare
/// `SELECT ...;` would otherwise break that wrapping.
pub fn base_query_sql(query: &SqlQuery) -> String {
    match query {
        SqlQuery::Table(name) => format!("SELECT * FROM {}", quote_identifier(name)),
        SqlQuery::Query(query) => query.trim().trim_end_matches(';').trim().to_string(),
    }
}

/// Run `sql` (no bound parameters) against `conn`, mapping every row into a
/// `RawRow`, subject to `row_limit`. Shared by plain SQL scans and the
/// SQL-pushdown join's synthesized query — both just need "run this SELECT
/// text, get column-keyed rows back".
pub fn run_query(conn: &Connection, sql: &str, row_limit: usize) -> Result<Vec<RawRow>, RmlError> {
    let mut stmt = conn.prepare(sql).map_err(|e| RmlError::Sql {
        context: sql.to_string(),
        source: e,
    })?;

    let column_names: Vec<String> = stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let mut rows = Vec::new();
    let mut sql_rows = stmt.query([]).map_err(|e| RmlError::Sql {
        context: sql.to_string(),
        source: e,
    })?;

    while let Some(sql_row) = sql_rows.next().map_err(|e| RmlError::Sql {
        context: sql.to_string(),
        source: e,
    })? {
        if rows.len() >= row_limit {
            return Err(RmlError::SourceTooLarge {
                limit: row_limit as u64,
                actual: rows.len() as u64 + 1,
            });
        }
        let mut row = RawRow::new();
        for (idx, col) in column_names.iter().enumerate() {
            let value_ref = sql_row.get_ref(idx).map_err(|e| RmlError::Sql {
                context: sql.to_string(),
                source: e,
            })?;
            // NULL columns are omitted entirely, matching CsvRow::get_str's
            // treatment of missing/empty values as "no value" — a NULL and
            // an absent column mean the same thing to the rest of the
            // pipeline (the triple/attribute referencing it is skipped).
            if let Some(lexical) = sql_value_to_string(value_ref) {
                row.insert(col.clone(), lexical);
            }
        }
        rows.push(row);
    }

    Ok(rows)
}

/// Discover the column names a base query (whole-table or arbitrary SELECT)
/// yields, without fetching any rows (`LIMIT 0`). Used to build the
/// column-prefixed `SELECT` list for SQL-pushdown joins — the synthesized
/// query's shape depends on the parent/child columns, which aren't known
/// statically for `rml:tableName`/`rml:sqlQuery` sources.
pub fn discover_columns(conn: &Connection, base_sql: &str) -> Result<Vec<String>, RmlError> {
    let sql = format!("SELECT * FROM ({base_sql}) LIMIT 0");
    let stmt = conn.prepare(&sql).map_err(|e| RmlError::Sql {
        context: sql.clone(),
        source: e,
    })?;
    Ok(stmt
        .column_names()
        .into_iter()
        .map(|s| s.to_string())
        .collect())
}

/// Synthesize the single SQL query that performs a same-connection SQL/SQL
/// join in the database (`RML_SQL_PLAN.md`'s "Efficient joins", tier 1). Pure
/// string-building — no database access — so it's directly unit-testable
/// (proving pushdown actually happened, not just that results are correct).
///
/// Every selected column is aliased `child_<col>`/`parent_<col>`, which
/// disambiguates child/parent columns of the same name (the exact
/// correctness hazard `RML_JOIN_PLAN.md` flags for the hash-join tier) —
/// `engine.rs`'s pushdown execution reads `child_<col>` for child-side
/// attributes and `parent_<col>` for the join `Object`'s term-map logic.
///
/// Each join condition also gets an `AND c.<col> <> '' AND p.<col> <> ''`
/// guard. Without it, SQL's `''= ''` would match empty-string join keys,
/// while the hash-join tier's `SourceRow::get_str` treats an empty string as
/// "no value" and never builds a key for it (same treatment as SQL `NULL`) —
/// so an unguarded `ON` clause would make the two tiers observably
/// non-equivalent on empty-string join columns. `NULL` needs no separate
/// guard: SQL's `NULL = NULL` is already `NULL` (never true), matching the
/// hash join's "no key, no match" behaviour for missing columns.
pub fn build_pushdown_query(
    child_base: &str,
    parent_base: &str,
    child_cols: &[String],
    parent_cols: &[String],
    conditions: &[JoinCondition],
) -> String {
    let select_list: Vec<String> = child_cols
        .iter()
        .map(|c| {
            format!(
                "c.{} AS {}",
                quote_identifier(c),
                quote_identifier(&format!("child_{c}"))
            )
        })
        .chain(parent_cols.iter().map(|c| {
            format!(
                "p.{} AS {}",
                quote_identifier(c),
                quote_identifier(&format!("parent_{c}"))
            )
        }))
        .collect();

    let on_clause: Vec<String> = conditions
        .iter()
        .map(|jc| {
            let c_col = quote_identifier(&jc.left_column);
            let p_col = quote_identifier(&jc.right_column);
            format!("(c.{c_col} = p.{p_col} AND c.{c_col} <> '' AND p.{p_col} <> '')")
        })
        .collect();

    format!(
        "SELECT {} FROM ({child_base}) AS c JOIN ({parent_base}) AS p ON {}",
        select_list.join(", "),
        on_clause.join(" AND "),
    )
}

/// Render a SQL value as its string lexical form, or `None` for `NULL`
/// (and, by the same "no value" convention, for `BLOB` — RML has no binary
/// term type to map a BLOB onto, so rather than corrupting it into a lossy
/// "String" form, the column is treated as absent, same as NULL, and
/// logged so a mapping author can see why an expected column vanished).
fn sql_value_to_string(value: ValueRef<'_>) -> Option<String> {
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(i) => Some(i.to_string()),
        ValueRef::Real(f) => Some(f.to_string()),
        ValueRef::Text(t) => Some(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(_) => {
            log::warn!("SQL source: BLOB column value skipped (no RML binary term type)");
            None
        }
    }
}

/// Quote a SQL identifier (table name or column alias) for safe
/// interpolation — identifiers can't be bound as query parameters in
/// `rusqlite`/SQLite, so this doubles any embedded `"` the same way
/// standard SQL identifier-quoting does.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::JoinCondition;

    #[test]
    fn build_pushdown_query_prefixes_and_aliases_every_column() {
        let sql = build_pushdown_query(
            "SELECT * FROM \"student\"",
            "SELECT * FROM \"sport\"",
            &["id".to_string(), "sport_id".to_string()],
            &["id".to_string(), "name".to_string()],
            &[JoinCondition {
                left_column: "sport_id".to_string(),
                right_column: "id".to_string(),
            }],
        );
        assert_eq!(
            sql,
            "SELECT c.\"id\" AS \"child_id\", c.\"sport_id\" AS \"child_sport_id\", \
             p.\"id\" AS \"parent_id\", p.\"name\" AS \"parent_name\" \
             FROM (SELECT * FROM \"student\") AS c JOIN (SELECT * FROM \"sport\") AS p \
             ON (c.\"sport_id\" = p.\"id\" AND c.\"sport_id\" <> '' AND p.\"id\" <> '')"
        );
    }

    #[test]
    fn build_pushdown_query_ands_multiple_conditions() {
        let sql = build_pushdown_query(
            "SELECT * FROM \"a\"",
            "SELECT * FROM \"b\"",
            &["x".to_string()],
            &["y".to_string()],
            &[
                JoinCondition {
                    left_column: "x".to_string(),
                    right_column: "y".to_string(),
                },
                JoinCondition {
                    left_column: "z".to_string(),
                    right_column: "w".to_string(),
                },
            ],
        );
        assert!(sql.contains(
            "ON (c.\"x\" = p.\"y\" AND c.\"x\" <> '' AND p.\"y\" <> '') AND (c.\"z\" = p.\"w\" AND c.\"z\" <> '' AND p.\"w\" <> '')"
        ));
    }

    #[test]
    fn quote_identifier_doubles_embedded_quotes() {
        assert_eq!(quote_identifier("a\"b"), "\"a\"\"b\"");
    }
}
