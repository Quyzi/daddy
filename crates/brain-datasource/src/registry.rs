//! Maps a [`crate::config::DatasourceConfig`]'s `kind` string to a
//! concrete [`crate::Datasource`] implementation.
//!
//! **Adding a new backend** (Postgres, MySQL, anything else with a
//! `SELECT` concept) means exactly three steps, and nothing else in the
//! workspace needs to change:
//! 1. Add a new module (`postgres.rs`, `mysql.rs`, ...) implementing
//!    [`crate::Datasource`] — copy `sqlite.rs`'s shape: a struct wrapping
//!    the backend's connection type, `connect` opening it, `query`
//!    running one statement and stringifying every returned column via
//!    [`crate::Row`]'s contract.
//! 2. Feature-gate it in `Cargo.toml` (`postgres = ["dep:postgres"]`) so
//!    the default build doesn't pay for a dependency most brains never
//!    use.
//! 3. Add one match arm below, under the same feature flag.
//!
//! Until that happens, `"postgres"`/`"mysql"` fail here with a clear,
//! actionable error rather than a confusing one three layers down.

use crate::sqlite::SqliteDatasource;
use crate::Datasource;
use brain_core::error::{BrainError, Result};

/// Opens a [`Datasource`] for `kind`, already-`${ENV_VAR}`-resolved
/// `dsn` in hand (see [`crate::config::resolve_env`]).
pub fn open(kind: &str, dsn: &str) -> Result<Box<dyn Datasource>> {
    match kind {
        "sqlite" => Ok(Box::new(SqliteDatasource::connect(dsn)?)),
        "postgres" | "mysql" => Err(BrainError::InvalidData(format!(
            "datasource kind {kind:?} is not implemented yet — it's a documented extension \
             point (see crates/brain-datasource/src/registry.rs's module docs for the three \
             steps to add it), not a bug. \"sqlite\" is the only kind available today."
        ))),
        other => Err(BrainError::InvalidData(format!(
            "unknown datasource kind {other:?} (expected \"sqlite\")"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unimplemented_kind_is_a_clear_error_not_a_panic() {
        let Err(err) = open("postgres", "postgres://localhost/db") else {
            panic!("expected an error for an unimplemented kind");
        };
        assert!(err.to_string().contains("not implemented yet"));
    }

    #[test]
    fn unknown_kind_is_a_clear_error() {
        let Err(err) = open("mongodb", "mongodb://localhost") else {
            panic!("expected an error for an unknown kind");
        };
        assert!(err.to_string().contains("unknown datasource kind"));
    }
}
