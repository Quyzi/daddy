//! `brain ingest` — extracts and caches text from source files (and, if
//! `.brain/sources.toml` exists, queries configured live datasources),
//! writing `documents`/`pages`/`blocks` rows. Rule-pack-driven indexing
//! (sections/chunks/entities/edges) is a separate step — see `brain
//! index` — so this command never needs to re-run just because a rule
//! pack changed.
//!
//! Two extraction paths, chosen per file by extension:
//! - **Geometric** (`.pdf` and plain-text-ish formats with no structure
//!   of their own): [`brain_extract::Extractor`] produces positioned
//!   words, and `brain_layout::reconstruct_document` guesses structure
//!   from geometry — the path every format used before this pipeline
//!   grew format-aware support.
//! - **Structured** (`.md`/`.markdown`, `.adoc`, `.url`/`.urls`):
//!   [`brain_extract::StructuredExtractor`] produces final blocks
//!   directly, since these formats already know their own structure —
//!   see `brain_extract::structured`'s module docs for why guessing it
//!   geometrically would be strictly worse. One input file can yield
//!   *several* documents here (a `.urls` list — see [`Payload::Structured`]).

use anyhow::{Context, Result};
use brain_core::{BBox, Block, BlockKind, BrainConfig, Document, ExtractorKind, OcrMode, Page};
use brain_datasource::config::{DatasourceConfig, QueryConfig, SourcesConfig};
use brain_extract::{
    AsciidocExtractor, Cache, Extractor, MarkdownExtractor, PageRange, PlainExtractor,
    PopplerExtractor, StructuredDoc, StructuredExtractor, TesseractExtractor, UrlExtractor,
};
use brain_store::Store;
use chrono::Utc;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const KNOWN_EXTENSIONS: &[&str] =
    &["pdf", "txt", "md", "markdown", "log", "adoc", "rst", "csv", "html", "htm", "url", "urls"];

/// Extensions routed through a [`StructuredExtractor`] instead of the
/// geometric [`Extractor`] path.
fn structured_extractor_for(ext: &str) -> Option<Box<dyn StructuredExtractor>> {
    match ext {
        "md" | "markdown" => Some(Box::new(MarkdownExtractor::new())),
        "adoc" => Some(Box::new(AsciidocExtractor::new())),
        "url" | "urls" => Some(Box::new(UrlExtractor::new())),
        _ => None,
    }
}

/// What one input file's extraction produced.
enum Payload {
    /// The `.pdf`/plain-text path: one document, words to be laid out.
    Geometric { sha256: String, title: String, extractor: ExtractorKind, raw_pages: Vec<brain_core::RawPage> },
    /// The structured path: one document per entry (almost always one,
    /// except a `.urls` list — see this module's docs). Each carries its
    /// own content, so each gets its own dedup hash at write time rather
    /// than sharing the input file's hash (mandatory for URLs, whose
    /// live content can change while the `.url`/`.urls` pointer file
    /// itself does not).
    Structured(Vec<StructuredDoc>),
}

/// One file's extraction result, produced by the parallel extraction
/// phase and consumed by the sequential database-write phase.
struct ExtractedFile {
    path: PathBuf,
    kind: String,
    bytes: u64,
    payload: Payload,
}

/// Outcome of writing one [`ExtractedFile`]'s document(s) to storage.
#[derive(Default)]
struct WriteOutcome {
    written: u32,
    skipped: u32,
    lines: Vec<String>,
}

