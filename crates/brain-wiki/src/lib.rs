//! Deterministic wiki generation and health-check linting.
//!
//! [`compile::compile`] turns the graph into `wiki/*.md` pages following
//! the brain schema's conventions (YAML frontmatter, `[[links]]`,
//! `[Source: ...]` citations) — one page per entity above a centrality
//! threshold, an `index.md`, and an appended `log.md` entry. A page a
//! human or the AI has since hand-edited (`status: curated` in its
//! frontmatter — see [`frontmatter`]) is never overwritten.
//!
//! [`lint::lint`] runs the mechanically-derivable half of the brain
//! schema's Lint Workflow: field-level contradictions across documents
//! and orphaned entities. Everything here costs zero AI tokens; the AI's
//! job starts where this leaves off (synthesis, judging which
//! contradiction is authoritative, deciding what an orphan is worth
//! explaining).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod compile;
pub mod frontmatter;
pub mod lint;
pub mod page;

pub use compile::{compile, CompileReport};
pub use frontmatter::{read_status, PageStatus};
pub use lint::{lint, render_lint_markdown, Contradiction, LintReport};
pub use page::build_page;
