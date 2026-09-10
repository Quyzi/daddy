//! Golden tests against real word data extracted from three
//! representative pages of the dnd-5e corpus (see the implementation
//! plan's Phase 3 exit criteria). Fixtures live in `tests/fixtures/` as
//! JSON dumps of `RawPage` — see
//! `crates/brain-extract/examples/dump_fixture.rs` for how they were
//! generated. No PDFs or external tools are involved in running these.

use brain_core::{BlockKind, RawPage};
use brain_layout::{layout_page, LaidOutBlock, LaidOutPage};
use std::fs;

fn load(name: &str) -> RawPage {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    let json = fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading fixture {path}: {e}"));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("parsing fixture {path}: {e}"))
}

fn heading<'a>(page: &'a LaidOutPage, text: &str) -> Option<&'a LaidOutBlock> {
    page.blocks.iter().find(|b| b.kind == BlockKind::Heading && b.text == text)
}

/// PHB p239: a two-column, OCR-fragmented spell reference page. This is
/// the plan's primary golden case — word-fragment rejoin ("M in o r
/// I l l u s io n" -> "Minor Illusion") and column-major reading order
/// both have to work for this to pass.
#[test]
fn phb_p239_rejoins_words_and_detects_spell_headings_in_order() {
    let raw = load("phb_p239.json");
    let page = layout_page(&raw);

    assert!(page.blocks.iter().any(|b| b.col == 0), "expected column 0");
    assert!(page.blocks.iter().any(|b| b.col == 1), "expected column 1 (this page is two-column)");

    let minor_illusion = heading(&page, "Minor Illusion").expect("\"Minor Illusion\" should be a detected heading");
    let mirror_image = heading(&page, "Mirror Image").expect("\"Mirror Image\" should be a detected heading");
    assert_eq!(minor_illusion.col, 0, "Minor Illusion is in the left column");
    assert_eq!(mirror_image.col, 1, "Mirror Image is in the right column");
    assert!(
        minor_illusion.ord < mirror_image.ord,
        "column-major reading order must visit all of column 0 before column 1"
    );

    let text = page.reading_order_text();
    assert!(
        text.contains("its emotions or read its thoughts"),
        "OCR-fragmented \"em otion s\" must be rejoined into \"emotions\", got: {text:?}"
    );
    assert!(!text.contains("em otion"), "must not leave a half-rejoined fragment behind");
}

/// ToB2 p21: a monster stat block sharing a page with narrative flavor
/// text — the layout that defeated naive column-major ordering during
/// development (see the implementation notes in `columns.rs`).
#[test]
fn tob2_p21_detects_monster_and_action_headings_with_intact_stat_fields() {
    let raw = load("tob2_p21.json");
    let page = layout_page(&raw);

    assert!(page.blocks.iter().any(|b| b.col == 1), "this page is two-column");
    heading(&page, "URIDIMMU").expect("the monster's all-caps name should be a detected heading");
    heading(&page, "ACTIONS").expect("the ACTIONS sub-heading should be a detected heading");

    let text = page.reading_order_text();
    assert!(text.contains("Armor Class 18"), "stat block fields must survive intact, got: {text:?}");
    assert!(text.contains("Hit Points 150 (12d10 + 84)"), "stat block fields must survive intact, got: {text:?}");
}

/// Designers & Dragons '70s p20: a clean, native (non-OCR) single-column
/// narrative page. This is the negative case for both word-rejoin (must
/// NOT merge real word spacing) and column detection (a running "title |
/// page number" header must not be mistaken for two body columns) — see
/// the implementation notes in `rejoin.rs` and `columns.rs` for the bugs
/// this guards against.
#[test]
fn dd70s_p20_stays_single_column_and_leaves_native_text_untouched() {
    let raw = load("dd70s_p20.json");
    let page = layout_page(&raw);

    assert!(
        page.blocks.iter().all(|b| b.col == 0),
        "a single-column narrative page must not be split into fake columns"
    );

    let text = page.reading_order_text();
    assert!(
        text.contains("This was thanks to Don Lowry, an ex-Air Force Captain who formed Lowrys"),
        "native PDF text must keep its real word spacing, got: {text:?}"
    );
}