pub fn run(
    root: &Path,
    paths: &[PathBuf],
    jobs: Option<usize>,
    force: bool,
    ocr_override: Option<&str>,
) -> Result<()> {
    let mut config = BrainConfig::load(root).context("loading .brain/config.toml (run `brain init` first)")?;
    if let Some(mode) = ocr_override {
        config.ocr = mode.parse().map_err(|e: brain_core::BrainError| anyhow::anyhow!(e.to_string()))?;
    }

    let files = collect_input_files(root, paths)?;

    let db_path = BrainConfig::db_path(root);
    let store = Store::open(&db_path).context("opening .brain/graph.db")?;
    let already_ingested: HashSet<String> =
        store.list_documents()?.into_iter().map(|d| d.sha256).collect();

    let cache = Cache::new(BrainConfig::cache_dir(root)).context("opening extraction cache")?;
    let poppler = PopplerExtractor::new().context("locating poppler tools (pdftotext/pdfinfo)")?;
    let tesseract = TesseractExtractor::new().ok(); // OCR is optional; degrade gracefully if missing
    if tesseract.is_none() && config.ocr != OcrMode::Never {
        eprintln!("warning: tesseract not found on PATH; scanned/image-only pages will be skipped, not OCR'd");
    }
    let plain = PlainExtractor::new();

    let run_extraction = |path: &Path| -> Result<Option<ExtractedFile>> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let bytes = std::fs::metadata(path)?.len();

        if let Some(extractor) = structured_extractor_for(&ext) {
            // No pre-extraction skip check here: a `.urls` list's own
            // bytes are a poor proxy for whether its *fetched* content
            // changed, and Markdown/AsciiDoc parsing is cheap enough
            // that re-parsing an unchanged file every run costs nothing
            // worth optimizing away. Each resulting document gets its
            // own post-extraction dedup check in `write_extracted`.
            let docs = extractor.extract_structured(path)?;
            return Ok(Some(ExtractedFile { path: path.to_path_buf(), kind: ext, bytes, payload: Payload::Structured(docs) }));
        }

        let sha256 = brain_extract::sha256_file(path)?;
        if already_ingested.contains(&sha256) && !force {
            return Ok(None);
        }
        let is_pdf = ext == "pdf";
        let title = path.file_stem().and_then(|s| s.to_str()).unwrap_or("untitled").to_string();

        let raw_pages = if let Some(cached) = cache.get(&sha256)? {
            cached
        } else {
            let pages = if is_pdf {
                match &tesseract {
                    Some(t) => brain_extract::extract_auto(path, &poppler, t, &config)?,
                    None => poppler.extract(path, PageRange::All)?,
                }
            } else {
                plain.extract(path, PageRange::All)?
            };
            cache.put(&sha256, &pages)?;
            pages
        };

        let extractor = if !is_pdf {
            ExtractorKind::Plain
        } else if raw_pages.iter().any(|p| p.ocr_confidence.is_some()) {
            ExtractorKind::Tesseract
        } else {
            ExtractorKind::Poppler
        };

        Ok(Some(ExtractedFile {
            path: path.to_path_buf(),
            kind: ext,
            bytes,
            payload: Payload::Geometric { sha256, title, extractor, raw_pages },
        }))
    };

    // Files are processed one at a time, *not* in parallel: OCR already
    // parallelizes across a single book's pages internally (see
    // `TesseractExtractor::extract_pages`), and that inner parallelism
    // already runs on rayon's shared global pool. Also parallelizing
    // across files would nest nested rayon scopes several deep every
    // time more than one OCR-heavy book was in flight at once -- exactly
    // this corpus's shape, with a handful of fully-scanned books mixed
    // among mostly-clean ones -- which was measured to blow the stack
    // ("thread '<unknown>' has overflowed its stack") during development.
    // `-j` instead sizes rayon's *global* pool once, up front, so it's
    // the inner per-page OCR parallelism that gets to use it fully.
    if let Some(n) = jobs {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .context("configuring OCR thread pool")?;
    }
    let results: Vec<(PathBuf, Result<Option<ExtractedFile>>)> = files
        .iter()
        .map(|p| {
            eprintln!("extracting {}", p.display());
            (p.clone(), run_extraction(p))
        })
        .collect();

    let mut ingested = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    // Wrap every document's writes in one transaction. Left on
    // autocommit, SQLite fsyncs on every single INSERT -- fine for one
    // row, ruinous for the tens of thousands of block rows a multi-book
    // ingest produces (this is what took a 3-book smoke test from
    // seconds to multiple minutes during development).
    store.conn().execute_batch("BEGIN")?;
    for (path, result) in results {
        match result {
            Ok(None) => {
                skipped += 1;
            }
            Ok(Some(extracted)) => match write_extracted(&store, &config, extracted, &already_ingested, force) {
                Ok(outcome) => {
                    for line in outcome.lines {
                        println!("{line}");
                    }
                    ingested += outcome.written;
                    skipped += outcome.skipped;
                }
                Err(e) => {
                    eprintln!("FAILED writing {}: {e}", path.display());
                    failed += 1;
                }
            },
            Err(e) => {
                eprintln!("FAILED extracting {}: {e}", path.display());
                failed += 1;
            }
        }
    }

    let (ds_ingested, ds_skipped, ds_failed) =
        run_datasources(root, &store, &already_ingested, force).context("ingesting configured datasources")?;
    ingested += ds_ingested;
    skipped += ds_skipped;
    failed += ds_failed;

    store.conn().execute_batch("COMMIT")?;

    if files.is_empty() && ds_ingested == 0 && ds_skipped == 0 {
        println!("No source files or configured datasources found to ingest.");
        return Ok(());
    }
    println!("\ningested {ingested}, skipped {skipped} (already ingested/unchanged), failed {failed}");
    Ok(())
}

