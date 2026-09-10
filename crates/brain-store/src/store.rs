//! The [`Store`] type: a thin, typed CRUD layer over the schema in
//! [`crate::schema`]. Every downstream crate (`brain-index`, `brain-query`,
//! `brain-wiki`, `brain-cli`) talks to the database only through this API —
//! no other crate writes raw SQL.

use brain_core::error::{BrainError, Result};
use brain_core::{
    Block, BlockId, BlockKind, Chunk, ChunkId, ChunkKind, DocId, Document, Edge, EdgeKind,
    Entity, EntityId, EntityKind, ExtractorKind, Page, PageId, Section, SectionId,
};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// One matched row from an FTS5 search over chunk text.
#[derive(Debug, Clone)]
pub struct FtsHit {
    /// The matching chunk's id.
    pub chunk_id: ChunkId,
    /// SQLite FTS5's `bm25()` score (more negative = more relevant).
    pub bm25: f64,
}

/// A field value attached to an entity from a specific document (used
/// when the same entity has different stated values in different books,
/// e.g. errata).
#[derive(Debug, Clone)]
pub struct EntityField {
    /// The document this value came from.
    pub doc_id: DocId,
    /// Field name (e.g. `"Armor Class"`).
    pub key: String,
    /// Field value as printed in that document.
    pub value: String,
}

/// One document's stated value in a [`FieldContradiction`].
#[derive(Debug, Clone)]
pub struct ContradictingValue {
    /// The value as stated.
    pub value: String,
    /// Title of the document that states it.
    pub document: String,
    /// When that document was ingested (rows are ordered newest first,
    /// as a proxy for "more likely to reflect current errata").
    pub ingested_at: String,
}

/// An entity for which two or more documents state different values for
/// the same field.
#[derive(Debug, Clone)]
pub struct FieldContradiction {
    /// The entity in question.
    pub entity_id: EntityId,
    /// The field that differs.
    pub key: String,
    /// Every document's stated value, newest first.
    pub values: Vec<ContradictingValue>,
}

