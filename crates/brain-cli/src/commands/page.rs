//! `brain page <doc> <n>` — prints one page's reading-order text
//! verbatim. The escape hatch when a query result isn't enough context:
//! go read the whole page it came from.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use std::path::Path;

pub fn run(root: &Path, doc: &str, page: u32) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;

    let document = store
        .find_document_by_title(doc)?
        .with_context(|| format!("no document found matching title {doc:?} (try `brain stats` or check the exact title)"))?;
    let doc_id = document.id.expect("documents read back from storage always have an id");
    let page_row = store
        .get_page(doc_id, page)
        .with_context(|| format!("page {page} of {:?} (document has {} pages)", document.title, document.page_count))?;

    println!("{}", page_row.text);
    Ok(())
}
