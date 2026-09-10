//! Builds sections from a document's stored blocks: every
//! [`BlockKind::Heading`] starts a new section that runs until the next
//! heading (or end of document). Body text before the first heading is
//! dropped as front-matter — a reasonable v1 simplification, since it's
//! never itself a citable, nameable unit.
//!
//! Sections nest when [`Block::heading_level`] is known: a heading with a
//! real depth (Markdown/AsciiDoc/HTML) closes any open section at the
//! same or deeper level and becomes a child of whichever shallower
//! section is still open, using a plain heading-depth stack — the same
//! algorithm a table-of-contents builder uses. A PDF's headings never
//! carry a depth (see that field's docs for why), so every one of them
//! is `level 1` and the stack empties after each — reproducing today's
//! flat behavior exactly, with no special-casing needed.

use brain_core::{slugify, Block, BlockKind};

/// One heading-anchored section, with its body kept as a list of block
/// texts (rather than pre-flattened) so [`crate::chunk::recognize`] can
/// match rule-pack field patterns against individual blocks — this is
/// what lets a stat block's "Armor Class 18" and "Hit Points 150 (...)"
/// lines (each its own block — see `brain-layout`'s heading-detection
/// notes on why short field-like lines often get isolated that way) be
/// pulled out as structured fields with a simple prefix match.
#[derive(Debug, Clone)]
pub struct BuiltSection {
    /// The section's heading text.
    pub title: String,
    /// URL/filename-safe slug derived from `title`.
    pub slug: String,
    /// First page of the section.
    pub start_page: u32,
    /// Last page of the section (inclusive).
    pub end_page: u32,
    /// Body blocks as `(page_no, text)`, in reading order. Does not
    /// include the heading block itself.
    pub body_blocks: Vec<(u32, String)>,
    /// Heading depth: `1` for a PDF's (always-flat) sections and for a
    /// structured source's top-level heading, `2`+ for a nested one. See
    /// this module's docs for how this is derived from
    /// [`Block::heading_level`].
    pub level: u32,
    /// Index, into the same `Vec<BuiltSection>` this section is part of,
    /// of this section's parent — `None` for a top-level section. Always
    /// an *earlier* index (a parent heading is always encountered before
    /// its children), so callers assigning real ids in order (see
    /// `brain_index::orchestrate::index_all`) can resolve it against ids
    /// they've already assigned by the time they reach this section.
    pub parent_index: Option<usize>,
}

impl BuiltSection {
    /// The section's body, flattened to one string (paragraphs joined by
    /// a blank line) — what gets stored as chunk text.
    pub fn body_text(&self) -> String {
        self.body_blocks.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n")
    }
}

/// Builds sections from a document's blocks, given in full
/// document-reading order (see
/// [`brain_store::Store::list_blocks_for_document`]). `known_field_labels`
/// should be the active rule pack's [`crate::pack::CompiledPack::all_field_labels`]
/// — see this module's private `looks_like_field_continuation` for why
/// sections need to know them.
pub fn build_sections(ordered_blocks: &[(u32, Block)], known_field_labels: &[String]) -> Vec<BuiltSection> {
    let mut sections = Vec::new();
    let mut current: Option<BuiltSection> = None;
    // Heading-depth stack: `(index into `sections`, that section's
    // level)`. A new heading at level `L` closes every open section at
    // level `>= L` (they can't be its ancestor) and becomes a child of
    // whatever's left on top, if anything.
    let mut stack: Vec<(usize, u32)> = Vec::new();

    for (page_no, block) in ordered_blocks {
        if block.kind == BlockKind::Chrome {
            continue;
        }
        let starts_new_section = block.kind == BlockKind::Heading
            && !(current.is_some() && looks_like_field_continuation(&block.text, known_field_labels));
        if starts_new_section {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            let level = block.heading_level.map(u32::from).unwrap_or(1);
            while matches!(stack.last(), Some(&(_, top_level)) if top_level >= level) {
                stack.pop();
            }
            let parent_index = stack.last().map(|&(idx, _)| idx);
            // This section hasn't been pushed to `sections` yet — it
            // will occupy the next free index once it is, whether that
            // happens at the next heading or at the final flush below.
            stack.push((sections.len(), level));
            current = Some(BuiltSection {
                title: block.text.clone(),
                slug: slugify(&block.text),
                start_page: *page_no,
                end_page: *page_no,
                body_blocks: Vec::new(),
                level,
                parent_index,
            });
        } else if let Some(section) = current.as_mut() {
            section.end_page = *page_no;
            section.body_blocks.push((*page_no, block.text.clone()));
        }
        // A body block before any heading has been seen is dropped.
    }
    if let Some(section) = current.take() {
        sections.push(section);
    }
    sections
}

