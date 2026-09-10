//! `brain lint` — reports contradictions and orphaned entities: the
//! mechanically-derivable half of the brain schema's Lint Workflow.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use brain_wiki::{lint, render_lint_markdown};
use std::path::Path;

pub fn run(root: &Path, json: bool) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;

    let report = lint(&store).context("running lint")?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render_lint_markdown(&report));
    }
    Ok(())
}
