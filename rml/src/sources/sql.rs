use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use super::RawRow;
use crate::RmlError;
use crate::ast::{SqlConnection, SqlQuery};

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
        let SqlConnection::Sqlite(db_path) = &self.connection;

        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| RmlError::Sql {
            context: db_path.display().to_string(),
            source: e,
        })?;

        let sql = match &self.query {
            SqlQuery::Table(name) => format!("SELECT * FROM {}", quote_identifier(name)),
            SqlQuery::Query(query) => query.clone(),
        };

        let mut stmt = conn.prepare(&sql).map_err(|e| RmlError::Sql {
            context: sql.clone(),
            source: e,
        })?;

        let column_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let row_limit = self.row_limit.unwrap_or(crate::MAX_SOURCE_ROWS);

        let mut rows = Vec::new();
        let mut sql_rows = stmt.query([]).map_err(|e| RmlError::Sql {
            context: sql.clone(),
            source: e,
        })?;

        while let Some(sql_row) = sql_rows.next().map_err(|e| RmlError::Sql {
            context: sql.clone(),
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
                    context: sql.clone(),
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

/// Quote a SQL identifier (table name) for safe interpolation into
/// `SELECT * FROM <name>` — identifiers can't be bound as query parameters
/// in `rusqlite`/SQLite, so this doubles any embedded `"` the same way
/// standard SQL identifier-quoting does.
fn quote_identifier(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}
