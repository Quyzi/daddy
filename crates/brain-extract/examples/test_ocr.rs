use brain_core::{BrainConfig, OcrMode};
use brain_extract::{extract_auto, is_low_confidence, PopplerExtractor, TesseractExtractor};
use std::path::Path;
use std::time::Instant;

fn main() {
    let path = std::env::args().nth(1).expect("usage: test_ocr <pdf>");
    let poppler = PopplerExtractor::new().unwrap();
    let tesseract = TesseractExtractor::new().unwrap();
    let config = BrainConfig { ocr: OcrMode::Auto, ..Default::default() };

    let t0 = Instant::now();
    let pages = extract_auto(Path::new(&path), &poppler, &tesseract, &config).unwrap();
    println!("extracted {} pages in {:.2}s", pages.len(), t0.elapsed().as_secs_f64());

    let low_conf = pages.iter().filter(|p| is_low_confidence(p, &config)).count();
    println!("low_confidence pages: {low_conf}/{}", pages.len());

    for p in pages.iter().take(3).skip(1) {
        println!(
            "page {} chars={} ocr_conf={:?} sample words: {:?}",
            p.page_no,
            p.char_count(),
            p.ocr_confidence,
            p.words.iter().take(8).map(|w| w.text.as_str()).collect::<Vec<_>>()
        );
    }
}