/// A typed handle to one brain's `graph.db`.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens the database at `path`, creating and migrating it if needed.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = crate::schema::open_and_migrate(path)?;
        Ok(Self { conn })
    }

    /// Opens an in-memory, already-migrated database. Used by tests and by
    /// short-lived tooling that never persists to disk.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().map_err(|e| BrainError::Db(e.to_string()))?;
        crate::schema::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// Runs `f` inside a single SQLite transaction, committing on `Ok` and
    /// rolling back on `Err`. Use this to wrap multi-row writes (e.g. one
    /// document's worth of pages) so a mid-batch failure never leaves the
    /// database half-written.
    pub fn transaction<T>(&mut self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let result = f(&tx)?;
        tx.commit().map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(result)
    }

    /// Direct access to the underlying connection for callers that need a
    /// query this API doesn't yet expose (e.g. ad-hoc `brain lint` reports).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ---------------------------------------------------------------
    // Documents
    // ---------------------------------------------------------------

    /// Inserts a new document. Fails if `path` is already present — callers
    /// should check [`Store::find_document_by_sha256`] first to decide
    /// whether this is a re-ingest.
    pub fn insert_document(&self, doc: &Document) -> Result<DocId> {
        self.conn
            .execute(
                "INSERT INTO documents(path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    doc.path,
                    doc.sha256,
                    doc.title,
                    doc.kind,
                    doc.page_count,
                    doc.bytes as i64,
                    doc.ingested_at.to_rfc3339(),
                    doc.extractor.as_str(),
                    doc.ocr as i64,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(DocId::new(self.conn.last_insert_rowid()))
    }

    /// Looks up a document by content hash — the cache-hit / re-ingest check.
    pub fn find_document_by_sha256(&self, sha256: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr
                 FROM documents WHERE sha256 = ?1",
                params![sha256],
                Self::row_to_document,
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Looks up a document by its path as recorded at ingest time.
    pub fn find_document_by_path(&self, path: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr
                 FROM documents WHERE path = ?1",
                params![path],
                Self::row_to_document,
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Fetches a document by id.
    pub fn get_document(&self, id: DocId) -> Result<Document> {
        self.conn
            .query_row(
                "SELECT id, path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr
                 FROM documents WHERE id = ?1",
                params![id.get()],
                Self::row_to_document,
            )
            .map_err(|_| BrainError::NotFound(format!("document {id}")))
    }

    /// Looks a document up by its title or filename stem (case-insensitive,
    /// exact match). Used by `brain page <doc> <n>` when the caller passes
    /// a human-readable name rather than an id.
    pub fn find_document_by_title(&self, title: &str) -> Result<Option<Document>> {
        self.conn
            .query_row(
                "SELECT id, path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr
                 FROM documents WHERE lower(title) = lower(?1) LIMIT 1",
                params![title],
                Self::row_to_document,
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every ingested document.
    pub fn list_documents(&self) -> Result<Vec<Document>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, path, sha256, title, kind, page_count, bytes, ingested_at, extractor, ocr
                 FROM documents ORDER BY title",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], Self::row_to_document)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Deletes a document and (via `ON DELETE CASCADE`) every page, block,
    /// section, and chunk derived from it. Used by `brain ingest --force`
    /// to fully replace a previously-ingested file.
    pub fn delete_document(&self, id: DocId) -> Result<()> {
        self.conn
            .execute("DELETE FROM documents WHERE id = ?1", params![id.get()])
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    fn row_to_document(row: &rusqlite::Row) -> rusqlite::Result<Document> {
        let extractor: String = row.get(8)?;
        let ingested_at: String = row.get(7)?;
        Ok(Document {
            id: Some(DocId::new(row.get(0)?)),
            path: row.get(1)?,
            sha256: row.get(2)?,
            title: row.get(3)?,
            kind: row.get(4)?,
            page_count: row.get::<_, i64>(5)? as u32,
            bytes: row.get::<_, i64>(6)? as u64,
            ingested_at: DateTime::parse_from_rfc3339(&ingested_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            extractor: extractor.parse().unwrap_or(ExtractorKind::Plain),
            ocr: row.get::<_, i64>(9)? != 0,
        })
    }

    // ---------------------------------------------------------------
    // Pages
    // ---------------------------------------------------------------

    /// Inserts one page's final (post-layout) text and quality metadata.
    pub fn insert_page(&self, page: &Page) -> Result<PageId> {
        self.conn
            .execute(
                "INSERT INTO pages(doc_id, page_no, width, height, text, ocr_conf, low_confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    page.doc_id.get(),
                    page.page_no,
                    page.width,
                    page.height,
                    page.text,
                    page.ocr_conf,
                    page.low_confidence as i64,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(PageId::new(self.conn.last_insert_rowid()))
    }

    /// Fetches one page of a document by 1-based page number.
    pub fn get_page(&self, doc_id: DocId, page_no: u32) -> Result<Page> {
        self.conn
            .query_row(
                "SELECT id, doc_id, page_no, width, height, text, ocr_conf, low_confidence
                 FROM pages WHERE doc_id = ?1 AND page_no = ?2",
                params![doc_id.get(), page_no],
                Self::row_to_page,
            )
            .map_err(|_| BrainError::NotFound(format!("page {page_no} of document {doc_id}")))
    }

    /// Lists every page of a document, in page order.
    pub fn list_pages(&self, doc_id: DocId) -> Result<Vec<Page>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, page_no, width, height, text, ocr_conf, low_confidence
                 FROM pages WHERE doc_id = ?1 ORDER BY page_no",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![doc_id.get()], Self::row_to_page)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    fn row_to_page(row: &rusqlite::Row) -> rusqlite::Result<Page> {
        Ok(Page {
            id: Some(PageId::new(row.get(0)?)),
            doc_id: DocId::new(row.get(1)?),
            page_no: row.get::<_, i64>(2)? as u32,
            width: row.get(3)?,
            height: row.get(4)?,
            text: row.get(5)?,
            ocr_conf: row.get(6)?,
            low_confidence: row.get::<_, i64>(7)? != 0,
        })
    }

    // ---------------------------------------------------------------
    // Blocks
    // ---------------------------------------------------------------

    /// Inserts one laid-out block.
    pub fn insert_block(&self, block: &Block) -> Result<BlockId> {
        self.conn
            .execute(
                "INSERT INTO blocks(page_id, col, ord, x0, y0, x1, y1, kind, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    block.page_id.get(),
                    block.col,
                    block.ord,
                    block.bbox.x0,
                    block.bbox.y0,
                    block.bbox.x1,
                    block.bbox.y1,
                    block_kind_str(block.kind),
                    block.text,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(BlockId::new(self.conn.last_insert_rowid()))
    }

    /// Lists every block of a page, in reading order.
    pub fn list_blocks(&self, page_id: PageId) -> Result<Vec<Block>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, page_id, col, ord, x0, y0, x1, y1, kind, text
                 FROM blocks WHERE page_id = ?1 ORDER BY ord",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![page_id.get()], |row| {
                let kind: String = row.get(8)?;
                Ok(Block {
                    id: Some(BlockId::new(row.get(0)?)),
                    page_id: PageId::new(row.get(1)?),
                    col: row.get::<_, i64>(2)? as u32,
                    ord: row.get::<_, i64>(3)? as u32,
                    bbox: brain_core::BBox {
                        x0: row.get(4)?,
                        y0: row.get(5)?,
                        x1: row.get(6)?,
                        y1: row.get(7)?,
                    },
                    kind: parse_block_kind(&kind),
                    text: row.get(9)?,
                })
            })
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every block of a document across all its pages, ordered
    /// `(page_no, ord)` — i.e. full document reading order. This is what
    /// `brain-index`'s section builder consumes.
    pub fn list_blocks_for_document(&self, doc_id: DocId) -> Result<Vec<(u32, Block)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT p.page_no, b.id, b.page_id, b.col, b.ord, b.x0, b.y0, b.x1, b.y1, b.kind, b.text
                 FROM blocks b JOIN pages p ON p.id = b.page_id
                 WHERE p.doc_id = ?1 AND p.low_confidence = 0
                 ORDER BY p.page_no, b.ord",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![doc_id.get()], |row| {
                let page_no: i64 = row.get(0)?;
                let kind: String = row.get(9)?;
                Ok((
                    page_no as u32,
                    Block {
                        id: Some(BlockId::new(row.get(1)?)),
                        page_id: PageId::new(row.get(2)?),
                        col: row.get::<_, i64>(3)? as u32,
                        ord: row.get::<_, i64>(4)? as u32,
                        bbox: brain_core::BBox {
                            x0: row.get(5)?,
                            y0: row.get(6)?,
                            x1: row.get(7)?,
                            y1: row.get(8)?,
                        },
                        kind: parse_block_kind(&kind),
                        text: row.get(10)?,
                    },
                ))
            })
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Wipes every derived table (sections, chunks, entities and
    /// everything hanging off them) while leaving `documents`, `pages`,
    /// and `blocks` untouched. `brain index` calls this before rebuilding
    /// from scratch — cheap, since it's rebuilding from already-stored
    /// blocks rather than re-extracting or re-laying-out anything.
    pub fn clear_index(&mut self) -> Result<()> {
        self.transaction(|tx| {
            for table in [
                "mentions",
                "edges",
                "entity_fields",
                "entity_defs",
                "entity_aliases",
                "entities",
                "chunks",
                "sections",
            ] {
                tx.execute(&format!("DELETE FROM {table}"), [])
                    .map_err(|e| BrainError::Db(e.to_string()))?;
            }
            Ok(())
        })
    }

    // ---------------------------------------------------------------
    // Sections
    // ---------------------------------------------------------------

    /// Inserts one section.
    pub fn insert_section(&self, section: &Section) -> Result<SectionId> {
        self.conn
            .execute(
                "INSERT INTO sections(doc_id, parent_id, level, title, slug, start_page, end_page)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    section.doc_id.get(),
                    section.parent_id.map(|p| p.get()),
                    section.level,
                    section.title,
                    section.slug,
                    section.start_page,
                    section.end_page,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(SectionId::new(self.conn.last_insert_rowid()))
    }

    /// Lists every section of a document, in document order.
    pub fn list_sections(&self, doc_id: DocId) -> Result<Vec<Section>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, parent_id, level, title, slug, start_page, end_page
                 FROM sections WHERE doc_id = ?1 ORDER BY start_page",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![doc_id.get()], Self::row_to_section)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    fn row_to_section(row: &rusqlite::Row) -> rusqlite::Result<Section> {
        let parent: Option<i64> = row.get(2)?;
        Ok(Section {
            id: Some(SectionId::new(row.get(0)?)),
            doc_id: DocId::new(row.get(1)?),
            parent_id: parent.map(SectionId::new),
            level: row.get::<_, i64>(3)? as u32,
            title: row.get(4)?,
            slug: row.get(5)?,
            start_page: row.get::<_, i64>(6)? as u32,
            end_page: row.get::<_, i64>(7)? as u32,
        })
    }

    // ---------------------------------------------------------------
    // Chunks (+ FTS5)
    // ---------------------------------------------------------------

    /// Inserts one chunk. The `chunks_fts` index is kept in sync
    /// automatically by triggers defined in the schema.
    pub fn insert_chunk(&self, chunk: &Chunk) -> Result<ChunkId> {
        self.conn
            .execute(
                "INSERT INTO chunks(doc_id, section_id, start_page, end_page, ord, kind, text, token_est)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    chunk.doc_id.get(),
                    chunk.section_id.map(|s| s.get()),
                    chunk.start_page,
                    chunk.end_page,
                    chunk.ord,
                    chunk_kind_str(chunk.kind),
                    chunk.text,
                    chunk.token_est,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(ChunkId::new(self.conn.last_insert_rowid()))
    }

    /// Fetches one chunk by id.
    pub fn get_chunk(&self, id: ChunkId) -> Result<Chunk> {
        self.conn
            .query_row(
                "SELECT id, doc_id, section_id, start_page, end_page, ord, kind, text, token_est
                 FROM chunks WHERE id = ?1",
                params![id.get()],
                Self::row_to_chunk,
            )
            .map_err(|_| BrainError::NotFound(format!("chunk {id}")))
    }

    /// Lists every chunk of a document, in document order.
    pub fn list_chunks(&self, doc_id: DocId) -> Result<Vec<Chunk>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, section_id, start_page, end_page, ord, kind, text, token_est
                 FROM chunks WHERE doc_id = ?1 ORDER BY ord",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![doc_id.get()], Self::row_to_chunk)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Full-text searches chunk text via FTS5 BM25, best matches first.
    /// `query` is passed through to SQLite's FTS5 query syntax.
    pub fn search_chunks(&self, query: &str, limit: usize) -> Result<Vec<FtsHit>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT rowid, bm25(chunks_fts) AS score FROM chunks_fts
                 WHERE chunks_fts MATCH ?1 ORDER BY score LIMIT ?2",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![query, limit as i64], |row| {
                Ok(FtsHit {
                    chunk_id: ChunkId::new(row.get(0)?),
                    bm25: row.get(1)?,
                })
            })
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every chunk in the database, across all documents. Used by
    /// the gazetteer pass, which needs to scan all chunk text in one go
    /// after every document's entities are known.
    pub fn list_all_chunks(&self) -> Result<Vec<Chunk>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, doc_id, section_id, start_page, end_page, ord, kind, text, token_est
                 FROM chunks ORDER BY doc_id, ord",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], Self::row_to_chunk)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    fn row_to_chunk(row: &rusqlite::Row) -> rusqlite::Result<Chunk> {
        let section: Option<i64> = row.get(2)?;
        let kind: String = row.get(6)?;
        Ok(Chunk {
            id: Some(ChunkId::new(row.get(0)?)),
            doc_id: DocId::new(row.get(1)?),
            section_id: section.map(SectionId::new),
            start_page: row.get::<_, i64>(3)? as u32,
            end_page: row.get::<_, i64>(4)? as u32,
            ord: row.get::<_, i64>(5)? as u32,
            kind: parse_chunk_kind(&kind),
            text: row.get(7)?,
            token_est: row.get::<_, i64>(8)? as u32,
        })
    }

    // ---------------------------------------------------------------
    // Entities, aliases, definitions, fields, mentions, edges
    // ---------------------------------------------------------------

    /// Inserts a new entity, or returns the id of an existing one with the
    /// same `(kind, slug)` — entity identity is idempotent on that pair so
    /// re-indexing never creates duplicates.
    pub fn upsert_entity(&self, entity: &Entity) -> Result<EntityId> {
        self.conn
            .execute(
                "INSERT INTO entities(kind, name, slug, canonical_id, centrality, confidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(kind, slug) DO UPDATE SET
                    name = excluded.name,
                    confidence = max(entities.confidence, excluded.confidence)",
                params![
                    entity.kind.as_str(),
                    entity.name,
                    entity.slug,
                    entity.canonical_id.map(|c| c.get()),
                    entity.centrality,
                    entity.confidence,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        self.conn
            .query_row(
                "SELECT id FROM entities WHERE kind = ?1 AND slug = ?2",
                params![entity.kind.as_str(), entity.slug],
                |r| r.get(0),
            )
            .map(EntityId::new)
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Fetches an entity by id.
    pub fn get_entity(&self, id: EntityId) -> Result<Entity> {
        self.conn
            .query_row(
                "SELECT id, kind, name, slug, canonical_id, centrality, confidence
                 FROM entities WHERE id = ?1",
                params![id.get()],
                Self::row_to_entity,
            )
            .map_err(|_| BrainError::NotFound(format!("entity {id}")))
    }

    /// Finds an entity by exact slug (any kind). A name can legitimately
    /// end up with two entities sharing a slug — e.g. "Fireball" the
    /// bare spell-list table entry (recognized only by the generic
    /// catch-all pack rule, kind `"topic"`) versus "Fireball" the actual
    /// spell definition elsewhere in the same book — and the table entry
    /// can easily out-rank the real definition on centrality alone (it
    /// sits packed among dozens of other spell names, which is a lot of
    /// cheap co-occurrence weight). So a non-`"topic"` kind always wins
    /// over `"topic"` regardless of centrality; centrality only breaks
    /// ties within the same specificity tier.
    pub fn find_entity_by_slug(&self, slug: &str) -> Result<Option<Entity>> {
        self.conn
            .query_row(
                "SELECT id, kind, name, slug, canonical_id, centrality, confidence
                 FROM entities WHERE slug = ?1
                 ORDER BY (kind = 'topic') ASC, centrality DESC LIMIT 1",
                params![slug],
                Self::row_to_entity,
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Finds an entity by an alias's normalized form (see
    /// [`Store::add_alias`]).
    pub fn find_entity_by_alias_norm(&self, norm: &str) -> Result<Option<Entity>> {
        self.conn
            .query_row(
                "SELECT e.id, e.kind, e.name, e.slug, e.canonical_id, e.centrality, e.confidence
                 FROM entities e JOIN entity_aliases a ON a.entity_id = e.id
                 WHERE a.norm = ?1 ORDER BY e.centrality DESC LIMIT 1",
                params![norm],
                Self::row_to_entity,
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every entity, optionally filtered to one kind.
    pub fn list_entities(&self, kind: Option<&EntityKind>) -> Result<Vec<Entity>> {
        let sql = "SELECT id, kind, name, slug, canonical_id, centrality, confidence FROM entities";
        if let Some(k) = kind {
            let mut stmt = self
                .conn
                .prepare(&format!("{sql} WHERE kind = ?1 ORDER BY centrality DESC"))
                .map_err(|e| BrainError::Db(e.to_string()))?;
            let rows = stmt
                .query_map(params![k.as_str()], Self::row_to_entity)
                .map_err(|e| BrainError::Db(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| BrainError::Db(e.to_string()))?;
            Ok(rows)
        } else {
            let mut stmt = self
                .conn
                .prepare(&format!("{sql} ORDER BY centrality DESC"))
                .map_err(|e| BrainError::Db(e.to_string()))?;
            let rows = stmt
                .query_map([], Self::row_to_entity)
                .map_err(|e| BrainError::Db(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| BrainError::Db(e.to_string()))?;
            Ok(rows)
        }
    }

    /// Updates an entity's stored centrality (called once per PageRank run).
    pub fn set_entity_centrality(&self, id: EntityId, centrality: f64) -> Result<()> {
        self.conn
            .execute(
                "UPDATE entities SET centrality = ?1 WHERE id = ?2",
                params![centrality, id.get()],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    fn row_to_entity(row: &rusqlite::Row) -> rusqlite::Result<Entity> {
        let kind: String = row.get(1)?;
        let canonical: Option<i64> = row.get(4)?;
        Ok(Entity {
            id: Some(EntityId::new(row.get(0)?)),
            kind: EntityKind::parse(&kind),
            name: row.get(2)?,
            slug: row.get(3)?,
            canonical_id: canonical.map(EntityId::new),
            centrality: row.get(5)?,
            confidence: row.get(6)?,
        })
    }

    /// Adds an alias for an entity. `norm` is the normalized (lowercased,
    /// whitespace-collapsed) form used for lookup; `alias` preserves the
    /// original casing for display.
    pub fn add_alias(&self, entity_id: EntityId, alias: &str, norm: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO entity_aliases(entity_id, alias, norm) VALUES (?1, ?2, ?3)",
                params![entity_id.get(), alias, norm],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    /// Records that `chunk_id` (in `doc_id`, on `page_no`) defines
    /// `entity_id`. `is_primary` marks the single definition an entity's
    /// wiki page should quote as its canonical text.
    pub fn add_definition(
        &self,
        entity_id: EntityId,
        chunk_id: ChunkId,
        doc_id: DocId,
        page_no: u32,
        is_primary: bool,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO entity_defs(entity_id, chunk_id, doc_id, page_no, is_primary)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entity_id.get(),
                    chunk_id.get(),
                    doc_id.get(),
                    page_no,
                    is_primary as i64,
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    /// Returns the entity a chunk primarily defines, if any (looked up
    /// from `entity_defs`). Used during gazetteer scanning to know which
    /// entity a `Mentions` edge should originate from.
    pub fn defining_entity_for_chunk(&self, chunk_id: ChunkId) -> Result<Option<EntityId>> {
        self.conn
            .query_row(
                "SELECT entity_id FROM entity_defs WHERE chunk_id = ?1 AND is_primary = 1 LIMIT 1",
                params![chunk_id.get()],
                |r| r.get(0),
            )
            .optional()
            .map(|opt: Option<i64>| opt.map(EntityId::new))
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Finds entities whose name contains `query` (case-insensitive),
    /// best (highest-centrality) matches first, capped at `limit`. Used
    /// by `brain explore` to seed a graph walk from name matches that an
    /// FTS5 search over chunk *text* might miss (e.g. a query that's
    /// just an entity's name, appearing verbatim only in its own
    /// heading, not repeated throughout its definition).
    pub fn search_entities_by_name(&self, query: &str, limit: usize) -> Result<Vec<Entity>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, name, slug, canonical_id, centrality, confidence
                 FROM entities WHERE name LIKE ?1 ESCAPE '\\'
                 ORDER BY (kind = 'topic') ASC, centrality DESC LIMIT ?2",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let pattern = format!("%{}%", like_escape(query));
        let rows = stmt
            .query_map(params![pattern, limit as i64], Self::row_to_entity)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Returns `(chunk_id, doc_id, page_no)` for an entity's primary
    /// definition, if it has one — the single citable source
    /// `brain get`/`brain explore` quote as "the" definition.
    pub fn primary_definition(&self, entity_id: EntityId) -> Result<Option<(ChunkId, DocId, u32)>> {
        self.conn
            .query_row(
                "SELECT chunk_id, doc_id, page_no FROM entity_defs
                 WHERE entity_id = ?1 AND is_primary = 1 LIMIT 1",
                params![entity_id.get()],
                |r| {
                    let chunk_id: i64 = r.get(0)?;
                    let doc_id: i64 = r.get(1)?;
                    let page_no: i64 = r.get(2)?;
                    Ok((ChunkId::new(chunk_id), DocId::new(doc_id), page_no as u32))
                },
            )
            .optional()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Sets one structured field value (e.g. `"Armor Class" -> "18"`) for
    /// an entity as stated in one document.
    pub fn set_field(&self, entity_id: EntityId, doc_id: DocId, key: &str, value: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO entity_fields(entity_id, doc_id, key, value) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(entity_id, doc_id, key) DO UPDATE SET value = excluded.value",
                params![entity_id.get(), doc_id.get(), key, value],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    /// Lists every field value recorded for an entity across all documents
    /// (multiple rows per key means multiple documents disagree — see
    /// `brain lint`'s contradiction detection).
    pub fn list_fields(&self, entity_id: EntityId) -> Result<Vec<EntityField>> {
        let mut stmt = self
            .conn
            .prepare("SELECT doc_id, key, value FROM entity_fields WHERE entity_id = ?1 ORDER BY key")
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![entity_id.get()], |row| {
                Ok(EntityField {
                    doc_id: DocId::new(row.get(0)?),
                    key: row.get(1)?,
                    value: row.get(2)?,
                })
            })
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Records that `entity_id` is mentioned in `chunk_id` at byte offset
    /// `[start, end)`.
    pub fn add_mention(
        &self,
        chunk_id: ChunkId,
        entity_id: EntityId,
        start: usize,
        end: usize,
        weight: f64,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO mentions(chunk_id, entity_id, start_off, end_off, weight)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![chunk_id.get(), entity_id.get(), start as i64, end as i64, weight],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    /// Counts how many chunks mention `entity_id`, used as a cheap
    /// popularity signal.
    pub fn mention_count(&self, entity_id: EntityId) -> Result<u64> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM mentions WHERE entity_id = ?1",
                params![entity_id.get()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as u64)
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Inserts or strengthens an edge between two entities. Calling this
    /// again for the same `(src, dst, kind)` adds to the existing weight
    /// rather than duplicating the row — repeated co-occurrence should
    /// make an edge stronger, not create parallel edges.
    pub fn add_edge(&self, edge: &Edge) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO edges(src_id, dst_id, kind, weight, evidence_chunk_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(src_id, dst_id, kind) DO UPDATE SET
                    weight = edges.weight + excluded.weight",
                params![
                    edge.src.get(),
                    edge.dst.get(),
                    edge.kind.as_str(),
                    edge.weight,
                    edge.evidence_chunk_id.map(|c| c.get()),
                ],
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        Ok(())
    }

    /// Lists every outgoing edge from `entity_id`, optionally filtered by
    /// kind, heaviest first.
    pub fn edges_from(&self, entity_id: EntityId, kind: Option<EdgeKind>) -> Result<Vec<Edge>> {
        let sql = "SELECT src_id, dst_id, kind, weight, evidence_chunk_id FROM edges WHERE src_id = ?1";
        if let Some(k) = kind {
            let mut stmt = self
                .conn
                .prepare(&format!("{sql} AND kind = ?2 ORDER BY weight DESC"))
                .map_err(|e| BrainError::Db(e.to_string()))?;
            let rows = stmt
                .query_map(params![entity_id.get(), k.as_str()], Self::row_to_edge)
                .map_err(|e| BrainError::Db(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| BrainError::Db(e.to_string()))?;
            Ok(rows)
        } else {
            let mut stmt = self
                .conn
                .prepare(&format!("{sql} ORDER BY weight DESC"))
                .map_err(|e| BrainError::Db(e.to_string()))?;
            let rows = stmt
                .query_map(params![entity_id.get()], Self::row_to_edge)
                .map_err(|e| BrainError::Db(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| BrainError::Db(e.to_string()))?;
            Ok(rows)
        }
    }

    /// Lists every edge touching `entity_id` in either direction — the
    /// full neighbourhood used by graph-walk expansion in `brain-query`.
    pub fn edges_touching(&self, entity_id: EntityId) -> Result<Vec<Edge>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT src_id, dst_id, kind, weight, evidence_chunk_id FROM edges
                 WHERE src_id = ?1 OR dst_id = ?1 ORDER BY weight DESC",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![entity_id.get()], Self::row_to_edge)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every edge in the graph. Used by PageRank, which needs the
    /// whole adjacency structure at once.
    pub fn all_edges(&self) -> Result<Vec<Edge>> {
        let mut stmt = self
            .conn
            .prepare("SELECT src_id, dst_id, kind, weight, evidence_chunk_id FROM edges")
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], Self::row_to_edge)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Lists every `(doc_id, page_no, is_primary)` an entity is defined
    /// at, across every document — the wiki page's "Appears in" section
    /// wants every source, not just the primary one.
    pub fn list_definitions(&self, entity_id: EntityId) -> Result<Vec<(DocId, u32, bool)>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT doc_id, page_no, is_primary FROM entity_defs
                 WHERE entity_id = ?1 ORDER BY is_primary DESC, doc_id",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map(params![entity_id.get()], |r| {
                let doc_id: i64 = r.get(0)?;
                let page_no: i64 = r.get(1)?;
                let is_primary: i64 = r.get(2)?;
                Ok((DocId::new(doc_id), page_no as u32, is_primary != 0))
            })
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    /// Counts the distinct documents that define an entity — the
    /// `source_count` a wiki page's frontmatter reports.
    pub fn definition_doc_count(&self, entity_id: EntityId) -> Result<usize> {
        self.conn
            .query_row(
                "SELECT COUNT(DISTINCT doc_id) FROM entity_defs WHERE entity_id = ?1",
                params![entity_id.get()],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n as usize)
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    // ---------------------------------------------------------------
    // Lint queries
    // ---------------------------------------------------------------

    /// Finds entities where two or more documents state different values
    /// for the same field (e.g. an errata'd hit-point total, or two
    /// sourcebooks disagreeing on a monster's challenge rating). Each
    /// returned [`FieldContradiction`] carries every document's stated
    /// value, most-recently-ingested first, so callers can flag which is
    /// likely more authoritative without guessing.
    pub fn find_field_contradictions(&self) -> Result<Vec<FieldContradiction>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT ef.entity_id, ef.key FROM entity_fields ef
                 GROUP BY ef.entity_id, ef.key
                 HAVING COUNT(DISTINCT ef.value) > 1
                 ORDER BY ef.entity_id, ef.key",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let pairs: Vec<(i64, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| BrainError::Db(e.to_string()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))?;

        let mut out = Vec::with_capacity(pairs.len());
        for (entity_id, key) in pairs {
            let mut vstmt = self
                .conn
                .prepare(
                    "SELECT ef.value, d.title, d.ingested_at FROM entity_fields ef
                     JOIN documents d ON d.id = ef.doc_id
                     WHERE ef.entity_id = ?1 AND ef.key = ?2
                     ORDER BY d.ingested_at DESC",
                )
                .map_err(|e| BrainError::Db(e.to_string()))?;
            let values = vstmt
                .query_map(params![entity_id, key], |r| {
                    Ok(ContradictingValue { value: r.get(0)?, document: r.get(1)?, ingested_at: r.get(2)? })
                })
                .map_err(|e| BrainError::Db(e.to_string()))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| BrainError::Db(e.to_string()))?;
            out.push(FieldContradiction { entity_id: EntityId::new(entity_id), key, values });
        }
        Ok(out)
    }

    /// Finds entities with no inbound or outbound edges and no recorded
    /// mentions anywhere in the corpus — likely a one-off heading that
    /// never connects to anything else (a stray table-of-contents
    /// fragment, a rarely-used term), the graph equivalent of a wiki
    /// page nothing links to.
    pub fn find_orphan_entities(&self) -> Result<Vec<Entity>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, name, slug, canonical_id, centrality, confidence FROM entities e
                 WHERE NOT EXISTS (SELECT 1 FROM edges WHERE src_id = e.id OR dst_id = e.id)
                   AND NOT EXISTS (SELECT 1 FROM mentions WHERE entity_id = e.id)
                 ORDER BY e.name",
            )
            .map_err(|e| BrainError::Db(e.to_string()))?;
        let rows = stmt
            .query_map([], Self::row_to_entity)
            .map_err(|e| BrainError::Db(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| BrainError::Db(e.to_string()))
    }

    fn row_to_edge(row: &rusqlite::Row) -> rusqlite::Result<Edge> {
        let kind: String = row.get(2)?;
        let evidence: Option<i64> = row.get(4)?;
        Ok(Edge {
            src: EntityId::new(row.get(0)?),
            dst: EntityId::new(row.get(1)?),
            kind: kind.parse().unwrap_or(EdgeKind::Mentions),
            weight: row.get(3)?,
            evidence_chunk_id: evidence.map(ChunkId::new),
        })
    }
}

/// Escapes `%`, `_`, and `\` in a user-supplied string so it can be
/// embedded in a `LIKE ... ESCAPE '\'` pattern without its own characters
/// acting as SQL wildcards.
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn block_kind_str(kind: BlockKind) -> &'static str {
    match kind {
        BlockKind::Heading => "heading",
        BlockKind::Body => "body",
        BlockKind::Table => "table",
        BlockKind::StatBlock => "statblock",
        BlockKind::Caption => "caption",
        BlockKind::Chrome => "chrome",
    }
}

fn parse_block_kind(s: &str) -> BlockKind {
    match s {
        "heading" => BlockKind::Heading,
        "table" => BlockKind::Table,
        "statblock" => BlockKind::StatBlock,
        "caption" => BlockKind::Caption,
        "chrome" => BlockKind::Chrome,
        _ => BlockKind::Body,
    }
}

fn chunk_kind_str(kind: ChunkKind) -> &'static str {
    match kind {
        ChunkKind::Prose => "prose",
        ChunkKind::Definition => "definition",
    }
}

fn parse_chunk_kind(s: &str) -> ChunkKind {
    match s {
        "definition" => ChunkKind::Definition,
        _ => ChunkKind::Prose,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::{BBox, Entity, EntityKind};

    fn sample_doc() -> Document {
        Document {
            id: None,
            path: "/tmp/x.pdf".into(),
            sha256: "abc123".into(),
            title: "Test Book".into(),
            kind: "pdf".into(),
            page_count: 1,
            bytes: 1234,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Poppler,
            ocr: false,
        }
    }

    #[test]
    fn document_roundtrip_and_dedup_lookup() {
        let store = Store::open_in_memory().unwrap();
        let id = store.insert_document(&sample_doc()).unwrap();
        let fetched = store.get_document(id).unwrap();
        assert_eq!(fetched.title, "Test Book");
        let by_sha = store.find_document_by_sha256("abc123").unwrap().unwrap();
        assert_eq!(by_sha.id, Some(id));
        assert!(store.find_document_by_sha256("nope").unwrap().is_none());
    }

    #[test]
    fn page_and_block_roundtrip() {
        let store = Store::open_in_memory().unwrap();
        let doc_id = store.insert_document(&sample_doc()).unwrap();
        let page_id = store
            .insert_page(&Page {
                id: None,
                doc_id,
                page_no: 1,
                width: 612.0,
                height: 792.0,
                text: "hello world".into(),
                ocr_conf: None,
                low_confidence: false,
            })
            .unwrap();
        let fetched = store.get_page(doc_id, 1).unwrap();
        assert_eq!(fetched.text, "hello world");

        store
            .insert_block(&Block {
                id: None,
                page_id,
                col: 0,
                ord: 0,
                bbox: BBox { x0: 0.0, y0: 0.0, x1: 10.0, y1: 10.0 },
                kind: BlockKind::Heading,
                text: "Chapter One".into(),
            })
            .unwrap();
        let blocks = store.list_blocks(page_id).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
    }

    #[test]
    fn chunk_search_finds_inserted_text() {
        let store = Store::open_in_memory().unwrap();
        let doc_id = store.insert_document(&sample_doc()).unwrap();
        let chunk_id = store
            .insert_chunk(&Chunk {
                id: None,
                doc_id,
                section_id: None,
                start_page: 1,
                end_page: 1,
                ord: 0,
                kind: ChunkKind::Definition,
                text: "Fireball is a 3rd-level evocation spell".into(),
                token_est: 10,
            })
            .unwrap();
        let hits = store.search_chunks("fireball", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].chunk_id, chunk_id);
    }

    #[test]
    fn slug_lookup_prefers_a_specific_kind_over_the_generic_topic_fallback_even_at_lower_centrality() {
        // Mirrors a real Player's Handbook case: "Fireball" appears both
        // as a bare spell-list table entry (recognized only by the
        // generic catch-all pack rule, kind "topic") and as the actual
        // spell definition elsewhere in the book. The table entry can
        // easily out-rank the real definition on centrality alone.
        let store = Store::open_in_memory().unwrap();
        let topic = Entity {
            id: None,
            kind: EntityKind::Topic,
            name: "Fireball".into(),
            slug: "fireball".into(),
            canonical_id: None,
            centrality: 0.05, // higher: sits in a dense spell-list table
            confidence: 0.5,
        };
        let spell = Entity { kind: EntityKind::Spell, centrality: 0.001, ..topic.clone() };
        store.upsert_entity(&topic).unwrap();
        let spell_id = store.upsert_entity(&spell).unwrap();

        let found = store.find_entity_by_slug("fireball").unwrap().expect("should find an entity");
        assert_eq!(found.id, Some(spell_id));
        assert_eq!(found.kind, EntityKind::Spell, "the real spell must win over the topic fallback despite lower centrality");
    }

    #[test]
    fn entity_upsert_is_idempotent_and_edges_accumulate_weight() {
        let store = Store::open_in_memory().unwrap();
        let e1 = Entity {
            id: None,
            kind: EntityKind::Spell,
            name: "Fireball".into(),
            slug: "fireball".into(),
            canonical_id: None,
            centrality: 0.0,
            confidence: 0.9,
        };
        let id1 = store.upsert_entity(&e1).unwrap();
        let id2 = store.upsert_entity(&e1).unwrap();
        assert_eq!(id1, id2, "same (kind, slug) must resolve to the same row");

        let e2 = Entity {
            kind: EntityKind::Class,
            name: "Wizard".into(),
            slug: "wizard".into(),
            ..e1.clone()
        };
        let wizard_id = store.upsert_entity(&e2).unwrap();

        store
            .add_edge(&Edge {
                src: wizard_id,
                dst: id1,
                kind: EdgeKind::Mentions,
                weight: 1.0,
                evidence_chunk_id: None,
            })
            .unwrap();
        store
            .add_edge(&Edge {
                src: wizard_id,
                dst: id1,
                kind: EdgeKind::Mentions,
                weight: 1.0,
                evidence_chunk_id: None,
            })
            .unwrap();
        let edges = store.edges_from(wizard_id, None).unwrap();
        assert_eq!(edges.len(), 1, "repeated edge must accumulate, not duplicate");
        assert_eq!(edges[0].weight, 2.0);
    }

    #[test]
    fn fields_and_contradiction_shape() {
        let store = Store::open_in_memory().unwrap();
        let d1 = store.insert_document(&sample_doc()).unwrap();
        let mut d2 = sample_doc();
        d2.path = "/tmp/y.pdf".into();
        d2.sha256 = "def456".into();
        let d2 = store.insert_document(&d2).unwrap();

        let entity = Entity {
            id: None,
            kind: EntityKind::Monster,
            name: "Goblin".into(),
            slug: "goblin".into(),
            canonical_id: None,
            centrality: 0.0,
            confidence: 1.0,
        };
        let eid = store.upsert_entity(&entity).unwrap();
        store.set_field(eid, d1, "Hit Points", "7").unwrap();
        store.set_field(eid, d2, "Hit Points", "9").unwrap();

        let fields = store.list_fields(eid).unwrap();
        assert_eq!(fields.len(), 2, "two documents disagreeing should yield two rows");

        let contradictions = store.find_field_contradictions().unwrap();
        assert_eq!(contradictions.len(), 1);
        assert_eq!(contradictions[0].entity_id, eid);
        assert_eq!(contradictions[0].key, "Hit Points");
        assert_eq!(contradictions[0].values.len(), 2);
    }

    #[test]
    fn orphan_entities_have_no_edges_and_no_mentions() {
        let store = Store::open_in_memory().unwrap();
        let connected = Entity {
            id: None,
            kind: EntityKind::Spell,
            name: "Fireball".into(),
            slug: "fireball".into(),
            canonical_id: None,
            centrality: 0.0,
            confidence: 1.0,
        };
        let orphan = Entity { name: "Stray Heading".into(), slug: "stray-heading".into(), ..connected.clone() };
        let connected_id = store.upsert_entity(&connected).unwrap();
        let orphan_id = store.upsert_entity(&orphan).unwrap();
        let other_id = store
            .upsert_entity(&Entity { name: "Wizard".into(), slug: "wizard".into(), kind: EntityKind::Class, ..connected })
            .unwrap();
        store
            .add_edge(&Edge { src: connected_id, dst: other_id, kind: EdgeKind::Mentions, weight: 1.0, evidence_chunk_id: None })
            .unwrap();

        let orphans = store.find_orphan_entities().unwrap();
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].id, Some(orphan_id));
    }
}
