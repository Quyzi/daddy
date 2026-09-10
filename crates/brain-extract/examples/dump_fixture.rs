//! One-off helper: extract a single page's RawPage and dump it as JSON,
//! used to generate brain-layout's golden test fixtures from real PDFs.
//! Not part of the test suite. Usage:
//! `cargo run --release -p brain-extract --example dump_fixture -- <pdf> <page> <out.json>`

use brain_extract::{Extractor, PageRange, PopplerExtractor};

fn main() {
    let mut args = std::env::args().skip(1);
    let pdf = args.next().expect("usage: dump_fixture <pdf> <page> <out.json>");
    let page: u32 = args.next().expect("page number").parse().expect("page must be a number");
    let out = args.next().expect("out.json path");

    let extractor = PopplerExtractor::new().expect("poppler tools not found");
    let pages = extractor
        .extract(std::path::Path::new(&pdf), PageRange::single(page))
        .expect("extraction failed");
    let json = serde_json::to_string_pretty(&pages[0]).unwrap();
    std::fs::write(&out, json).unwrap();
    println!("wrote {out} ({} words)", pages[0].words.len());
}
