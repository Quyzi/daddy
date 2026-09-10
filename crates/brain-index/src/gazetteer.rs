//! Whole-corpus mention scanning: builds one Aho-Corasick automaton over
//! every recognized entity's name and scans every chunk's text for
//! occurrences in a single linear pass — this is what turns "Curse of
//! Strahd mentions Strahd von Zarovich" from an O(entities × chunks)
//! string-search problem into an O(chunk text length) one, regardless of
//! how many thousands of entities the corpus has produced.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use brain_core::error::{BrainError, Result};
use brain_core::EntityId;

/// Entity names shorter than this are excluded from the gazetteer
/// entirely — a 1-2 character "entity" (most often a stray heading
/// fragment) produces almost nothing but false-positive matches inside
/// unrelated words.
const MIN_PATTERN_LEN: usize = 3;

/// One detected occurrence of an entity's name in a chunk's text.
#[derive(Debug, Clone, Copy)]
pub struct Mention {
    /// The entity whose name matched.
    pub entity_id: EntityId,
    /// Byte offset of the match's start in the chunk text.
    pub start: usize,
    /// Byte offset of the match's end (exclusive).
    pub end: usize,
}

/// A compiled whole-corpus name matcher.
pub struct Gazetteer {
    ac: AhoCorasick,
    entity_ids: Vec<EntityId>,
}

impl Gazetteer {
    /// Builds a gazetteer from `(entity_id, name)` pairs. Multiple rows
    /// per entity (aliases) are fine — whichever matches, the mention
    /// resolves back to that entity.
    pub fn build(entries: &[(EntityId, String)]) -> Result<Self> {
        let filtered: Vec<&(EntityId, String)> =
            entries.iter().filter(|(_, name)| name.chars().count() >= MIN_PATTERN_LEN).collect();
        let patterns: Vec<&str> = filtered.iter().map(|(_, name)| name.as_str()).collect();
        let entity_ids: Vec<EntityId> = filtered.iter().map(|(id, _)| *id).collect();

        let ac = AhoCorasickBuilder::new()
            .ascii_case_insensitive(true)
            .match_kind(MatchKind::LeftmostLongest)
            .build(&patterns)
            .map_err(|e| BrainError::InvalidData(format!("building gazetteer: {e}")))?;
        Ok(Self { ac, entity_ids })
    }

    /// Scans `text`, returning every match that sits on word boundaries
    /// (so a pattern like `"Fire"` won't match inside `"Firebolt"`).
    pub fn scan(&self, text: &str) -> Vec<Mention> {
        let bytes = text.as_bytes();
        self.ac
            .find_iter(text)
            .filter_map(|m| {
                let (start, end) = (m.start(), m.end());
                let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
                let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
                if before_ok && after_ok {
                    Some(Mention { entity_id: self.entity_ids[m.pattern().as_usize()], start, end })
                } else {
                    None
                }
            })
            .collect()
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::EntityId;

    #[test]
    fn matches_whole_words_and_prefers_longest() {
        let gaz = Gazetteer::build(&[
            (EntityId::new(1), "Fire".to_string()),
            (EntityId::new(2), "Fireball".to_string()),
        ])
        .unwrap();
        let hits = gaz.scan("Wizards cast Fireball, not just Fire.");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].entity_id, EntityId::new(2), "must prefer the longer match, not \"Fire\" inside \"Fireball\"");
        assert_eq!(hits[1].entity_id, EntityId::new(1));
    }

    #[test]
    fn does_not_match_inside_a_longer_unrelated_word() {
        let gaz = Gazetteer::build(&[(EntityId::new(1), "Fire".to_string())]).unwrap();
        let hits = gaz.scan("The firearm was stolen.");
        assert!(hits.is_empty(), "\"Fire\" must not match inside \"firearm\"");
    }

    #[test]
    fn excludes_very_short_names_from_the_gazetteer() {
        let gaz = Gazetteer::build(&[(EntityId::new(1), "Ox".to_string())]).unwrap();
        let hits = gaz.scan("An ox pulls the cart, oxen everywhere.");
        assert!(hits.is_empty(), "2-char names are excluded to avoid false-positive noise");
    }

    #[test]
    fn is_case_insensitive() {
        let gaz = Gazetteer::build(&[(EntityId::new(1), "Fireball".to_string())]).unwrap();
        let hits = gaz.scan("a FIREBALL spell");
        assert_eq!(hits.len(), 1);
    }
}
