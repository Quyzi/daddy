//! `brain explore` — ranked, cited, budgeted search over the graph.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_query::{explore, render_explore_markdown, ExploreOptions};
use brain_store::Store;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn run(root: &Path, query: &str, budget: usize, hops: u32, kind: Option<&str>, json: bool) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;

    let opts = ExploreOptions { budget_tokens: budget, hops, kind: kind.map(String::from), ..Default::default() };
    let result = explore(&store, query, &opts).context("running explore query")?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print!("{}", render_explore_markdown(&result));
    }
    Ok(())
}