fn write_extracted(
    store: &Store,
    config: &BrainConfig,
    extracted: ExtractedFile,
    already_ingested: &HashSet<String>,
    force: bool,
) -> Result<WriteOutcome> {
    match extracted.payload {
        Payload::Geometric { sha256, title, extractor, raw_pages } => {
            let doc = Document {
                id: None,
                path: extracted.path.display().to_string(),
                sha256,
                title,
                kind: extracted.kind,
                page_count: raw_pages.len() as u32,
                bytes: extracted.bytes,
                ingested_at: Utc::now(),
                extractor,
                ocr: raw_pages.iter().any(|p| p.ocr_confidence.is_some()),
                frontmatter: None,
            };
            let doc_id = upsert_document(store, &doc)?;

            let laid_out = brain_layout::reconstruct_document(&raw_pages);
            let page_count = laid_out.len();
            for (raw, laid) in raw_pages.iter().zip(laid_out.iter()) {
                let low_confidence = brain_extract::is_low_confidence(raw, config);
                let page_id = store.insert_page(&Page {
                    id: None,
                    doc_id,
                    page_no: laid.page_no,
                    width: laid.width,
                    height: laid.height,
                    text: laid.reading_order_text(),
                    ocr_conf: raw.ocr_confidence,
                    low_confidence,
                })?;
                for block in &laid.blocks {
                    store.insert_block(&Block {
                        id: None,
                        page_id,
                        col: block.col,
                        ord: block.ord,
                        bbox: block.bbox,
                        kind: block.kind,
                        text: block.text.clone(),
                        heading_level: None,
                    })?;
                }
            }
            Ok(WriteOutcome {
                written: 1,
                skipped: 0,
                lines: vec![format!("ingested {page_count:>4} pages  {}", extracted.path.display())],
            })
        }
        Payload::Structured(docs) => {
            let mut outcome = WriteOutcome::default();
            for doc in docs {
                let content_hash = brain_extract::sha256_bytes(structured_doc_fingerprint(&doc).as_bytes());
                if already_ingested.contains(&content_hash) && !force {
                    outcome.skipped += 1;
                    continue;
                }
                let block_count = write_structured_doc(store, &extracted.kind, &doc, &content_hash)?;
                outcome.written += 1;
                outcome.lines.push(format!("ingested {block_count:>4} blocks  {}  <- {}", doc.title, extracted.path.display()));
            }
            Ok(outcome)
        }
    }
}

/// Inserts a [`StructuredDoc`] as one document with one synthetic page
/// (`page_no = 1`) holding its blocks — see `brain_extract::structured`'s
/// docs for why these formats bypass `brain-layout` entirely.
fn write_structured_doc(store: &Store, ext: &str, doc: &StructuredDoc, content_hash: &str) -> Result<usize> {
    let bytes: u64 = doc.blocks.iter().map(|b| b.text.len() as u64).sum();
    let height = doc.blocks.iter().map(|b| b.bbox.y1).fold(1.0f64, f64::max);
    let new_doc = Document {
        id: None,
        path: doc.source_path.clone(),
        sha256: content_hash.to_string(),
        title: doc.title.clone(),
        kind: ext.to_string(),
        page_count: 1,
        bytes,
        ingested_at: Utc::now(),
        extractor: ExtractorKind::Plain,
        ocr: false,
        frontmatter: doc.frontmatter.clone(),
    };
    let doc_id = upsert_document(store, &new_doc)?;

    let page_text = doc.blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
    let page_id = store.insert_page(&Page {
        id: None,
        doc_id,
        page_no: 1,
        width: 1.0,
        height,
        text: page_text,
        ocr_conf: None,
        low_confidence: false,
    })?;
    for block in &doc.blocks {
        store.insert_block(&Block {
            id: None,
            page_id,
            col: block.col,
            ord: block.ord,
            bbox: block.bbox,
            kind: block.kind,
            text: block.text.clone(),
            heading_level: block.heading_level,
        })?;
    }
    Ok(doc.blocks.len())
}