/// Decides whether a heading-classified line is really a structured
/// field value, not the start of a new section. Two independent checks:
///
/// 1. **Colon-labeled** (pack-agnostic): a short label immediately
///    followed by a colon, e.g. `"Range: Touch"` or `"Duration:
///    Instantaneous"`.
/// 2. **Known field label** (needs `known_field_labels`, the active rule
///    pack's declared field names): a line starting with one of them
///    even with *no* colon, e.g. `"Armor Class 18"` or `"Hit Points 150
///    (12d10 + 84)"`.
///
/// Both exist because `brain-layout`'s heading detector has no font-size
/// signal to work from (see its module docs), so a short field-value
/// line commonly scores as its own heading — which, left unchecked,
/// fragments a spell's or monster's fields into their own one-line
/// "sections" instead of leaving them attached to the entity they
/// describe. Check 1 alone was not enough: confirmed against a real
/// 57-book corpus run during development, colon-free stat-block fields
/// (D&D monsters state "Armor Class 18", never "Armor Class: 18") were
/// still routinely fragmenting a monster's own name away from its stats,
/// undercounting recognized monsters by nearly half.
fn looks_like_field_continuation(text: &str, known_field_labels: &[String]) -> bool {
    if let Some(colon_pos) = text.find(':') {
        if colon_pos > 0 && colon_pos <= 30 {
            let label = &text[..colon_pos];
            if label.chars().any(|c| c.is_alphabetic()) && !label.contains('.') {
                return true;
            }
        }
    }
    known_field_labels.iter().any(|label| {
        text.strip_prefix(label.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{BBox, BlockId, PageId};

    fn block(kind: BlockKind, text: &str, ord: u32) -> Block {
        Block {
            id: Some(BlockId::new(1)),
            page_id: PageId::new(1),
            col: 0,
            ord,
            bbox: BBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 },
            kind,
            text: text.to_string(),
            heading_level: None,
        }
    }

    fn heading_at(text: &str, ord: u32, level: u8) -> Block {
        Block { heading_level: Some(level), ..block(BlockKind::Heading, text, ord) }
    }

    #[test]
    fn nested_headings_get_the_right_parent_and_level() {
        // Title (1) > Section (2) > Sub (3), then a sibling Section (2)
        // back out at the same depth as the first.
        let blocks = vec![
            (1, heading_at("Title", 0, 1)),
            (1, block(BlockKind::Body, "intro", 1)),
            (1, heading_at("Section", 2, 2)),
            (1, block(BlockKind::Body, "section body", 3)),
            (1, heading_at("Sub", 4, 3)),
            (1, block(BlockKind::Body, "sub body", 5)),
            (1, heading_at("Section Two", 6, 2)),
        ];
        let sections = build_sections(&blocks, &[]);
        assert_eq!(sections.len(), 4);

        assert_eq!(sections[0].title, "Title");
        assert_eq!(sections[0].level, 1);
        assert_eq!(sections[0].parent_index, None);

        assert_eq!(sections[1].title, "Section");
        assert_eq!(sections[1].level, 2);
        assert_eq!(sections[1].parent_index, Some(0));

        assert_eq!(sections[2].title, "Sub");
        assert_eq!(sections[2].level, 3);
        assert_eq!(sections[2].parent_index, Some(1));

        // Back out to level 2: parents at level >= 2 (both "Section" and
        // "Sub") close, leaving "Title" (level 1) as the parent again.
        assert_eq!(sections[3].title, "Section Two");
        assert_eq!(sections[3].level, 2);
        assert_eq!(sections[3].parent_index, Some(0));
    }

    #[test]
    fn pdf_style_headings_with_no_known_level_stay_flat() {
        let blocks = vec![
            (1, block(BlockKind::Heading, "Fireball", 0)),
            (1, block(BlockKind::Body, "body", 1)),
            (1, block(BlockKind::Heading, "Shield", 2)),
        ];
        let sections = build_sections(&blocks, &[]);
        assert!(sections.iter().all(|s| s.level == 1 && s.parent_index.is_none()));
    }

    #[test]
    fn splits_on_headings_and_tracks_page_span() {
        let blocks = vec![
            (1, block(BlockKind::Body, "front matter, dropped", 0)),
            (1, block(BlockKind::Heading, "Fireball", 1)),
            (1, block(BlockKind::Body, "3rd-level evocation", 2)),
            (2, block(BlockKind::Body, "A bright streak flashes...", 0)),
            (2, block(BlockKind::Heading, "Fire Shield", 1)),
            (2, block(BlockKind::Body, "A thin veil of fire...", 2)),
        ];
        let sections = build_sections(&blocks, &[]);
        assert_eq!(sections.len(), 2);

        assert_eq!(sections[0].title, "Fireball");
        assert_eq!(sections[0].start_page, 1);
        assert_eq!(sections[0].end_page, 2);
        assert_eq!(sections[0].body_blocks.len(), 2);
        assert!(sections[0].body_text().contains("3rd-level evocation"));
        assert!(sections[0].body_text().contains("A bright streak"));

        assert_eq!(sections[1].title, "Fire Shield");
        assert_eq!(sections[1].start_page, 2);
        assert_eq!(sections[1].end_page, 2);
    }

    #[test]
    fn field_like_headings_stay_attached_to_the_open_section() {
        // Mirrors the real Player's Handbook layout bug: "Range:" and
        // "Duration:" lines misclassified as Heading-kind blocks must
        // not fragment Fireball into orphaned one-line sections.
        let blocks = vec![
            (241, block(BlockKind::Heading, "Fireball", 0)),
            (241, block(BlockKind::Body, "3rd-level evocation", 1)),
            (241, block(BlockKind::Heading, "Casting Time: 1 action", 2)),
            (241, block(BlockKind::Heading, "Range: 150 feet", 3)),
            (241, block(BlockKind::Body, "A bright streak flashes...", 4)),
            (241, block(BlockKind::Heading, "Duration: Instantaneous", 5)),
            (241, block(BlockKind::Heading, "Shield", 6)),
        ];
        let sections = build_sections(&blocks, &[]);
        assert_eq!(sections.len(), 2, "field lines must not create their own sections");
        assert_eq!(sections[0].title, "Fireball");
        let body = sections[0].body_text();
        assert!(body.contains("Casting Time: 1 action"));
        assert!(body.contains("Range: 150 feet"));
        assert!(body.contains("Duration: Instantaneous"));
        assert!(body.contains("A bright streak"));
        assert_eq!(sections[1].title, "Shield");
    }

    #[test]
    fn colon_free_field_lines_stay_attached_given_known_field_labels() {
        // Mirrors the real corpus bug: D&D stat blocks state "Armor Class
        // 18", never "Armor Class: 18" -- no colon, so this needs the
        // pack's declared field labels to recognize as a continuation.
        // Without this, a monster's own name section ends up empty and
        // its fields land in an orphaned "Armor Class 18" section
        // instead, which is exactly what made monster recognition
        // undercount by nearly half against the full 57-book corpus.
        let blocks = vec![
            (21, block(BlockKind::Heading, "URIDIMMU", 0)),
            (21, block(BlockKind::Body, "Large celestial, lawful good", 1)),
            (21, block(BlockKind::Heading, "Armor Class 18 (natural armor)", 2)),
            (21, block(BlockKind::Heading, "Hit Points 150 (12d10 + 84)", 3)),
            (21, block(BlockKind::Body, "Speed 30 ft., fly 90 ft.", 4)),
            (21, block(BlockKind::Heading, "ACTIONS", 5)),
        ];
        let known_fields = vec!["Armor Class".to_string(), "Hit Points".to_string()];
        let sections = build_sections(&blocks, &known_fields);
        assert_eq!(sections.len(), 2, "colon-free field lines must not create their own sections");
        assert_eq!(sections[0].title, "URIDIMMU");
        let body = sections[0].body_text();
        assert!(body.contains("Armor Class 18 (natural armor)"));
        assert!(body.contains("Hit Points 150 (12d10 + 84)"));
        assert_eq!(sections[1].title, "ACTIONS");
    }

    #[test]
    fn chrome_blocks_are_skipped_entirely() {
        let blocks = vec![
            (1, block(BlockKind::Chrome, "Player's Handbook 239", 0)),
            (1, block(BlockKind::Heading, "Minor Illusion", 1)),
            (1, block(BlockKind::Body, "Illusion cantrip", 2)),
            (1, block(BlockKind::Chrome, "239", 3)),
        ];
        let sections = build_sections(&blocks, &[]);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].body_blocks.len(), 1);
    }

    #[test]
    fn document_with_no_headings_produces_no_sections() {
        let blocks = vec![(1, block(BlockKind::Body, "just prose, no heading ever", 0))];
        assert!(build_sections(&blocks, &[]).is_empty());
    }
}
