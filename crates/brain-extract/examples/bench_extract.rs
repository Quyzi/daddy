//! Manual throughput/correctness check against a real directory of PDFs.
//! Not part of the test suite (paths are machine-specific) — run with:
//! `cargo run --release -p brain-extract --example bench_extract -- <dir>`

use brain_extract::{Extractor, PageRange, PopplerExtractor};
use std::time::Instant;
use walkdir::WalkDir;

fn main() {
    let dir = std::env::args().nth(1).expect("usage: bench_extract <dir>");
    let extractor = PopplerExtractor::new().expect("poppler tools not found");

    let mut total_pages = 0u64;
    let mut total_words = 0u64;
    let mut errors = 0u64;
    let start = Instant::now();

    for entry in WalkDir::new(&dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("pdf")) != Some(true) {
            continue;
        }
        let cap = match extractor.probe(path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("PROBE FAIL {}: {e}", path.display());
                errors += 1;
                continue;
            }
        };
        if !cap.supported {
            eprintln!("UNSUPPORTED {}: {:?}", path.display(), cap.note);
            errors += 1;
            continue;
        }
        let t0 = Instant::now();
        match extractor.extract(path, PageRange::All) {
            Ok(pages) => {
                let words: usize = pages.iter().map(|p| p.words.len()).sum();
                total_pages += pages.len() as u64;
                total_words += words as u64;
                println!(
                    "{:>5} pages {:>8} words  {:>7.2}s  {}",
                    pages.len(),
                    words,
                    t0.elapsed().as_secs_f64(),
                    path.display()
                );
            }
            Err(e) => {
                eprintln!("EXTRACT FAIL {}: {e}", path.display());
                errors += 1;
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "\nTOTAL: {total_pages} pages, {total_words} words, {errors} errors, {:.2}s",
        elapsed.as_secs_f64()
    );
}
