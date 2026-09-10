//! Rule packs: declarative TOML files that teach `brain index` what kinds
//! of entities to recognize in a document's headings, with zero Rust
//! code per domain. See `packs/generic.toml` (works for any brain) and
//! `packs/dnd5e.toml` (spell/monster/class/... recognizers) for examples.

use brain_core::error::{BrainError, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::Path;

/// Default search window (characters of section body text, from the
/// start) a `require` pattern is matched against when the pack doesn't
/// specify one. Generous enough to reach past a spell's cantrip/level
/// line to its "Casting Time:" line, tight enough to not accidentally
/// match text belonging to the *next* section on a short heading.
fn default_within_chars() -> usize {
    400
}

/// One `[[entity]]` table in a rule pack's TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct EntityRule {
    /// The entity kind this rule recognizes (`"spell"`, `"monster"`,
    /// `"topic"`, or any other string — see [`brain_core::EntityKind`]).
    pub kind: String,
    /// What kind of layout element anchors a candidate: currently always
    /// a heading block (the only anchor `brain-layout` produces that's
    /// reliable enough to build sections from). Kept as a table for
    /// forward-compatibility with other anchor types.
    #[serde(default)]
    pub anchor: Anchor,
    /// Patterns that must ALL match somewhere in the section's body text
    /// for this rule to recognize it. An empty list always matches —
    /// that's how `packs/generic.toml`'s catch-all "topic" rule works.
    #[serde(default)]
    pub require: Vec<RequirePattern>,
    /// Field names to extract as `entity_fields`, matched as literal
    /// line prefixes against each body block (e.g. a field named
    /// `"Armor Class"` matches a block whose text starts with
    /// `"Armor Class"`, capturing the remainder as the value).
    #[serde(default)]
    pub fields: Vec<String>,
    /// Whether a matched section must stay a single, never-split chunk
    /// regardless of length (`true` for structured entities like spells
    /// and monsters) or may be split into multiple prose chunks once it
    /// gets long (`false` for generic, headings-only topics — see
    /// `packs/generic.toml`).
    #[serde(default = "default_atomic")]
    pub atomic: bool,
}

fn default_atomic() -> bool {
    true
}

/// Anchor configuration for an [`EntityRule`]. Every field is currently
/// documentation-only (all sections in this crate's model already start
/// at a heading) but kept as real, parsed schema so a future anchor type
/// doesn't require a breaking TOML format change.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Anchor {
    /// Whether the anchor is a heading block. Always `true` in v1.
    #[serde(default)]
    pub heading: bool,
    /// Whether an ALL-CAPS heading (common for monster stat block names)
    /// is an acceptable anchor, as opposed to requiring title case.
    #[serde(default)]
    pub allcaps_ok: bool,
}

/// One `require` entry: a regex that must match within the first
/// `within_chars` characters of a section's body text.
#[derive(Debug, Clone, Deserialize)]
pub struct RequirePattern {
    /// The pattern (Rust `regex` crate syntax).
    pub regex: String,
    /// How far into the section body to search for it.
    #[serde(default = "default_within_chars")]
    pub within_chars: usize,
}

/// A parsed (but not yet regex-compiled) rule pack.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RulePack {
    /// Optional human-readable name for logging.
    #[serde(default)]
    pub name: Option<String>,
    /// The recognizers, tried in file order — the first whose `require`
    /// patterns all match wins, so put more specific rules first and a
    /// catch-all last.
    #[serde(default, rename = "entity")]
    pub entities: Vec<EntityRule>,
}

impl RulePack {
    /// Parses a rule pack from TOML text.
    pub fn parse(toml_text: &str) -> Result<Self> {
        toml::from_str(toml_text).map_err(|e| BrainError::RulePack(e.to_string()))
    }

    /// Loads a rule pack from a file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::parse(&text)
    }
}

/// A [`RequirePattern`] with its regex already compiled.
#[derive(Debug)]
pub struct CompiledRequire {
    /// Compiled pattern.
    pub regex: Regex,
    /// Search window, in characters.
    pub within_chars: usize,
}

