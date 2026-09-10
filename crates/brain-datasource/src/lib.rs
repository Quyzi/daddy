//! `Datasource` is the extension point for ingesting live queryable
//! stores — as opposed to files — into the graph: a SQLite database
//! today, with Postgres/MySQL left as documented, not-yet-implemented
//! extension points (see the module docs on [`registry`]) rather than
//! built out speculatively ahead of an actual need.
//!
//! Every backend returns rows in the same shape (`Vec<(column, value)>`,
//! every value already stringified), so `brain-cli`'s block-building code
//! (which turns a row into a heading + field blocks — see the
//! implementation plan's datasource section) never needs to know which
//! backend produced it.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod registry;
pub mod sqlite;

use brain_core::error::Result;

/// One configured read-only query's result: rows of `(column, value)`
/// pairs, in column-declaration order, with every value already
/// rendered as text — regardless of the source column's real type — so
/// downstream block-building code (see `brain-cli`'s `ingest.rs`) never
/// branches on backend-specific type systems.
pub type Row = Vec<(String, String)>;

/// A backend that can run a read-only SQL query against a live store and
/// hand back its rows. Implementations issue *only* the exact query text
/// a caller provides — never anything else — so pointing a `dsn` at a
/// live application database carries no risk of this tool writing to it.
///
/// `Send`, not `Sync`: `brain ingest` opens and queries one connection at
/// a time on a single thread (datasource ingestion is a small,
/// occasional side-pipeline, not something worth parallelizing — see the
/// implementation plan), and `rusqlite::Connection` itself isn't `Sync`,
/// so requiring it here would make the built-in SQLite backend unable to
/// implement its own trait.
pub trait Datasource: Send {
    /// Opens a connection. `dsn` has already had `${ENV_VAR}`
    /// interpolation applied (see [`config::resolve_env`]) by the time
    /// this is called.
    fn connect(dsn: &str) -> Result<Self>
    where
        Self: Sized;

    /// Runs one query, returning every matched row.
    fn query(&self, sql: &str) -> Result<Vec<Row>>;
}
