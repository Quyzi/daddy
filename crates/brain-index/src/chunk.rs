//! Turns a [`BuiltSection`] into one or more indexable chunks, applying a
//! rule pack's recognizers to decide whether the section is a structured
//! entity definition (atomic — never split, regardless of length) or
//! generic prose (split at paragraph boundaries once it gets long).

use crate::pack::CompiledPack;
use crate::sections::BuiltSection;
use brain_core::ChunkKind;

/// Sections longer than this (in bytes of UTF-8 text) get split into
/// multiple prose chunks at paragraph boundaries. Roughly 1,200 tokens
/// at ~4 bytes/token, matching the implementation plan's chunk-size
/// target.
pub const MAX_PROSE_CHUNK_CHARS: usize = 4800;

/// What a rule pack recognized a section as.
pub struct Recognized {
    /// Entity kind string (e.g. `"spell"`, `"monster"`, `"topic"`).
    pub kind: String,
    /// Extracted `(field name, value)` pairs.
    pub fields: Vec<(String, String)>,
    /// Whether this kind must stay a single, never-split chunk.
    pub atomic: bool,
}

/// Tries every rule in `pack`, in order, returning the first whose
/// `require` patterns all match within their configured prefix window of
/// the section's body text. An empty `require` list always matches,
/// which is how a catch-all rule (see `packs/generic.toml`) works.
///
/// Patterns are matched against the flattened body text, not anchored to
/// line starts: `brain-layout` joins several originally-separate short
/// lines into one block whenever they don't individually score as
/// headings (a spell's "Casting Time:"/"Range:"/... lines commonly do
/// this), so a field or phrase can legitimately land mid-block rather
/// than at position 0.
pub fn recognize(section: &BuiltSection, pack: &CompiledPack) -> Option<Recognized> {
    let body = section.body_text();
    'rule: for rule in &pack.rules {
        for req in &rule.requires {
            let window = char_safe_prefix(&body, req.within_chars);
            if !req.regex.is_match(window) {
                continue 'rule;
            }
        }
        let fields = extract_fields(&body, &rule.fields);
        return Some(Recognized { kind: rule.kind.clone(), fields, atomic: rule.atomic });
    }
    None
}

/// Extracts `(field name, value)` pairs from flattened body text. A
/// field's value runs from right after its label to whichever comes
/// first: another field's label, a paragraph break, or the end of the
/// text — which correctly bounds a value even when two labels ended up
/// merged into the same block (e.g. `"Large celestial, lawful good Armor
/// Class 18 (natural armor)"`, where naive prefix matching would only
/// ever find whichever field happens to start the block).
fn extract_fields(body: &str, field_names: &[String]) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    for field in field_names {
        let Some(label_start) = find_label(body, field) else { continue };
        let after = &body[label_start + field.len()..];
        let after = after.trim_start().trim_start_matches(':').trim_start();
        let mut cutoff = after.len();
        if let Some(pos) = after.find("\n\n") {
            cutoff = cutoff.min(pos);
        }
        for other in field_names {
            if other == field {
                continue;
            }
            if let Some(pos) = find_label(after, other) {
                cutoff = cutoff.min(pos);
            }
        }
        let value = after[..cutoff].trim();
        if !value.is_empty() {
            fields.push((field.clone(), value.to_string()));
        }
    }
    fields
}

/// Finds `label` in `text` as a whole-word occurrence (the character
/// immediately before it, if any, isn't itself alphanumeric) — enough to
/// stop e.g. `"Skills"` from matching inside some unrelated longer word,
/// without the complexity of a full regex per field.
fn find_label(text: &str, label: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut search_from = 0;
    while let Some(rel) = text[search_from..].find(label) {
        let pos = search_from + rel;
        let boundary_ok = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
        if boundary_ok {
            return Some(pos);
        }
        search_from = pos + 1;
    }
    None
}

