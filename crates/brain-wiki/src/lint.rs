//! `brain lint`: mechanical implementation of the brain schema's Lint
//! Workflow health checks that are actually derivable from the stored
//! graph — contradictions and orphans. (Stale-claim detection, "concept
//! mentioned but never explained", and citation-completeness checks stay
//! judgement calls for the AI to make over the compiled wiki; this tool
//! surfaces the structural facts a human or AI needs to make them.)

use brain_core::error::Result;
use brain_store::Store;
use serde::Serialize;

/// One entity where two or more source documents state a different
/// value for the same field.
#[derive(Debug, Clone, Serialize)]
pub struct Contradiction {
    /// The entity in question.
    pub entity_name: String,
    /// The field that differs.
    pub key: String,
    /// Every document's stated value, newest first.
    pub values: Vec<(String, String)>,
}

/// The full result of one `lint` run.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LintReport {
    /// Entities with no edges and no recorded mentions anywhere.
    pub orphans: Vec<String>,
    /// Field-level contradictions across documents.
    pub contradictions: Vec<Contradiction>,
}

/// Runs every lint check against `store`.
pub fn lint(store: &Store) -> Result<LintReport> {
    let orphans = store.find_orphan_entities()?.into_iter().map(|e| e.name).collect();

    let mut contradictions = Vec::new();
    for c in store.find_field_contradictions()? {
        let entity = store.get_entity(c.entity_id)?;
        contradictions.push(Contradiction {
            entity_name: entity.name,
            key: c.key,
            values: c.values.into_iter().map(|v| (v.value, v.document)).collect(),
        });
    }

    Ok(LintReport { orphans, contradictions })
}

/// Renders a [`LintReport`] as markdown, matching the brain schema's
/// `wiki/lint-report-[date].md` convention.
pub fn render_lint_markdown(report: &LintReport) -> String {
    let mut out = String::new();
    out.push_str("# Lint Report\n\n");

    out.push_str(&format!("## Contradictions ({})\n\n", report.contradictions.len()));
    if report.contradictions.is_empty() {
        out.push_str("None found.\n\n");
    } else {
        for c in &report.contradictions {
            out.push_str(&format!("- **{}** \u{2014} `{}` disagrees across sources:\n", c.entity_name, c.key));
            for (value, doc) in &c.values {
                out.push_str(&format!("  - {value} ({doc})\n"));
            }
        }
        out.push('\n');
    }

    out.push_str(&format!("## Orphan entities ({})\n\n", report.orphans.len()));
    if report.orphans.is_empty() {
        out.push_str("None found.\n");
    } else {
        for name in &report.orphans {
            out.push_str(&format!("- {name}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_report_renders_reassuringly() {
        let report = LintReport::default();
        let md = render_lint_markdown(&report);
        assert!(md.contains("None found"));
    }

    #[test]
    fn renders_a_contradiction_with_all_its_sources() {
        let report = LintReport {
            orphans: vec![],
            contradictions: vec![Contradiction {
                entity_name: "Goblin".into(),
                key: "Hit Points".into(),
                values: vec![("7".into(), "Monster Manual".into()), ("9".into(), "Errata 2019".into())],
            }],
        };
        let md = render_lint_markdown(&report);
        assert!(md.contains("**Goblin** \u{2014} `Hit Points`"));
        assert!(md.contains("7 (Monster Manual)"));
        assert!(md.contains("9 (Errata 2019)"));
    }
}
