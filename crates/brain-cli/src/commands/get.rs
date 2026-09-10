//! `brain get <entity>` — a verbatim entity lookup: definition, fields,
//! and graph neighbours.

use anyhow::{Context, Result};
use brain_core::BrainConfig;
use brain_query::{get_entity, render_entity_markdown};
use brain_store::Store;
use std::path::Path;

pub fn run(root: &Path, entity: &str, json: bool) -> Result<()> {
    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db (run `brain init` first)")?;

    match get_entity(&store, entity).context("looking up entity")? {
        Some(view) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&view)?);
            } else {
                print!("{}", render_entity_markdown(&view));
            }
            Ok(())
        }
        None => anyhow::bail!("no entity found matching {entity:?}"),
    }
}
