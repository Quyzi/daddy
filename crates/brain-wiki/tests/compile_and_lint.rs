//! End-to-end test: builds a graph via `brain-index`'s orchestration,
//! then exercises `brain-wiki`'s `compile` (including curated-page
//! preservation) and `lint`.

use brain_core::{BBox, Block, BlockKind, Document, ExtractorKind, Page};
use brain_index::{index_all, CompiledPack, RulePack};
use brain_store::Store;
use brain_wiki::{compile, lint, PageStatus};
use chrono::Utc;
use std::fs;

const PACK_TOML: &str = r#"
[[entity]]
kind = "spell"
require = [{ regex = "Casting Time:", within_chars = 300 }]
fields = ["Casting Time", "Range"]

[[entity]]
kind = "topic"
atomic = false
"#;

fn seeded_store() -> Store {
    let mut store = Store::open_in_memory().unwrap();
    let doc = store
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
        })
        .unwrap();
    let page = store
        .insert_page(&Page { id: None, doc_id: doc, page_no: 241, width: 600.0, height: 800.0, text: String::new(), ocr_conf: None, low_confidence: false })
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
            .insert_block(&Block { id: None, page_id: page, col: 0, ord: ord as u32, bbox: BBox { x0: 0.0, y0: 0.0, x1: 1.0, y1: 1.0 }, kind, text: text.into() })
            .unwrap();
    }
    let pack = CompiledPack::compile(&RulePack::parse(PACK_TOML).unwrap()).unwrap();
    index_all(&mut store, &pack).unwrap();
    store
}

#[test]
fn compile_prefers_the_specific_kind_when_two_entities_share_a_slug() {
    // Mirrors a real Player's Handbook case: a bare spell-list table
    // entry for "Fireball" (recognized only as a generic "topic") and
    // the actual spell definition both exist in the graph, sharing the
    // slug "fireball". Only one file can exist at that path -- it must
    // be the real spell, not whichever happened to be processed last.
    let store = seeded_store();
    store
        .upsert_entity(&brain_core::Entity {
            id: None,
            kind: brain_core::EntityKind::Topic,
            name: "Fireball".into(),
            slug: "fireball".into(),
            canonical_id: None,
            centrality: 0.9, // deliberately higher than the real spell's
            confidence: 0.5,
        })
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    compile(&store, dir.path(), 0.0).unwrap();

    let content = fs::read_to_string(dir.path().join("fireball.md")).unwrap();
    assert!(content.contains("kind: spell"), "the specific spell entity must win over the topic duplicate:\n{content}");
    assert!(content.contains("Casting Time: 1 action"));
}

#[test]
fn compile_dedups_before_filtering_by_centrality_not_after() {
    // The topic duplicate clears a min-centrality threshold that the
    // real spell entity does not. Filtering before dedup would keep
    // exactly the wrong one; deduping first must let the specific
    // entity win regardless, even though it then gets dropped by the
    // threshold entirely if it alone doesn't clear it -- what matters
    // here is that the topic duplicate never silently takes its place.
    let store = seeded_store();
    store
        .upsert_entity(&brain_core::Entity {
            id: None,
            kind: brain_core::EntityKind::Topic,
            name: "Fireball".into(),
            slug: "fireball".into(),
            canonical_id: None,
            centrality: 0.9,
            confidence: 0.5,
        })
        .unwrap();
    // The real spell entity's centrality (set during indexing) is far
    // below 0.9; a threshold between the two must not resurrect the topic.
    let dir = tempfile::tempdir().unwrap();
    compile(&store, dir.path(), 0.5).unwrap();

    let path = dir.path().join("fireball.md");
    if path.exists() {
        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("kind: topic"), "the topic duplicate must never win by riding a threshold the real entity misses:\n{content}");
    }
}

#[test]
fn compile_writes_a_cited_page_and_an_index() {
    let store = seeded_store();
    let dir = tempfile::tempdir().unwrap();

    let report = compile(&store, dir.path(), 0.0).unwrap();
    assert!(report.written >= 1);

    let fireball_path = dir.path().join("fireball.md");
    assert!(fireball_path.exists());
    let content = fs::read_to_string(&fireball_path).unwrap();
    assert!(content.starts_with("---\n"));
    assert!(content.contains("status: generated"));
    assert!(content.contains("[Source: Player's Handbook p.241]"));
    assert!(content.contains("Casting Time: 1 action"));

    let index = fs::read_to_string(dir.path().join("index.md")).unwrap();
    assert!(index.contains("[[fireball]]"));

    let log = fs::read_to_string(dir.path().join("log.md")).unwrap();
    assert!(log.contains("compile |"));
}

#[test]
fn compile_never_overwrites_a_curated_page() {
    let store = seeded_store();
    let dir = tempfile::tempdir().unwrap();
    compile(&store, dir.path(), 0.0).unwrap();

    let fireball_path = dir.path().join("fireball.md");
    let hand_edited = "---\ntitle: Fireball\nstatus: curated\n---\n\nMy own carefully written notes.\n";
    fs::write(&fireball_path, hand_edited).unwrap();
    assert_eq!(brain_wiki::read_status(&fireball_path), PageStatus::Curated);

    let report = compile(&store, dir.path(), 0.0).unwrap();
    assert_eq!(report.skipped_curated, 1);
    let content = fs::read_to_string(&fireball_path).unwrap();
    assert_eq!(content, hand_edited, "a curated page must be left byte-for-byte alone");
}

#[test]
fn lint_reports_no_contradictions_for_a_clean_single_source_graph() {
    let store = seeded_store();
    let report = lint(&store).unwrap();
    assert!(report.contradictions.is_empty());
}
