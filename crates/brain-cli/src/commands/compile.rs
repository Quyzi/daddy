//! `brain compile` — generates/updates `wiki/*.md` from the current
//! graph, preserving any page a human or the AI has since curated.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use std::path::{Path, PathBuf};

pub fn run(root: &Path, out: Option<&Path>, min_centrality: f64) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;

    let wiki_dir: PathBuf = out.map(Path::to_path_buf).unwrap_or_else(|| root.join("wiki"));
    let report = brain_wiki::compile(&store, &wiki_dir, min_centrality).context("compiling wiki")?;

    println!("wiki directory:   {}", wiki_dir.display());
    println!("candidates:       {}", report.total_candidates);
    println!("written/updated:  {}", report.written);
    println!("skipped (curated):{}", report.skipped_curated);
    Ok(())
}