/// An [`EntityRule`] with its patterns compiled once, up front, instead
/// of per-section.
#[derive(Debug)]
pub struct CompiledRule {
    /// Entity kind this rule produces.
    pub kind: String,
    /// Compiled `require` patterns; a section must satisfy all of them.
    pub requires: Vec<CompiledRequire>,
    /// Field names to extract.
    pub fields: Vec<String>,
    /// Whether a match must stay a single, never-split chunk.
    pub atomic: bool,
}

/// A rule pack ready to recognize sections, with every pattern
/// pre-compiled and (optionally) a generic catch-all pack's rules
/// appended after this pack's own — see [`CompiledPack::load_with_fallback`].
#[derive(Debug)]
pub struct CompiledPack {
    /// Rules in match-priority order.
    pub rules: Vec<CompiledRule>,
}

impl CompiledPack {
    /// Every distinct field label this pack ever extracts, across all
    /// rules. Used by [`crate::sections::build_sections`] to recognize a
    /// bare field line (`"Armor Class 18"`, no colon) as a continuation
    /// of the section already open rather than a new heading — see that
    /// function's docs for why a field-label-only heuristic (matching a
    /// colon) isn't enough on its own.
    pub fn all_field_labels(&self) -> Vec<String> {
        let mut labels: Vec<String> =
            self.rules.iter().flat_map(|r| r.fields.iter().cloned()).collect();
        labels.sort();
        labels.dedup();
        labels
    }

    /// Compiles a single [`RulePack`]'s regexes.
    pub fn compile(pack: &RulePack) -> Result<Self> {
        let mut rules = Vec::with_capacity(pack.entities.len());
        for rule in &pack.entities {
            let mut requires = Vec::with_capacity(rule.require.len());
            for req in &rule.require {
                let regex = Regex::new(&req.regex)
                    .map_err(|e| BrainError::RulePack(format!("bad regex {:?}: {e}", req.regex)))?;
                requires.push(CompiledRequire { regex, within_chars: req.within_chars });
            }
            rules.push(CompiledRule {
                kind: rule.kind.clone(),
                requires,
                fields: rule.fields.clone(),
                atomic: rule.atomic,
            });
        }
        Ok(Self { rules })
    }

    /// Loads `{packs_dir}/{name}.toml`, and — unless `name` already *is*
    /// `"generic"` — appends `{packs_dir}/generic.toml`'s rules after it,
    /// so any heading a domain pack's specific rules don't recognize
    /// still becomes at least a generic topic entity (see the
    /// implementation plan's "generic pack gives a floor" rationale).
    pub fn load_with_fallback(packs_dir: &Path, name: &str) -> Result<Self> {
        let mut pack = RulePack::load(&packs_dir.join(format!("{name}.toml")))?;
        if name != "generic" {
            let generic_path = packs_dir.join("generic.toml");
            if generic_path.exists() {
                let generic = RulePack::load(&generic_path)?;
                pack.entities.extend(generic.entities);
            }
        }
        Self::compile(&pack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_pack() {
        let toml = r#"
name = "test"

[[entity]]
kind = "spell"
anchor = { heading = true }
require = [
  { regex = '^Casting Time:', within_chars = 200 },
]
fields = ["Casting Time", "Range"]
"#;
        let pack = RulePack::parse(toml).unwrap();
        assert_eq!(pack.entities.len(), 1);
        assert_eq!(pack.entities[0].kind, "spell");
        assert_eq!(pack.entities[0].fields, vec!["Casting Time", "Range"]);
    }

    #[test]
    fn empty_require_list_compiles_and_is_vacuously_satisfiable() {
        let toml = r#"
[[entity]]
kind = "topic"
anchor = { heading = true }
"#;
        let pack = RulePack::parse(toml).unwrap();
        let compiled = CompiledPack::compile(&pack).unwrap();
        assert_eq!(compiled.rules.len(), 1);
        assert!(compiled.rules[0].requires.is_empty());
    }

    #[test]
    fn bad_regex_is_a_rule_pack_error_not_a_panic() {
        let toml = r#"
[[entity]]
kind = "spell"
require = [{ regex = "(unclosed" }]
"#;
        let pack = RulePack::parse(toml).unwrap();
        let err = CompiledPack::compile(&pack).unwrap_err();
        assert!(matches!(err, BrainError::RulePack(_)));
    }
}
