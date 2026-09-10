//! SQL schema and forward-only migrations for a brain's `graph.db`.
//!
//! Migrations are applied in order and tracked via SQLite's built-in
//! `PRAGMA user_version`, so opening an already-current database is a
//! no-op and opening an older one upgrades it in place.

use brain_core::error::{BrainError, Result};
use rusqlite::Connection;

/// Ordered list of migrations. Index 0 upgrades version 0 -> 1, and so on;
/// there is no down-migration support (a fresh `brain init` is cheaper
/// than a rollback for a locally-regenerable index).
const MIGRATIONS: &[&str] = &[MIGRATION_0001];

const MIGRATION_0001: &str = r#"
CREATE TABLE documents (
    id           INTEGER PRIMARY KEY,
    path         TEXT NOT NULL UNIQUE,
    sha256       TEXT NOT NULL,
    title        TEXT NOT NULL,
    kind         TEXT NOT NULL,
    page_count   INTEGER NOT NULL,
    bytes        INTEGER NOT NULL,
    ingested_at  TEXT NOT NULL,
    extractor    TEXT NOT NULL,
    ocr          INTEGER NOT NULL
);
CREATE INDEX idx_documents_sha256 ON documents(sha256);

CREATE TABLE pages (
    id             INTEGER PRIMARY KEY,
    doc_id         INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page_no        INTEGER NOT NULL,
    width          REAL NOT NULL,
    height         REAL NOT NULL,
    text           TEXT NOT NULL,
    ocr_conf       REAL,
    low_confidence INTEGER NOT NULL DEFAULT 0,
    UNIQUE(doc_id, page_no)
);
CREATE INDEX idx_pages_doc ON pages(doc_id);

CREATE TABLE blocks (
    id      INTEGER PRIMARY KEY,
    page_id INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    col     INTEGER NOT NULL,
    ord     INTEGER NOT NULL,
    x0      REAL NOT NULL,
    y0      REAL NOT NULL,
    x1      REAL NOT NULL,
    y1      REAL NOT NULL,
    kind    TEXT NOT NULL,
    text    TEXT NOT NULL
);
CREATE INDEX idx_blocks_page ON blocks(page_id);

CREATE TABLE sections (
    id         INTEGER PRIMARY KEY,
    doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    parent_id  INTEGER REFERENCES sections(id) ON DELETE CASCADE,
    level      INTEGER NOT NULL,
    title      TEXT NOT NULL,
    slug       TEXT NOT NULL,
    start_page INTEGER NOT NULL,
    end_page   INTEGER NOT NULL
);
CREATE INDEX idx_sections_doc ON sections(doc_id);
CREATE INDEX idx_sections_parent ON sections(parent_id);

CREATE TABLE chunks (
    id         INTEGER PRIMARY KEY,
    doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    section_id INTEGER REFERENCES sections(id) ON DELETE SET NULL,
    start_page INTEGER NOT NULL,
    end_page   INTEGER NOT NULL,
    ord        INTEGER NOT NULL,
    kind       TEXT NOT NULL,
    text       TEXT NOT NULL,
    token_est  INTEGER NOT NULL
);
CREATE INDEX idx_chunks_doc ON chunks(doc_id);
CREATE INDEX idx_chunks_section ON chunks(section_id);

-- External-content FTS5 index over chunks.text, kept in sync by triggers
-- below so callers never have to remember to update it separately.
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    text,
    content = 'chunks',
    content_rowid = 'id',
    tokenize = 'porter unicode61'
);

CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, text) VALUES ('delete', old.id, old.text);
    INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE entities (
    id           INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL,
    name         TEXT NOT NULL,
    slug         TEXT NOT NULL,
    canonical_id INTEGER REFERENCES entities(id) ON DELETE SET NULL,
    centrality   REAL NOT NULL DEFAULT 0,
    confidence   REAL NOT NULL DEFAULT 0,
    UNIQUE(kind, slug)
);
CREATE INDEX idx_entities_slug ON entities(slug);
CREATE INDEX idx_entities_canonical ON entities(canonical_id);

CREATE TABLE entity_aliases (
    entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    alias     TEXT NOT NULL,
    norm      TEXT NOT NULL,
    PRIMARY KEY (entity_id, norm)
);
CREATE INDEX idx_aliases_norm ON entity_aliases(norm);

CREATE TABLE entity_defs (
    entity_id  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    chunk_id   INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    doc_id     INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    page_no    INTEGER NOT NULL,
    is_primary INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (entity_id, chunk_id)
);
CREATE INDEX idx_defs_entity ON entity_defs(entity_id);
CREATE INDEX idx_defs_doc ON entity_defs(doc_id);

CREATE TABLE entity_fields (
    entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    doc_id    INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key       TEXT NOT NULL,
    value     TEXT NOT NULL,
    PRIMARY KEY (entity_id, doc_id, key)
);
CREATE INDEX idx_fields_entity ON entity_fields(entity_id);

CREATE TABLE mentions (
    chunk_id  INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    entity_id INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    start_off INTEGER NOT NULL,
    end_off   INTEGER NOT NULL,
    weight    REAL NOT NULL DEFAULT 1.0
);
CREATE INDEX idx_mentions_chunk ON mentions(chunk_id);
CREATE INDEX idx_mentions_entity ON mentions(entity_id);

CREATE TABLE edges (
    src_id            INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    dst_id            INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL,
    weight            REAL NOT NULL DEFAULT 1.0,
    evidence_chunk_id INTEGER REFERENCES chunks(id) ON DELETE SET NULL,
    PRIMARY KEY (src_id, dst_id, kind)
);
CREATE INDEX idx_edges_src ON edges(src_id);
CREATE INDEX idx_edges_dst ON edges(dst_id);
"#;

/// Opens (or creates) the database at `path` and brings it up to the
/// latest schema version.
pub fn open_and_migrate(path: &std::path::Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path).map_err(|e| BrainError::Db(e.to_string()))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|e| BrainError::Db(e.to_string()))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| BrainError::Db(e.to_string()))?;
    migrate(&conn)?;
    Ok(conn)
}

/// Applies any migrations newer than the database's current
/// `user_version`, in order, each inside its own transaction.
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|e| BrainError::Db(e.to_string()))?;
    let current = current as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch(sql)
            .map_err(|e| BrainError::Db(format!("migration {}: {e}", i + 1)))?;
        conn.pragma_update(None, "user_version", (i + 1) as i64)
            .map_err(|e| BrainError::Db(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_migrates_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        // Re-running on an already-current DB must be a no-op, not an error.
        migrate(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
    }

    #[test]
    fn fts5_is_available_in_the_bundled_build() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute(
            "INSERT INTO documents(path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr)
             VALUES ('p','s','t','pdf',1,1,'2024-01-01T00:00:00Z','plain',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chunks(doc_id, section_id, start_page, end_page, ord, kind, text, token_est)
             VALUES (1, NULL, 1, 1, 0, 'prose', 'the quick brown fox jumps', 5)",
            [],
        )
        .unwrap();
        let hit: String = conn
            .query_row(
                "SELECT text FROM chunks_fts WHERE chunks_fts MATCH 'quick' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(hit.contains("quick"));
    }
}
