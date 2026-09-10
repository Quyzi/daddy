//! `brain stats` — reports basic counts once a graph exists.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use std::path::Path;

#[allow(missing_docs)]
pub fn run(root: &Path, json: bool) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;
    let docs = store.list_documents()?;
    let total_pages: u32 = docs.iter().map(|d| d.page_count).sum();
    let entities = store.list_entities(None)?;

    if json {
        let out = serde_json::json!({
            "documents": docs.len(),
            "pages": total_pages,
            "entities": entities.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!("documents: {}", docs.len());
        println!("pages:     {total_pages}");
        println!("entities:  {}", entities.len());
    }
    Ok(())
}
