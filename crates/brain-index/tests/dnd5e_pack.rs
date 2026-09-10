//! Validates the real `packs/dnd5e.toml` rule pack against real corpus
//! text — the same golden fixtures `brain-layout` uses (word data
//! extracted from actual PDF pages), run all the way through layout,
//! section-building, and recognition.

use brain_core::{BlockId, PageId, RawPage};
use brain_index::{recognize, CompiledPack};
use std::fs;
use std::path::Path;

fn load_fixture(name: &str) -> RawPage {
    let path = format!(
        "{}/../brain-layout/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let json = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    serde_json::from_str(&json).unwrap()
}

fn build_sections_from_fixture(name: &str, pack: &CompiledPack) -> Vec<brain_index::BuiltSection> {
    let raw = load_fixture(name);
    let laid_out = brain_layout::layout_page(&raw);
    let blocks: Vec<(u32, brain_core::Block)> = laid_out
        .blocks
        .iter()
        .map(|b| {
            (
                laid_out.page_no,
                brain_core::Block {
                    id: Some(BlockId::new(1)),
                    page_id: PageId::new(1),
                    col: b.col,
                    ord: b.ord,
                    bbox: b.bbox,
                    kind: b.kind,
                    text: b.text.clone(),
                    heading_level: None,
                },
            )
        })
        .collect();
    brain_index::build_sections(&blocks, &pack.all_field_labels())
}

fn dnd5e_pack() -> CompiledPack {
    CompiledPack::load_with_fallback(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs").as_path(), "dnd5e")
        .expect("packs/dnd5e.toml should load and compile")
}

#[test]
fn recognizes_minor_illusion_and_mirror_image_as_spells_with_fields() {
    let pack = dnd5e_pack();
    let sections = build_sections_from_fixture("phb_p239.json", &pack);

    let minor_illusion = sections
        .iter()
        .find(|s| s.title == "Minor Illusion")
        .expect("Minor Illusion should be a built section");
    let r = recognize(minor_illusion, &pack).expect("should recognize as an entity");
    assert_eq!(r.kind, "spell");
    assert!(r.fields.iter().any(|(k, v)| k == "Casting Time" && v == "1 action"));
    assert!(r.fields.iter().any(|(k, v)| k == "Range" && v == "30 feet"));

    let mirror_image = sections
        .iter()
        .find(|s| s.title == "Mirror Image")
        .expect("Mirror Image should be a built section");
    let r = recognize(mirror_image, &pack).expect("should recognize as an entity");
    assert_eq!(r.kind, "spell");
}

#[test]
fn recognizes_uridimmu_as_a_monster_with_stat_fields() {
    let pack = dnd5e_pack();
    let sections = build_sections_from_fixture("tob2_p21.json", &pack);

    let uridimmu = sections
        .iter()
        .find(|s| s.title == "URIDIMMU")
        .expect("URIDIMMU should be a built section");
    let r = recognize(uridimmu, &pack).expect("should recognize as an entity");
    assert_eq!(r.kind, "monster");
    assert!(r.fields.iter().any(|(k, v)| k == "Armor Class" && v.contains("18")));
    assert!(r.fields.iter().any(|(k, v)| k == "Hit Points" && v.contains("150")));
}

#[test]
fn recognizes_a_datasource_row_as_an_npc_with_fields() {
    // Exactly the block shape `brain-cli`'s `write_datasource_query`
    // produces for one SQL row: a heading (the row's name) followed by
    // "{Column}: {value}" body blocks -- no PDF/layout fixture involved,
    // proving the pack needs no datasource-specific indexing code at all.
    let pack = dnd5e_pack();
    let blocks: Vec<(u32, brain_core::Block)> = vec![
        (1, heading_block("Ismark Kolyanovich", 0)),
        (1, body_block("Race: Human", 1)),
        (1, body_block("Location: Vallaki", 2)),
        (1, body_block("Notes: Son of the burgomaster.", 3)),
    ];
    let sections = brain_index::build_sections(&blocks, &pack.all_field_labels());
    let npc = sections
        .iter()
        .find(|s| s.title == "Ismark Kolyanovich")
        .expect("the row's heading should be its own section");
    let r = recognize(npc, &pack).expect("should recognize as an entity");
    assert_eq!(r.kind, "npc");
    assert!(r.fields.iter().any(|(k, v)| k == "Race" && v == "Human"));
    assert!(r.fields.iter().any(|(k, v)| k == "Location" && v == "Vallaki"));
}

fn heading_block(text: &str, ord: u32) -> brain_core::Block {
    brain_core::Block {
        id: Some(BlockId::new(1)),
        page_id: PageId::new(1),
        col: 0,
        ord,
        bbox: brain_core::BBox { x0: 0.0, y0: ord as f64, x1: 1.0, y1: ord as f64 + 1.0 },
        kind: brain_core::BlockKind::Heading,
        text: text.to_string(),
        heading_level: Some(1),
    }
}

fn body_block(text: &str, ord: u32) -> brain_core::Block {
    brain_core::Block { kind: brain_core::BlockKind::Body, heading_level: None, ..heading_block(text, ord) }
}

#[test]
fn non_entity_headings_fall_back_to_generic_topic() {
    let pack = dnd5e_pack();
    let sections = build_sections_from_fixture("dd70s_p20.json", &pack);
    for section in &sections {
        let r = recognize(section, &pack).expect("generic fallback should always match something");
        assert_eq!(r.kind, "topic", "a narrative history book heading should fall through to the generic pack");
    }
}
