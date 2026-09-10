//! Builds one entity's wiki page content — deterministic markdown with
//! YAML frontmatter, following the conventions in this project's brain
//! `CLAUDE.md` schema (frontmatter, `[[page-name]]` links, `[Source:
//! ...]` citations).

use brain_core::error::Result;
use brain_core::{slugify, Entity};
use brain_store::Store;
use chrono::NaiveDate;
use std::collections::BTreeMap;

/// Maximum related-entity links shown, heaviest edge first.
const MAX_RELATED: usize = 20;

/// Builds the full markdown content for `entity`'s page. Always marks
/// `status: generated` in the frontmatter — callers are responsible for
/// not calling this for a page whose on-disk `status` is `curated` (see
/// [`crate::frontmatter::read_status`]).
pub fn build_page(store: &Store, entity: &Entity, today: NaiveDate) -> Result<String> {
    let entity_id = entity.id.expect("entities read back from storage always have an id");
    let source_count = store.definition_doc_count(entity_id)?;

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(&format!("title: {}\n", entity.name));
    out.push_str(&format!("kind: {}\n", entity.kind.as_str()));
    out.push_str(&format!("created: {today}\n"));
    out.push_str(&format!("last_updated: {today}\n"));
    out.push_str(&format!("source_count: {source_count}\n"));
    out.push_str("status: generated\n");
    out.push_str("---\n\n");

    write_definition(store, entity_id, &mut out)?;
    write_fields(store, entity_id, &mut out)?;
    write_related(store, entity_id, &mut out)?;
    write_appears_in(store, entity_id, &mut out)?;

    Ok(out)
}

fn write_definition(store: &Store, entity_id: brain_core::EntityId, out: &mut String) -> Result<()> {
    let Some((chunk_id, doc_id, page_no)) = store.primary_definition(entity_id)? else {
        out.push_str("_No definition on file yet — this entity is known only from mentions elsewhere in the corpus._\n\n");
        return Ok(());
    };
    let chunk = store.get_chunk(chunk_id)?;
    let doc = store.get_document(doc_id)?;
    out.push_str("## Definition\n\n");
    for line in chunk.text.trim().lines() {
        out.push_str("> ");
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("\n[Source: {} p.{}]\n\n", doc.title, page_no));
    Ok(())
}

fn write_fields(store: &Store, entity_id: brain_core::EntityId, out: &mut String) -> Result<()> {
    let fields = store.list_fields(entity_id)?;
    if fields.is_empty() {
        return Ok(());
    }
    out.push_str("## Fields\n\n| key | value | source |\n|---|---|---|\n");

    let mut by_key: BTreeMap<&str, Vec<&brain_store::EntityField>> = BTreeMap::new();
    for f in &fields {
        by_key.entry(f.key.as_str()).or_default().push(f);
    }
    for (key, values) in &by_key {
        for f in values {
            let doc_title = store.get_document(f.doc_id).map(|d| d.title).unwrap_or_default();
            out.push_str(&format!("| {key} | {} | {doc_title} |\n", f.value));
        }
        if values.len() > 1 {
            out.push_str(&format!(
                "| _⚠ {} sources disagree on **{key}**_ | | |\n",
                values.len()
            ));
        }
    }
    out.push('\n');
    Ok(())
}

fn write_related(store: &Store, entity_id: brain_core::EntityId, out: &mut String) -> Result<()> {
    let mut edges = store.edges_touching(entity_id)?;
    edges.sort_by(|a, b| b.weight.partial_cmp(&a.weight).unwrap());
    if edges.is_empty() {
        return Ok(());
    }
    let mut links = Vec::new();
    for edge in edges.into_iter().take(MAX_RELATED) {
        let other_id = if edge.src == entity_id { edge.dst } else { edge.src };
        let other = store.get_entity(other_id)?;
        links.push(format!("[[{}]]", other.slug));
    }
    out.push_str("## Related\n\n");
    out.push_str(&links.join(" \u{b7} "));
    out.push_str("\n\n");
    Ok(())
}

fn write_appears_in(store: &Store, entity_id: brain_core::EntityId, out: &mut String) -> Result<()> {
    let defs = store.list_definitions(entity_id)?;
    if defs.len() <= 1 {
        return Ok(()); // nothing beyond (or without) the primary definition to list
    }
    out.push_str("## Appears in\n\n");
    for (doc_id, page_no, _) in &defs {
        let doc = store.get_document(*doc_id)?;
        out.push_str(&format!("- [[{}]] p.{page_no}\n", slugify(&doc.title)));
    }
    out.push('\n');
    Ok(())
}
