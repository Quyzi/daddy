//! `brain index` — rebuilds sections, chunks, entities, and edges from
//! already-stored blocks (see `brain ingest`). Safe and fast to re-run
//! after editing a rule pack: nothing here touches a PDF or redoes OCR.

use crate::packs::load_pack;
use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_store::Store;
use std::path::Path;

pub fn run(root: &Path, pack_override: Option<&str>) -> Result<()> {
    let config = BrainConfig::load(root).context("loading .brain/config.toml (run `brain init` first)")?;
    let pack_name = pack_override.unwrap_or(&config.pack);
    let pack = load_pack(pack_name).with_context(|| format!("loading rule pack {pack_name:?}"))?;

    let db_path = BrainConfig::db_path(root);
    let mut store = Store::open(&db_path).context("opening .brain/graph.db")?;

    let report = brain_index::index_all(&mut store, &pack).context("indexing")?;

    println!("documents: {}", report.documents);
    println!("sections:  {}", report.sections);
    println!("chunks:    {}", report.chunks);
    println!("entities:  {}", report.entities);
    println!("mentions:  {}", report.mentions);
    println!("edges:     {}", report.edges);
    Ok(())
}
