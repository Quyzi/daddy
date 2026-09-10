//! Reads just enough of an existing wiki page's YAML frontmatter to know
//! whether `brain compile` is allowed to overwrite it. This is
//! deliberately not a general YAML parser — the wiki schema's
//! frontmatter is a fixed, simple `key: value` block, and scanning for
//! one field line is both sufficient and immune to whatever a human
//! editor's YAML formatting quirks might otherwise trip up.

use std::path::Path;

/// The two states a wiki page's `status:` frontmatter field can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStatus {
    /// Machine-generated; safe for `brain compile` to overwrite.
    Generated,
    /// Someone (human or a prior AI synthesis pass) has since edited
    /// this page; `brain compile` must never overwrite it.
    Curated,
}

/// Reads the `status:` field from a file's frontmatter, if the file
/// exists and has one. A missing file, missing frontmatter, or missing
/// field all count as [`PageStatus::Generated`] — i.e. safe to write —
/// since that's the only state a page can be in before it's ever been
/// generated at all.
pub fn read_status(path: &Path) -> PageStatus {
    let Ok(content) = std::fs::read_to_string(path) else { return PageStatus::Generated };
    for line in content.lines().take(20) {
        if let Some(value) = line.strip_prefix("status:") {
            if value.trim() == "curated" {
                return PageStatus::Curated;
            }
            break;
        }
        if line.trim() == "---" && !content.starts_with("---") {
            break; // reached the end of frontmatter without finding a status field
        }
    }
    PageStatus::Generated
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_file_is_generated() {
        let dir = tempdir().unwrap();
        assert_eq!(read_status(&dir.path().join("nope.md")), PageStatus::Generated);
    }

    #[test]
    fn detects_curated_status() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("fireball.md");
        std::fs::write(&path, "---\ntitle: Fireball\nstatus: curated\n---\nbody\n").unwrap();
        assert_eq!(read_status(&path), PageStatus::Curated);
    }

    #[test]
    fn defaults_to_generated_when_status_is_generated_or_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.md");
        std::fs::write(&path, "---\ntitle: A\nstatus: generated\n---\nbody\n").unwrap();
        assert_eq!(read_status(&path), PageStatus::Generated);

        let path2 = dir.path().join("b.md");
        std::fs::write(&path2, "---\ntitle: B\n---\nbody\n").unwrap();
        assert_eq!(read_status(&path2), PageStatus::Generated);
    }
}