/// Deterministic bytes representing a structured document's content, for
/// dedup hashing — title plus every block's text, each field separated
/// by a control character that can't appear in normal text (so `"A" +
/// "B"` and `"AB"` split across two blocks never collide).
fn structured_doc_fingerprint(doc: &StructuredDoc) -> String {
    let mut s = String::with_capacity(doc.title.len() + doc.blocks.iter().map(|b| b.text.len() + 1).sum::<usize>());
    s.push_str(&doc.title);
    s.push('\u{1}');
    for block in &doc.blocks {
        s.push_str(&block.text);
        s.push('\u{1}');
    }
    s
}

/// Inserts `doc`, replacing any existing document at the same `path`
/// whose content has since changed. `documents.path` is unique, so a
/// second ingest of a locally-edited PDF, a re-fetched URL whose page
/// changed, or a datasource query whose rows changed would otherwise
/// fail outright on that constraint the moment its content (and
/// therefore its `sha256`) differs from what's already stored — deleting
/// the stale row first (which cascades to its pages/blocks/sections/
/// chunks/entity defs, per the schema's `ON DELETE CASCADE`s) makes
/// re-ingesting changed content at a stable path just work, the same way
/// it already works for a file whose content hasn't changed (the
/// `already_ingested` sha256 check skips it before ever reaching here).
fn upsert_document(store: &Store, doc: &Document) -> Result<brain_core::DocId> {
    if let Some(existing) = store.find_document_by_path(&doc.path)? {
        let existing_id = existing.id.expect("a document read back from storage always has an id");
        if existing.sha256 == doc.sha256 {
            return Ok(existing_id);
        }
        store.delete_document(existing_id)?;
    }
    Ok(store.insert_document(doc)?)
}

/// Runs every query in `.brain/sources.toml`, if it exists, writing one
/// document per configured query — see the implementation plan's
/// datasource section for why a query result's rows become heading +
/// colon-labeled field blocks (the same shape `brain-index`'s existing
/// field-continuation heuristic already understands, at no extra cost).
/// Returns `(ingested, skipped, failed)` query counts.
fn run_datasources(
    root: &Path,
    store: &Store,
    already_ingested: &HashSet<String>,
    force: bool,
) -> Result<(u32, u32, u32)> {
    let Some(sources) = SourcesConfig::load(root).context("parsing .brain/sources.toml")? else {
        return Ok((0, 0, 0));
    };

    let mut ingested = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for ds in &sources.datasources {
        let dsn = match brain_datasource::config::resolve_env(&ds.dsn) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("FAILED resolving dsn for datasource {:?}: {e}", ds.name);
                failed += ds.queries.len() as u32;
                continue;
            }
        };
        let conn = match brain_datasource::registry::open(&ds.kind, &dsn) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("FAILED connecting to datasource {:?}: {e}", ds.name);
                failed += ds.queries.len() as u32;
                continue;
            }
        };
        for query in &ds.queries {
            match conn.query(&query.sql) {
                Ok(rows) => match write_datasource_query(store, ds, query, &rows, already_ingested, force) {
                    Ok(Some(rows_written)) => {
                        println!("ingested {rows_written:>4} rows   {}/{}", ds.name, query.name);
                        ingested += 1;
                    }
                    Ok(None) => skipped += 1,
                    Err(e) => {
                        eprintln!("FAILED writing datasource query {}/{}: {e}", ds.name, query.name);
                        failed += 1;
                    }
                },
                Err(e) => {
                    eprintln!("FAILED running datasource query {}/{}: {e}", ds.name, query.name);
                    failed += 1;
                }
            }
        }
    }
    Ok((ingested, skipped, failed))
}

