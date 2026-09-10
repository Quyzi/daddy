//! `brain init` — scaffold a new brain's `.brain/` directory and database.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use std::path::Path;

/// Creates `{root}/.brain/` with a config file and an empty, migrated
/// database. Safe to re-run: an existing config's `pack` is left alone
/// unless explicitly overwritten by a fresh `--pack`.
pub fn run(root: &Path, pack: &str) -> Result<()> {
    std::fs::create_dir_all(root)
        .with_context(|| format!("creating brain root {}", root.display()))?;

    let mut config = BrainConfig::load(root).unwrap_or_default();
    config.pack = pack.to_string();
    config.save(root).context("writing .brain/config.toml")?;

    let db_path = BrainConfig::db_path(root);
    Store::open(&db_path).context("creating .brain/graph.db")?;

    println!(
        "Initialized brain at {} (pack: {pack}, db: {})",
        root.display(),
        db_path.display()
    );
    Ok(())
}
