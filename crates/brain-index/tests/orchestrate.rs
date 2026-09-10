//! End-to-end test of `index_all` against an in-memory store, using
//! hand-built documents/pages/blocks (standing in for what `brain
//! ingest` + `brain-layout` would have produced) rather than real PDFs —
//! this exercises the full sections -> chunks -> entities -> gazetteer ->
//! edges -> centrality pipeline in one shot.

use brain_core::{
    BBox, Block, BlockKind, Document, EntityKind, ExtractorKind, Page,
};
use brain_index::{index_all, CompiledPack, RulePack};
use brain_store::Store;
use chrono::Utc;

const PACK_TOML: &str = r#"
[[entity]]
kind = "spell"
require = [{ regex = "Casting Time:", within_chars = 300 }]
fields = ["Casting Time", "Range"]

[[entity]]
kind = "topic"
atomic = false
"#;

fn insert_doc_with_blocks(store: &Store, title: &str, blocks_by_page: Vec<Vec<(BlockKind, &str)>>) -> brain_core::DocId {
    let doc_id = store
        .insert_document(&Document {
            id: None,
            path: format!("/tmp/{title}.pdf"),
            sha256: title.to_string(),
            title: title.to_string(),
            kind: "pdf".to_string(),
            page_count: blocks_by_page.len() as u32,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Poppler,
            ocr: false,
            frontmatter: None,
        })
        .unwrap();

    for (i, page_blocks) in blocks_by_page.iter().enumerate() {
        let page_no = (i + 1) as u32;
        let page_id = store
            .insert_page(&Page {
                id: None,
                doc_id,
                page_no,
                width: 600.0,
                height: 800.0,
                text: String::new(),
                ocr_conf: None,
                low_confidence: false,
            })
            .unwrap();
        for (ord, (kind, text)) in page_blocks.iter().enumerate() {
            store
                .insert_block(&Block {
                    id: None,
                    page_id,
                    col: 0,
                    ord: ord as u32,
                    bbox: BBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 },
                    kind: *kind,
                    text: text.to_string(),
                    heading_level: None,
                })
                .unwrap();
        }
    }
    doc_id
}

#[test]
fn indexes_a_spell_and_a_cross_book_mention_end_to_end() {
    let mut store = Store::open_in_memory().unwrap();

    // Book 1: defines the spell "Fireball".
    insert_doc_with_blocks(
        &store,
        "Players Handbook",
        vec![vec![
            (BlockKind::Heading, "Fireball"),
            (BlockKind::Body, "3rd-level evocation"),
            (BlockKind::Body, "Casting Time: 1 action"),
            (BlockKind::Body, "Range: 150 feet"),
        ]],
    );

    // Book 2: an adventure module whose flavor text mentions Fireball.
    insert_doc_with_blocks(
        &store,
        "Curse of Strahd",
        vec![vec![
            (BlockKind::Heading, "The Wizard's Tower"),
            (BlockKind::Body, "The wizard threatens the party with a Fireball spell."),
        ]],
    );

    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    let report = index_all(&mut store, &pack).unwrap();

    assert_eq!(report.documents, 2);
    assert_eq!(report.sections, 2);
    assert!(report.chunks >= 2);
    assert!(report.entities >= 2, "expect at least the Fireball spell and the Wizard's Tower topic");

    let fireball = store.find_entity_by_slug("fireball").unwrap().expect("fireball entity should exist");
    assert_eq!(fireball.kind, EntityKind::Spell);
    assert!(fireball.centrality > 0.0, "a mentioned entity should have nonzero centrality after pagerank");

    let fields = store.list_fields(fireball.id.unwrap()).unwrap();
    assert!(fields.iter().any(|f| f.key == "Casting Time" && f.value == "1 action"));
    assert!(fields.iter().any(|f| f.key == "Range" && f.value == "150 feet"));

    // The gazetteer pass should have found "Fireball" mentioned in the
    // second book's flavor text and recorded a mention for it.
    assert!(store.mention_count(fireball.id.unwrap()).unwrap() >= 1);

    let tower = store
        .find_entity_by_slug("the-wizard-s-tower")
        .expect("lookup should not error")
        .expect("topic entity for the second heading should exist");
    assert_eq!(tower.kind, EntityKind::Topic);

    // The tower's own definition chunk mentions Fireball -> a directed
    // edge from the tower entity to the fireball entity should exist.
    let edges = store.edges_from(tower.id.unwrap(), None).unwrap();
    assert!(edges.iter().any(|e| e.dst == fireball.id.unwrap()));
}

