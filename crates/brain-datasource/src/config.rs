//! `.brain/sources.toml`: the optional, sibling config to
//! `.brain/config.toml` that declares live datasources for `brain
//! ingest` to query, alongside (not instead of) its usual `raw/` file
//! walk.
//!
//! ```toml
//! [[datasource]]
//! name = "campaign-db"
//! kind = "sqlite"
//! dsn  = "raw/campaign.sqlite3"
//!
//! [[datasource.query]]
//! name = "npc_roster"
//! sql  = "SELECT name, race, location, notes FROM npcs ORDER BY name"
//! ```
//!
//! **Credentials never belong in this file** — it lives inside a brain
//! that is itself a real git repository (see `brains/*/CLAUDE.md`'s `##
//! Git` section), so a `dsn` containing a bare password would get
//! committed. Write `${ENV_VAR}` inside `dsn` instead; [`resolve_env`]
//! substitutes it from the process environment at ingest time only, so
//! the committed file never carries a secret. This matters most for a
//! future Postgres/MySQL backend (a live server with real credentials);
//! a `sqlite` `dsn` is just a path and has nothing to leak, but the same
//! interpolation applies uniformly regardless of kind.

use brain_core::error::{BrainError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// Parsed `.brain/sources.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SourcesConfig {
    /// Every configured datasource, in file order.
    #[serde(default, rename = "datasource")]
    pub datasources: Vec<DatasourceConfig>,
}

/// One `[[datasource]]` table: a single connection, with one or more
/// queries run against it.
#[derive(Debug, Clone, Deserialize)]
pub struct DatasourceConfig {
    /// Human-readable name, used in document titles/citations.
    pub name: String,
    /// Backend kind — see [`crate::registry::open`] for what's actually
    /// implemented today.
    pub kind: String,
    /// Connection string: a filesystem path for `sqlite`, a connection
    /// URL for anything else. May contain `${ENV_VAR}` references — see
    /// this module's docs.
    pub dsn: String,
    /// The read-only queries to run against this connection.
    #[serde(default, rename = "query")]
    pub queries: Vec<QueryConfig>,
}

/// One configured query: becomes exactly one synthetic `Document` per
/// ingest run (see the implementation plan's datasource section for the
/// row -> heading+field-blocks shape this produces).
#[derive(Debug, Clone, Deserialize)]
pub struct QueryConfig {
    /// Human-readable name, used as the resulting document's title.
    pub name: String,
    /// The exact `SELECT` to run — issued verbatim, never modified.
    pub sql: String,
    /// Which returned column becomes each row's heading text. Defaults
    /// to the first column when unset.
    #[serde(default)]
    pub heading_column: Option<String>,
}

impl SourcesConfig {
    /// Path to a brain's optional datasource config.
    pub fn path(root: &Path) -> PathBuf {
        root.join(".brain").join("sources.toml")
    }

    /// Loads `.brain/sources.toml` if it exists, returning `None`
    /// (rather than an error) when a brain has no datasources configured
    /// — this is an optional, additive feature, not a required one.
    pub fn load(root: &Path) -> Result<Option<Self>> {
        let path = Self::path(root);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&text)
            .map_err(|e| BrainError::InvalidData(format!("{}: {e}", path.display())))?;
        Ok(Some(config))
    }
}

/// Substitutes every `${VAR_NAME}` in `dsn` with that environment
/// variable's value. A reference to a variable that isn't set is left as
/// an error rather than silently becoming an empty string — a DSN with a
/// silently-blanked password would fail in a much more confusing way
/// (connection refused, not "credential missing") than at ingest time.
pub fn resolve_env(dsn: &str) -> Result<String> {
    let mut out = String::with_capacity(dsn.len());
    let mut rest = dsn;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return Err(BrainError::InvalidData(format!(
                "unterminated ${{...}} in datasource dsn {dsn:?}"
            )));
        };
        let var = &after[..end];
        let value = std::env::var(var).map_err(|_| {
            BrainError::InvalidData(format!(
                "datasource dsn references ${{{var}}}, which is not set in the environment"
            ))
        })?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_sources_toml() {
        let toml = r#"
[[datasource]]
name = "campaign-db"
kind = "sqlite"
dsn  = "raw/campaign.sqlite3"

[[datasource.query]]
name = "npc_roster"
sql  = "SELECT name FROM npcs"
"#;
        let config: SourcesConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.datasources.len(), 1);
        assert_eq!(config.datasources[0].queries.len(), 1);
        assert_eq!(config.datasources[0].queries[0].name, "npc_roster");
    }

    #[test]
    fn resolves_env_var_references() {
        std::env::set_var("BRAIN_TEST_DSN_VAR", "secretpass");
        let resolved = resolve_env("postgres://user:${BRAIN_TEST_DSN_VAR}@localhost/db").unwrap();
        assert_eq!(resolved, "postgres://user:secretpass@localhost/db");
        std::env::remove_var("BRAIN_TEST_DSN_VAR");
    }

    #[test]
    fn missing_env_var_is_an_error_not_a_blank_substitution() {
        let err = resolve_env("postgres://user:${BRAIN_TEST_DEFINITELY_UNSET}@localhost/db").unwrap_err();
        assert!(err.to_string().contains("BRAIN_TEST_DEFINITELY_UNSET"));
    }

    #[test]
    fn dsn_with_no_placeholders_is_unchanged() {
        assert_eq!(resolve_env("raw/campaign.sqlite3").unwrap(), "raw/campaign.sqlite3");
    }

    #[test]
    fn missing_sources_toml_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(SourcesConfig::load(dir.path()).unwrap().is_none());
    }
}