/// Writes one datasource query's result as a single document, one
/// section per row: a heading block (the row's `heading_column`, or its
/// first column) followed by a `"{Column}: {value}"` body block per
/// remaining column. Returns `Ok(None)` if the result is unchanged since
/// the last ingest (by content hash), `Ok(Some(row_count))` otherwise.
fn write_datasource_query(
    store: &Store,
    ds: &DatasourceConfig,
    query: &QueryConfig,
    rows: &[brain_datasource::Row],
    already_ingested: &HashSet<String>,
    force: bool,
) -> Result<Option<usize>> {
    let mut blocks = Vec::new();
    let mut fingerprint = String::new();
    let mut ord: u32 = 0;

    for row in rows {
        if row.is_empty() {
            continue;
        }
        let heading_idx = query
            .heading_column
            .as_deref()
            .and_then(|name| row.iter().position(|(col, _)| col == name))
            .unwrap_or(0);

        let heading_text = row[heading_idx].1.clone();
        fingerprint.push_str(&heading_text);
        fingerprint.push('\u{1}');
        blocks.push(row_block(BlockKind::Heading, Some(1), heading_text, ord));
        ord += 1;

        for (i, (column, value)) in row.iter().enumerate() {
            if i == heading_idx {
                continue;
            }
            fingerprint.push_str(column);
            fingerprint.push(':');
            fingerprint.push_str(value);
            fingerprint.push('\u{1}');
            blocks.push(row_block(BlockKind::Body, None, format!("{column}: {value}"), ord));
            ord += 1;
        }
    }

    let content_hash = brain_extract::sha256_bytes(fingerprint.as_bytes());
    if already_ingested.contains(&content_hash) && !force {
        return Ok(None);
    }

    let path = format!("datasource://{}/{}", ds.name, query.name);
    let bytes: u64 = blocks.iter().map(|b| b.text.len() as u64).sum();
    let doc = Document {
        id: None,
        path,
        sha256: content_hash,
        title: query.name.clone(),
        kind: format!("{}-query", ds.kind),
        page_count: 1,
        bytes,
        ingested_at: Utc::now(),
        extractor: ExtractorKind::Plain,
        ocr: false,
        frontmatter: None,
    };
    let doc_id = upsert_document(store, &doc)?;

    let page_text = blocks.iter().map(|b| b.text.as_str()).collect::<Vec<_>>().join("\n\n");
    let page_id = store.insert_page(&Page {
        id: None,
        doc_id,
        page_no: 1,
        width: 1.0,
        height: ord as f64,
        text: page_text,
        ocr_conf: None,
        low_confidence: false,
    })?;
    let row_count = rows.len();
    for block in blocks {
        store.insert_block(&Block { page_id, ..block })?;
    }
    Ok(Some(row_count))
}

fn row_block(kind: BlockKind, heading_level: Option<u8>, text: String, ord: u32) -> Block {
    Block {
        id: None,
        page_id: brain_core::PageId::new(0), // placeholder; overwritten in `write_datasource_query`
        col: 0,
        ord,
        bbox: BBox { x0: 0.0, y0: ord as f64, x1: 1.0, y1: ord as f64 + 1.0 },
        kind,
        text,
        heading_level,
    }
}

