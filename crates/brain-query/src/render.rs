//! Markdown rendering for `brain explore` and `brain get` output. JSON
//! output (the `--json` CLI flag) just serializes the types directly via
//! `serde_json` and doesn't need a renderer.

use crate::types::{EntityView, ExploreResult};
use brain_core::slugify;

/// Renders an [`ExploreResult`] as markdown: one section per result item,
/// each with a `[Source: ...]` citation, matching the citation
/// convention used throughout this project's wiki pages.
pub fn render_explore_markdown(result: &ExploreResult) -> String {
    let mut out = format!("# Results for: {}\n\n", result.query);
    if result.items.is_empty() {
        out.push_str("_No results found._\n");
        return out;
    }
    for item in &result.items {
        if let Some(name) = &item.entity_name {
            let kind = item.entity_kind.as_deref().unwrap_or("");
            out.push_str(&format!("## {name} ({kind})\n\n"));
        }
        out.push_str(item.text.trim());
        out.push_str(&format!("\n\n[Source: {} p.{}]\n\n", item.citation.document, item.citation.page));
    }
    out
}

/// Renders an [`EntityView`] as markdown: definition, fields table, and
/// related-entity links (as `[[wiki-style]]` links, consistent with the
/// wiki pages `brain compile` generates).
pub fn render_entity_markdown(view: &EntityView) -> String {
    let mut out = format!("# {} ({})\n\n", view.name, view.kind);

    match (&view.definition, &view.citation) {
        (Some(def), Some(c)) => {
            out.push_str(def.trim());
            out.push_str(&format!("\n\n[Source: {} p.{}]\n\n", c.document, c.page));
        }
        (Some(def), None) => {
            out.push_str(def.trim());
            out.push_str("\n\n");
        }
        _ => out.push_str("_No definition on file._\n\n"),
    }

    if !view.fields.is_empty() {
        out.push_str("## Fields\n\n| key | value | source |\n|---|---|---|\n");
        for f in &view.fields {
            out.push_str(&format!("| {} | {} | {} |\n", f.key, f.value, f.document));
        }
        out.push('\n');
    }

    if !view.neighbours.is_empty() {
        out.push_str("## Related\n\n");
        let links: Vec<String> =
            view.neighbours.iter().map(|n| format!("[[{}]]", slugify(&n.name))).collect();
        out.push_str(&links.join(" \u{b7} "));
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Citation, FieldValue, Neighbour};
    use brain_core::EntityId;

    #[test]
    fn empty_explore_result_says_so_plainly() {
        let result = ExploreResult { query: "nonsense".into(), items: vec![], total_tokens: 0 };
        let md = render_explore_markdown(&result);
        assert!(md.contains("No results found"));
    }

    #[test]
    fn entity_view_renders_fields_and_related_links() {
        let view = EntityView {
            name: "Fireball".into(),
            kind: "spell".into(),
            definition: Some("A bright streak flashes...".into()),
            citation: Some(Citation { document: "Player's Handbook".into(), page: 241 }),
            fields: vec![FieldValue { key: "Range".into(), value: "150 feet".into(), document: "Player's Handbook".into() }],
            neighbours: vec![Neighbour { id: EntityId::new(1), name: "Wizard".into(), kind: "class".into(), weight: 3.0 }],
        };
        let md = render_entity_markdown(&view);
        assert!(md.contains("# Fireball (spell)"));
        assert!(md.contains("[Source: Player's Handbook p.241]"));
        assert!(md.contains("| Range | 150 feet | Player's Handbook |"));
        assert!(md.contains("[[wizard]]"));
    }
}
