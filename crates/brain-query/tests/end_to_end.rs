//! End-to-end test: builds a small graph via `brain-index`'s
//! orchestration (the same path `brain index` uses), then exercises
//! `brain-query`'s `explore` and `get_entity` against it.

use brain_core::{BBox, Block, BlockKind, Document, ExtractorKind, Page};
use brain_index::{index_all, CompiledPack, RulePack};
use brain_query::{explore, get_entity, ExploreOptions};
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

fn seed_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();

    let phb = store
        .insert_document(&Document {
            id: None,
            path: "/tmp/phb.pdf".into(),
            sha256: "phb".into(),
            title: "Player's Handbook".into(),
            kind: "pdf".into(),
            page_count: 1,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Poppler,
            ocr: false,
            frontmatter: None,
        })
        .unwrap();
    let phb_page = store
        .insert_page(&Page { id: None, doc_id: phb, page_no: 241, width: 600.0, height: 800.0, text: String::new(), ocr_conf: None, low_confidence: false })
        .unwrap();
    for (ord, (kind, text)) in [
        (BlockKind::Heading, "Fireball"),
        (BlockKind::Body, "3rd-level evocation"),
        (BlockKind::Body, "Casting Time: 1 action"),
        (BlockKind::Body, "Range: 150 feet"),
    ]
    .into_iter()
    .enumerate()
    {
        store
            .insert_block(&Block { id: None, page_id: phb_page, col: 0, ord: ord as u32, bbox: BBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }, kind, text: text.into(), heading_level: None })
            .unwrap();
    }

    let strahd = store
        .insert_document(&Document {
            id: None,
            path: "/tmp/strahd.pdf".into(),
            sha256: "strahd".into(),
            title: "Curse of Strahd".into(),
            kind: "pdf".into(),
            page_count: 1,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Poppler,
            ocr: false,
            frontmatter: None,
        })
        .unwrap();
    let strahd_page = store
        .insert_page(&Page { id: None, doc_id: strahd, page_no: 88, width: 600.0, height: 800.0, text: String::new(), ocr_conf: None, low_confidence: false })
        .unwrap();
    for (ord, (kind, text)) in [
        (BlockKind::Heading, "The Wizard of Wines"),
        (BlockKind::Body, "The resident wizard threatens intruders with a Fireball spell if provoked."),
    ]
    .into_iter()
    .enumerate()
    {
        store
            .insert_block(&Block { id: None, page_id: strahd_page, col: 0, ord: ord as u32, bbox: BBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }, kind, text: text.into(), heading_level: None })
            .unwrap();
    }

    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    index_all(&mut store, &pack).unwrap();
    store
}

#[test]
fn explore_finds_the_spell_and_cites_its_source() {
    let store = seed_store();
    let result = explore(&store, "fireball", &ExploreOptions::default()).unwrap();
    assert!(!result.items.is_empty(), "expected at least one result for \"fireball\"");
    let top = &result.items[0];
    assert_eq!(top.entity_name.as_deref(), Some("Fireball"));
    assert_eq!(top.entity_kind.as_deref(), Some("spell"));
    assert_eq!(top.citation.document, "Player's Handbook");
    assert_eq!(top.citation.page, 241);
    assert!(top.text.contains("Casting Time: 1 action"));
}

#[test]
fn explore_kind_filter_excludes_other_kinds() {
    let store = seed_store();
    let opts = ExploreOptions { kind: Some("monster".to_string()), ..Default::default() };
    let result = explore(&store, "fireball", &opts).unwrap();
    assert!(result.items.iter().all(|i| i.entity_kind.as_deref() == Some("monster")));
}

#[test]
fn explore_respects_token_budget() {
    let store = seed_store();
    let opts = ExploreOptions { budget_tokens: 1, hops: 2, ..Default::default() };
    let result = explore(&store, "fireball wizard strahd", &opts).unwrap();
    assert!(result.items.len() <= 1, "a tiny budget must cap the result count");
}

#[test]
fn get_entity_returns_verbatim_definition_and_fields() {
    let store = seed_store();
    let view = get_entity(&store, "Fireball").unwrap().expect("fireball should exist");
    assert_eq!(view.kind, "spell");
    assert!(view.definition.as_deref().unwrap().contains("Casting Time: 1 action"));
    assert!(view.fields.iter().any(|f| f.key == "Casting Time" && f.value == "1 action"));
}

#[test]
fn get_entity_returns_none_for_unknown_names() {
    let store = seed_store();
    assert!(get_entity(&store, "Definitely Not A Real Spell Name").unwrap().is_none());
}
