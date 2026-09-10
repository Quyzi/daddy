//! The one [`Datasource`] backend shipped built in: an arbitrary SQLite
//! file, opened read-only. Also the reference implementation to copy
//! when contributing a new backend — see [`crate::registry`]'s module
//! docs for the exact three steps that involves.

use crate::{Datasource, Row};
use brain_core::error::{BrainError, Result};
use rusqlite::{types::ValueRef, Connection, OpenFlags};

/// A connection to a user-configured SQLite database, distinct from the
/// brain's own `graph.db` (opened by `brain-store::Store` instead) —
/// this is *external* data being pulled into the graph, not the graph's
/// own storage.
pub struct SqliteDatasource(Connection);

impl Datasource for SqliteDatasource {
    fn connect(dsn: &str) -> Result<Self> {
        // Read-only by construction: `SQLITE_OPEN_READ_ONLY` makes any
        // accidental write attempt (there should never be one — see this
        // trait's docs) fail loudly instead of silently mutating the
        // user's data.
        let conn = Connection::open_with_flags(dsn, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| BrainError::Extraction(format!("opening sqlite datasource {dsn:?}: {e}")))?;
        Ok(Self(conn))
    }

    fn query(&self, sql: &str) -> Result<Vec<Row>> {
        let mut stmt = self
            .0
            .prepare(sql)
            .map_err(|e| BrainError::Extraction(format!("preparing query: {e}")))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
        let rows = stmt
            .query_map([], |row| {
                let mut out = Row::with_capacity(columns.len());
                for (i, col) in columns.iter().enumerate() {
                    let value = row.get_ref(i)?;
                    out.push((col.clone(), value_to_string(value)));
                }
                Ok(out)
            })
            .map_err(|e| BrainError::Extraction(format!("running query: {e}")))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Extraction(format!("reading query results: {e}")))
    }
}

/// Renders any SQLite column value as text, regardless of its declared
/// type — this backend's whole job is presenting rows in the one
/// backend-agnostic shape [`crate::Datasource::query`] promises, so a
/// caller building `"{column}: {value}"` field blocks never needs a
/// per-type match of its own.
fn value_to_string(value: ValueRef) -> String {
    match value {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => format!("<{} bytes of binary data>", b.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection as RawConnection;

    #[test]
    fn queries_a_real_sqlite_file_and_stringifies_every_column_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("npcs.sqlite3");
        {
            let setup = RawConnection::open(&path).unwrap();
            setup
                .execute_batch(
                    "CREATE TABLE npcs (name TEXT, level INTEGER, gold REAL, notes TEXT);
                     INSERT INTO npcs VALUES ('Strahd', 20, 0.0, NULL);",
                )
                .unwrap();
        }

        let ds = SqliteDatasource::connect(path.to_str().unwrap()).unwrap();
        let rows = ds.query("SELECT name, level, gold, notes FROM npcs").unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row[0], ("name".to_string(), "Strahd".to_string()));
        assert_eq!(row[1], ("level".to_string(), "20".to_string()));
        assert_eq!(row[2], ("gold".to_string(), "0".to_string()));
        assert_eq!(row[3], ("notes".to_string(), String::new()));
    }

    #[test]
    fn connecting_read_only_rejects_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readonly.sqlite3");
        RawConnection::open(&path).unwrap().execute_batch("CREATE TABLE t (x)").unwrap();

        let ds = SqliteDatasource::connect(path.to_str().unwrap()).unwrap();
        let err = ds.0.execute("INSERT INTO t VALUES (1)", []).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("read"));
    }
}
