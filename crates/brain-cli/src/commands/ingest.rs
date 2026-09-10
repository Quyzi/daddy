//! `brain ingest` — extracts and caches text from source files, laying
//! out each document's pages and writing `documents`/`pages`/`blocks`
//! rows. Rule-pack-driven indexing (sections/chunks/entities/edges) is a
//! separate step — see `brain index` — so this command never needs to
//! re-run just because a rule pack changed.

use anyhow::{Context, Result};
use brain_core::{BrainConfig, Document, ExtractorKind, OcrMode, Page};
use brain_extract::{Cache, Extractor, PageRange, PlainExtractor, PopplerExtractor, TesseractExtractor};
use brain_store::Store;
use chrono::Utc;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

const KNOWN_EXTENSIONS: &[&str] =
    &["pdf", "txt", "md", "markdown", "log", "adoc", "rst", "csv", "html", "htm"];

/// One file's extraction result, produced by the parallel extraction
/// phase and consumed by the sequential database-write phase.
struct Extracted {
    path: PathBuf,
    sha256: String,
    title: String,
    kind: String,
    bytes: u64,
    extractor: ExtractorKind,
    raw_pages: Vec<brain_core::RawPage>,
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
    if files.is_empty() {
        println!("No source files found to ingest.");
        return Ok(());
    }

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

    let run_extraction = |path: &Path| -> Result<Option<Extracted>> {
        let sha256 = brain_extract::sha256_file(path)?;
        if already_ingested.contains(&sha256) && !force {
            return Ok(None);
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let is_pdf = ext == "pdf";
        let bytes = std::fs::metadata(path)?.len();
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

        Ok(Some(Extracted { path: path.to_path_buf(), sha256, title, kind: ext, bytes, extractor, raw_pages }))
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
    let results: Vec<(PathBuf, Result<Option<Extracted>>)> = files
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
            Ok(Some(extracted)) => {
                match write_document(&store, &config, extracted) {
                    Ok(pages) => {
                        println!("ingested {pages:>4} pages  {}", path.display());
                        ingested += 1;
                    }
                    Err(e) => {
                        eprintln!("FAILED writing {}: {e}", path.display());
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                eprintln!("FAILED extracting {}: {e}", path.display());
                failed += 1;
            }
        }
    }

    store.conn().execute_batch("COMMIT")?;

    println!("\ningested {ingested}, skipped {skipped} (already ingested), failed {failed}");
    Ok(())
}

fn write_document(store: &Store, config: &BrainConfig, extracted: Extracted) -> Result<usize> {
    let ocr = extracted.raw_pages.iter().any(|p| p.ocr_confidence.is_some());
    let doc_id = store.insert_document(&Document {
        id: None,
        path: extracted.path.display().to_string(),
        sha256: extracted.sha256,
        title: extracted.title,
        kind: extracted.kind,
        page_count: extracted.raw_pages.len() as u32,
        bytes: extracted.bytes,
        ingested_at: Utc::now(),
        extractor: extracted.extractor,
        ocr,
    })?;

    let laid_out = brain_layout::reconstruct_document(&extracted.raw_pages);
    let page_count = laid_out.len();
    for (raw, laid) in extracted.raw_pages.iter().zip(laid_out.iter()) {
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
            store.insert_block(&brain_core::Block {
                id: None,
                page_id,
                col: block.col,
                ord: block.ord,
                bbox: block.bbox,
                kind: block.kind,
                text: block.text.clone(),
            })?;
        }
    }
    Ok(page_count)
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