/// Like `insert_doc_with_blocks`, but shaped like a structured (Markdown/
/// Obsidian) source: the heading carries `heading_level: Some(1)` (so
/// it's this document's primary entity — see `orchestrate.rs`'s
/// `doc_primary_entity` docs) and the document can carry front matter.
fn insert_note(store: &Store, title: &str, body_text: &str, frontmatter: Option<&str>) -> brain_core::DocId {
    let doc_id = store
        .insert_document(&Document {
            id: None,
            path: format!("/tmp/{title}.md"),
            sha256: title.to_string(),
            title: title.to_string(),
            kind: "md".to_string(),
            page_count: 1,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Plain,
            ocr: false,
            frontmatter: frontmatter.map(str::to_string),
        })
        .unwrap();
    let page_id = store
        .insert_page(&Page {
            id: None,
            doc_id,
            page_no: 1,
            width: 1.0,
            height: 1.0,
            text: String::new(),
            ocr_conf: None,
            low_confidence: false,
        })
        .unwrap();
    store
        .insert_block(&Block {
            id: None,
            page_id,
            col: 0,
            ord: 0,
            bbox: BBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 },
            kind: BlockKind::Heading,
            text: title.to_string(),
            heading_level: Some(1),
        })
        .unwrap();
    store
        .insert_block(&Block {
            id: None,
            page_id,
            col: 0,
            ord: 1,
            bbox: BBox { x0: 0.0, y0: 1.0, x1: 1.0, y1: 2.0 },
            kind: BlockKind::Body,
            text: body_text.to_string(),
            heading_level: None,
        })
        .unwrap();
    doc_id
}

#[test]
fn wikilink_between_two_notes_becomes_a_links_to_edge() {
    let mut store = Store::open_in_memory().unwrap();
    insert_note(&store, "Strahd von Zarovich", "The vampire lord of Barovia.", None);
    insert_note(&store, "Barovia", "See [[Strahd von Zarovich]] for the ruler of this land.", None);

    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    index_all(&mut store, &pack).unwrap();

    let strahd = store.find_entity_by_slug("strahd-von-zarovich").unwrap().expect("strahd entity should exist");
    let barovia = store.find_entity_by_slug("barovia").unwrap().expect("barovia entity should exist");

    let edges = store.edges_from(barovia.id.unwrap(), Some(brain_core::EdgeKind::LinksTo)).unwrap();
    assert!(
        edges.iter().any(|e| e.dst == strahd.id.unwrap()),
        "Barovia's [[wikilink]] to Strahd should produce a LinksTo edge"
    );
}

#[test]
fn inline_tag_and_frontmatter_tag_both_link_to_the_same_tag_entity() {
    let mut store = Store::open_in_memory().unwrap();
    insert_note(&store, "Session One", "The party meets #npc-ismark for the first time.", None);
    insert_note(&store, "Ismark Kolyanovich", "A minor npc.", Some(r#"{"tags":["npc-ismark"]}"#));

    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    index_all(&mut store, &pack).unwrap();

    let tag = store.find_entity_by_slug("npc-ismark").unwrap().expect("tag entity should exist");
    assert_eq!(tag.kind, EntityKind::Other("tag".to_string()));

    let session_one = store.find_entity_by_slug("session-one").unwrap().unwrap();
    let ismark = store.find_entity_by_slug("ismark-kolyanovich").unwrap().unwrap();

    let from_session = store.edges_from(session_one.id.unwrap(), Some(brain_core::EdgeKind::Mentions)).unwrap();
    assert!(from_session.iter().any(|e| e.dst == tag.id.unwrap()), "inline #tag should link to the tag entity");

    let from_ismark = store.edges_from(ismark.id.unwrap(), Some(brain_core::EdgeKind::Mentions)).unwrap();
    assert!(from_ismark.iter().any(|e| e.dst == tag.id.unwrap()), "frontmatter tags: should link to the same tag entity");
}

#[test]
fn frontmatter_aliases_resolve_by_alternate_name() {
    let mut store = Store::open_in_memory().unwrap();
    insert_note(&store, "Strahd von Zarovich", "The vampire lord.", Some(r#"{"aliases":["Strahd"]}"#));

    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    index_all(&mut store, &pack).unwrap();

    let by_alias = store.find_entity_by_alias_norm("strahd").unwrap().expect("alias lookup should resolve");
    assert_eq!(by_alias.slug, "strahd-von-zarovich");
}

#[test]
fn reindexing_is_idempotent_and_does_not_duplicate_entities() {
    let mut store = Store::open_in_memory().unwrap();
    insert_doc_with_blocks(
        &store,
        "Players Handbook",
        vec![vec![
            (BlockKind::Heading, "Fireball"),
            (BlockKind::Body, "Casting Time: 1 action"),
        ]],
    );
    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();

    let first = index_all(&mut store, &pack).unwrap();
    let second = index_all(&mut store, &pack).unwrap();

    assert_eq!(first.entities, second.entities);
    assert_eq!(first.chunks, second.chunks);
    assert_eq!(store.list_entities(None).unwrap().len(), first.entities);
}
