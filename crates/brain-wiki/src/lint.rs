//! `brain lint`: mechanical implementation of the brain schema's Lint
//! Workflow health checks that are actually derivable from the stored
//! graph — contradictions and orphans. (Stale-claim detection, "concept
//! mentioned but never explained", and citation-completeness checks stay
//! judgement calls for the AI to make over the compiled wiki; this tool
//! surfaces the structural facts a human or AI needs to make them.)

use brain_core::error::Result;
use brain_core::slugify;
use brain_store::Store;
use serde::Serialize;
use std::collections::HashSet;

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

/// One `[[wikilink]]` whose target doesn't match any ingested document.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedWikilink {
    /// The document containing the link.
    pub source_document: String,
    /// The link's target text, as written.
    pub target: String,
}

/// The full result of one `lint` run.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LintReport {
    /// Entities with no edges and no recorded mentions anywhere.
    pub orphans: Vec<String>,
    /// Field-level contradictions across documents.
    pub contradictions: Vec<Contradiction>,
    /// `[[wikilink]]`s whose target isn't (yet) an ingested document —
    /// not dropped at index time (see `brain_index::orchestrate`'s
    /// wikilink-resolution pass), surfaced here instead so nothing about
    /// a broken link is silently lost.
    pub unresolved_wikilinks: Vec<UnresolvedWikilink>,
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

    let unresolved_wikilinks = find_unresolved_wikilinks(store)?;

    Ok(LintReport { orphans, contradictions, unresolved_wikilinks })
}

/// Re-derives every `[[wikilink]]`'s resolution against the current
/// document set, rather than reading a persisted list — the same
/// "compute live from the graph" approach as every other check here (see
/// this module's top-level docs), and it means a link that resolves
/// today because its target has since been ingested stops being
/// reported with no extra bookkeeping anywhere.
fn find_unresolved_wikilinks(store: &Store) -> Result<Vec<UnresolvedWikilink>> {
    let documents = store.list_documents()?;
    let known_slugs: HashSet<String> = documents.iter().map(|d| slugify(&d.title)).collect();

    let mut unresolved = Vec::new();
    for doc in &documents {
        let Some(doc_id) = doc.id else { continue };
        for chunk in store.list_chunks(doc_id)? {
            for link in brain_index::extract_wikilinks(&chunk.text) {
                if !known_slugs.contains(&slugify(&link.target)) {
                    unresolved.push(UnresolvedWikilink {
                        source_document: doc.title.clone(),
                        target: link.target,
                    });
                }
            }
        }
    }
    Ok(unresolved)
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
        out.push_str("None found.\n\n");
    } else {
        for name in &report.orphans {
            out.push_str(&format!("- {name}\n"));
        }
        out.push('\n');
    }

    out.push_str(&format!("## Unresolved wikilinks ({})\n\n", report.unresolved_wikilinks.len()));
    if report.unresolved_wikilinks.is_empty() {
        out.push_str("None found.\n");
    } else {
        for link in &report.unresolved_wikilinks {
            out.push_str(&format!("- [[{}]] in {} \u{2014} no matching document\n", link.target, link.source_document));
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
            contradictions: vec![Contradiction {
                entity_name: "Goblin".into(),
                key: "Hit Points".into(),
                values: vec![("7".into(), "Monster Manual".into()), ("9".into(), "Errata 2019".into())],
            }],
            ..Default::default()
        };
        let md = render_lint_markdown(&report);
        assert!(md.contains("**Goblin** \u{2014} `Hit Points`"));
        assert!(md.contains("7 (Monster Manual)"));
        assert!(md.contains("9 (Errata 2019)"));
    }
}