/// Slices `s` to at most `max_bytes`, backing off to the nearest earlier
/// UTF-8 character boundary so this never panics on multi-byte text.
fn char_safe_prefix(s: &str, max_bytes: usize) -> &str {
    if max_bytes >= s.len() {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// One chunk built from a section, ready for [`brain_store::Store::insert_chunk`].
pub struct BuiltChunk {
    /// `Definition` for an atomic recognized entity, `Prose` otherwise.
    pub kind: ChunkKind,
    /// The chunk's text (title-prefixed on the first chunk of a section).
    pub text: String,
}

/// Builds the chunk(s) for one section. `recognized` should be the result
/// of [`recognize`] for this same section (computed once, passed in
/// rather than recomputed, since callers also need its fields/kind for
/// entity bookkeeping).
pub fn build_chunks(section: &BuiltSection, recognized: Option<&Recognized>) -> Vec<BuiltChunk> {
    let full_text = format!("{}\n\n{}", section.title, section.body_text());
    let atomic = recognized.map(|r| r.atomic).unwrap_or(false);

    if atomic || full_text.len() <= MAX_PROSE_CHUNK_CHARS {
        let kind = if atomic { ChunkKind::Definition } else { ChunkKind::Prose };
        return vec![BuiltChunk { kind, text: full_text }];
    }

    // Split at paragraph (body-block) boundaries, keeping each chunk
    // under the size budget. The section title is prefixed onto every
    // resulting chunk so each stays independently citable/searchable
    // without losing "what is this the middle of" context.
    let mut chunks = Vec::new();
    let mut current = String::new();
    for (_, block_text) in &section.body_blocks {
        if !current.is_empty() && current.len() + block_text.len() + 2 > MAX_PROSE_CHUNK_CHARS {
            chunks.push(finish_prose_chunk(&section.title, std::mem::take(&mut current)));
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(block_text);
    }
    if !current.is_empty() {
        chunks.push(finish_prose_chunk(&section.title, current));
    }
    if chunks.is_empty() {
        // A heading with no body at all: still index the heading text
        // itself so it's findable.
        chunks.push(BuiltChunk { kind: ChunkKind::Prose, text: section.title.clone() });
    }
    chunks
}

fn finish_prose_chunk(title: &str, body: String) -> BuiltChunk {
    BuiltChunk { kind: ChunkKind::Prose, text: format!("{title}\n\n{body}") }
}

/// Produces a nicer display name from a heading: title-cases an ALL-CAPS
/// heading (`"URIDIMMU"` -> `"Uridimmu"`), and leaves anything already
/// mixed-case (`"Minor Illusion"`) untouched.
pub fn display_name(title: &str) -> String {
    let has_alpha = title.chars().any(|c| c.is_alphabetic());
    let all_caps = has_alpha && title.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase());
    if !all_caps {
        return title.trim().to_string();
    }
    title
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{CompiledPack, RulePack};

    fn section(title: &str, body_blocks: Vec<&str>) -> BuiltSection {
        BuiltSection {
            title: title.to_string(),
            slug: brain_core::slugify(title),
            start_page: 1,
            end_page: 1,
            body_blocks: body_blocks.into_iter().map(|s| (1, s.to_string())).collect(),
            level: 1,
            parent_index: None,
        }
    }

    fn spell_pack() -> CompiledPack {
        let toml = r#"
[[entity]]
kind = "spell"
require = [{ regex = "Casting Time:", within_chars = 200 }]
fields = ["Casting Time", "Range"]

[[entity]]
kind = "topic"
atomic = false
"#;
        CompiledPack::compile(&RulePack::parse(toml).unwrap()).unwrap()
    }

    #[test]
    fn recognizes_a_spell_and_extracts_fields() {
        let pack = spell_pack();
        let s = section("Fireball", vec!["3rd-level evocation", "Casting Time: 1 action", "Range: 150 feet"]);
        let r = recognize(&s, &pack).unwrap();
        assert_eq!(r.kind, "spell");
        assert!(r.atomic);
        assert!(r.fields.contains(&("Casting Time".to_string(), "1 action".to_string())));
        assert!(r.fields.contains(&("Range".to_string(), "150 feet".to_string())));
    }

    #[test]
    fn falls_back_to_generic_topic_when_no_specific_rule_matches() {
        let pack = spell_pack();
        let s = section("Random Chapter Heading", vec!["just some narrative text"]);
        let r = recognize(&s, &pack).unwrap();
        assert_eq!(r.kind, "topic");
        assert!(!r.atomic);
    }

    #[test]
    fn atomic_entities_never_split_regardless_of_length() {
        let pack = spell_pack();
        let padding = "padding text ".repeat(50);
        let long_body: Vec<&str> = std::iter::once("Casting Time: 1 action")
            .chain(std::iter::repeat_n(padding.as_str(), 5))
            .collect();
        let s = section("Fireball", long_body);
        let recognized = recognize(&s, &pack).unwrap();
        let chunks = build_chunks(&s, Some(&recognized));
        assert_eq!(chunks.len(), 1, "an atomic entity must stay one chunk no matter how long");
        assert_eq!(chunks[0].kind, ChunkKind::Definition);
    }

    #[test]
    fn long_generic_sections_split_at_paragraph_boundaries() {
        let big_paragraph = "word ".repeat(1000); // ~5000 bytes, over the 4800 budget
        let s = section("Chapter One", vec![&big_paragraph, "a second paragraph"]);
        let chunks = build_chunks(&s, None);
        assert!(chunks.len() >= 2, "an oversized non-atomic section must split");
        assert!(chunks.iter().all(|c| c.kind == ChunkKind::Prose));
        assert!(chunks[0].text.starts_with("Chapter One"));
    }

    #[test]
    fn display_name_titlecases_allcaps_but_leaves_mixed_case_alone() {
        assert_eq!(display_name("URIDIMMU"), "Uridimmu");
        assert_eq!(display_name("Minor Illusion"), "Minor Illusion");
        assert_eq!(display_name("MEAD ARCHON"), "Mead Archon");
    }
}