/// Resolves the set of files to ingest: explicit `paths` (files used
/// directly, directories walked recursively), or `{root}/raw/` by
/// default (skipping `raw/assets`, per the brain schema convention).
fn collect_input_files(root: &Path, paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let roots: Vec<PathBuf> = if paths.is_empty() { vec![root.join("raw")] } else { paths.to_vec() };
    let mut files = Vec::new();
    for p in roots {
        if p.is_file() {
            files.push(p);
            continue;
        }
        if !p.is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&p).into_iter().filter_entry(|e| {
            e.file_name().to_str().map(|n| n != "assets").unwrap_or(true)
        }) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            if KNOWN_EXTENSIONS.contains(&ext.as_str()) {
                files.push(entry.into_path());
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::PageId;

    fn sample_block(text: &str, ord: u32, heading_level: Option<u8>) -> brain_extract::StructuredBlock {
        brain_extract::StructuredBlock {
            col: 0,
            ord,
            bbox: BBox { x0: 0.0, y0: ord as f64, x1: 1.0, y1: ord as f64 + 1.0 },
            kind: if heading_level.is_some() { BlockKind::Heading } else { BlockKind::Body },
            text: text.to_string(),
            heading_level,
        }
    }

    fn sample_doc(title: &str, body: &str) -> StructuredDoc {
        StructuredDoc {
            title: title.to_string(),
            source_path: format!("https://example.com/{title}"),
            frontmatter: None,
            blocks: vec![sample_block(title, 0, Some(1)), sample_block(body, 1, None)],
        }
    }

    #[test]
    fn fingerprint_is_stable_and_content_sensitive() {
        let a = sample_doc("Same Title", "same body");
        let b = sample_doc("Same Title", "same body");
        let c = sample_doc("Same Title", "different body");
        assert_eq!(structured_doc_fingerprint(&a), structured_doc_fingerprint(&b));
        assert_ne!(structured_doc_fingerprint(&a), structured_doc_fingerprint(&c));
    }

    #[test]
    fn upsert_document_replaces_changed_content_at_the_same_path_instead_of_erroring() {
        // Regression test for a real bug this change fixed: `documents.path`
        // is UNIQUE, so re-ingesting a URL/datasource query/edited file at
        // the same path with genuinely different content used to fail
        // outright on that constraint instead of updating in place.
        let store = Store::open_in_memory().unwrap();
        let base = Document {
            id: None,
            path: "https://example.com/note".to_string(),
            sha256: "hash-one".to_string(),
            title: "Note".to_string(),
            kind: "url".to_string(),
            page_count: 1,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Plain,
            ocr: false,
            frontmatter: None,
        };
        let first_id = upsert_document(&store, &base).unwrap();

        let mut changed = base.clone();
        changed.sha256 = "hash-two".to_string();
        changed.title = "Note (updated)".to_string();
        let second_id = upsert_document(&store, &changed).unwrap();

        let fetched = store.get_document(second_id).unwrap();
        assert_eq!(fetched.sha256, "hash-two");
        assert_eq!(fetched.title, "Note (updated)");
        assert_eq!(store.list_documents().unwrap().len(), 1, "the stale row must be replaced, not duplicated");
        let _ = first_id;
    }

    #[test]
    fn upsert_document_is_a_no_op_when_content_is_unchanged() {
        let store = Store::open_in_memory().unwrap();
        let doc = Document {
            id: None,
            path: "https://example.com/note".to_string(),
            sha256: "same-hash".to_string(),
            title: "Note".to_string(),
            kind: "url".to_string(),
            page_count: 1,
            bytes: 1,
            ingested_at: Utc::now(),
            extractor: ExtractorKind::Plain,
            ocr: false,
            frontmatter: None,
        };
        let first_id = upsert_document(&store, &doc).unwrap();
        let second_id = upsert_document(&store, &doc).unwrap();
        assert_eq!(first_id, second_id);
        assert_eq!(store.list_documents().unwrap().len(), 1);
    }

    #[test]
    fn write_extracted_structured_writes_new_and_skips_unchanged_documents() {
        let store = Store::open_in_memory().unwrap();
        let config = BrainConfig::default();
        let doc = sample_doc("A Note", "some body text");
        let hash = brain_extract::sha256_bytes(structured_doc_fingerprint(&doc).as_bytes());

        let extracted = ExtractedFile {
            path: PathBuf::from("raw/urls/list.urls"),
            kind: "urls".to_string(),
            bytes: 0,
            payload: Payload::Structured(vec![doc.clone()]),
        };
        let outcome = write_extracted(&store, &config, extracted, &HashSet::new(), false).unwrap();
        assert_eq!(outcome.written, 1);
        assert_eq!(outcome.skipped, 0);

        let already = [hash].into_iter().collect::<HashSet<_>>();
        let extracted_again = ExtractedFile {
            path: PathBuf::from("raw/urls/list.urls"),
            kind: "urls".to_string(),
            bytes: 0,
            payload: Payload::Structured(vec![doc]),
        };
        let outcome2 = write_extracted(&store, &config, extracted_again, &already, false).unwrap();
        assert_eq!(outcome2.written, 0);
        assert_eq!(outcome2.skipped, 1, "unchanged content, already ingested, must be skipped not re-inserted");
    }

    #[test]
    fn write_datasource_query_shapes_rows_as_heading_plus_colon_fields() {
        let store = Store::open_in_memory().unwrap();
        let ds = DatasourceConfig { name: "campaign".into(), kind: "sqlite".into(), dsn: "unused".into(), queries: vec![] };
        let query = QueryConfig { name: "npcs".into(), sql: "unused".into(), heading_column: Some("name".into()) };
        let rows = vec![vec![
            ("name".to_string(), "Ismark".to_string()),
            ("race".to_string(), "Human".to_string()),
        ]];

        let written = write_datasource_query(&store, &ds, &query, &rows, &HashSet::new(), false).unwrap();
        assert_eq!(written, Some(1));

        let doc = store.find_document_by_path("datasource://campaign/npcs").unwrap().unwrap();
        let blocks = store.list_blocks(PageId::new(1)).unwrap();
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[0].text, "Ismark");
        assert_eq!(blocks[1].text, "race: Human");
        let _ = doc;
    }
}
