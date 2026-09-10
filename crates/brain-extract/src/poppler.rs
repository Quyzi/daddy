//! Native-PDF-text extraction backend, built on poppler's `pdftotext` and
//! `pdfinfo` command-line tools (invoked as subprocesses — see the design
//! rationale in the top-level implementation plan).

use crate::types::{Capability, Extractor, PageRange};
use brain_core::error::{BrainError, Result};
use brain_core::{BBox, RawPage, Word};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Extracts positioned words from PDFs using poppler's `pdftotext
/// -bbox-layout`, which emits an XHTML document with per-word bounding
/// boxes — see the corpus probing notes in the implementation plan for why
/// this beats plain `-layout` text.
pub struct PopplerExtractor {
    pdftotext_bin: PathBuf,
    pdfinfo_bin: PathBuf,
}

impl PopplerExtractor {
    /// Locates `pdftotext` and `pdfinfo` on `PATH`. Returns an error
    /// (rather than panicking) if either is missing, since this backend is
    /// a runtime subprocess dependency, not a linked library.
    pub fn new() -> Result<Self> {
        let pdftotext_bin = which::which("pdftotext")
            .map_err(|_| BrainError::Extraction("pdftotext not found on PATH (install poppler-utils)".into()))?;
        let pdfinfo_bin = which::which("pdfinfo")
            .map_err(|_| BrainError::Extraction("pdfinfo not found on PATH (install poppler-utils)".into()))?;
        Ok(Self { pdftotext_bin, pdfinfo_bin })
    }
}

impl Extractor for PopplerExtractor {
    fn name(&self) -> &'static str {
        "poppler"
    }

    fn probe(&self, path: &Path) -> Result<Capability> {
        let is_pdf = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("pdf"))
            .unwrap_or(false);
        if !is_pdf {
            return Ok(Capability {
                supported: false,
                page_count: None,
                note: Some("not a .pdf file".into()),
            });
        }
        match crate::pdfutil::page_count(&self.pdfinfo_bin, path)? {
            Some(n) => Ok(Capability { supported: true, page_count: Some(n), note: None }),
            None => Ok(Capability {
                supported: false,
                page_count: None,
                note: Some("pdfinfo could not read this file".into()),
            }),
        }
    }

    fn extract(&self, path: &Path, pages: PageRange) -> Result<Vec<RawPage>> {
        let mut cmd = Command::new(&self.pdftotext_bin);
        cmd.arg("-bbox-layout");
        let start_page = match pages {
            PageRange::All => 1,
            PageRange::Range(start, end) => {
                cmd.arg("-f").arg(start.to_string());
                cmd.arg("-l").arg(end.to_string());
                start
            }
        };
        cmd.arg(path).arg("-");
        let output = cmd
            .output()
            .map_err(|e| BrainError::Extraction(format!("running pdftotext: {e}")))?;
        if !output.status.success() {
            return Err(BrainError::Extraction(format!(
                "pdftotext failed on {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        parse_bbox_xml(&output.stdout, start_page)
    }
}

/// Parses `pdftotext -bbox-layout` XHTML output into flat per-page word
/// lists. Block/line grouping in the source XML is deliberately discarded
/// here — poppler's own grouping isn't reliable for the multi-column,
/// stat-block-interleaved layouts this corpus contains, so `brain-layout`
/// rebuilds structure from word geometry alone.
fn parse_bbox_xml(xml: &[u8], start_page: u32) -> Result<Vec<RawPage>> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut pages = Vec::new();
    let mut current: Option<RawPage> = None;
    let mut page_no = start_page;

    // Pending word state while inside a <word ...>...</word> element.
    let mut in_word = false;
    let mut word_bbox: Option<BBox> = None;
    let mut word_text = String::new();

    let mut buf = Vec::new();
    loop {
        match reader
            .read_event_into(&mut buf)
            .map_err(|e| BrainError::Extraction(format!("parsing pdftotext bbox output: {e}")))?
        {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if name == b"page" {
                    let (mut width, mut height) = (0.0f64, 0.0f64);
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"width" => width = parse_f64_attr(&attr.value),
                            b"height" => height = parse_f64_attr(&attr.value),
                            _ => {}
                        }
                    }
                    current = Some(RawPage {
                        page_no,
                        width,
                        height,
                        words: Vec::new(),
                        ocr_confidence: None,
                    });
                } else if name == b"word" {
                    let mut bbox = BBox { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"xMin" => bbox.x0 = parse_f64_attr(&attr.value),
                            b"yMin" => bbox.y0 = parse_f64_attr(&attr.value),
                            b"xMax" => bbox.x1 = parse_f64_attr(&attr.value),
                            b"yMax" => bbox.y1 = parse_f64_attr(&attr.value),
                            _ => {}
                        }
                    }
                    in_word = true;
                    word_bbox = Some(bbox);
                    word_text.clear();
                }
            }
            Event::Text(t) => {
                if in_word {
                    let decoded = t
                        .unescape()
                        .map_err(|e| BrainError::Extraction(format!("decoding word text: {e}")))?;
                    word_text.push_str(&decoded);
                }
            }
            Event::End(e) => {
                let local = e.local_name();
                let name = local.as_ref();
                if name == b"word" {
                    if in_word {
                        if let Some(bbox) = word_bbox.take() {
                            if !word_text.trim().is_empty() {
                                if let Some(page) = current.as_mut() {
                                    page.words.push(Word {
                                        text: word_text.trim().to_string(),
                                        bbox,
                                        confidence: None,
                                    });
                                }
                            }
                        }
                    }
                    in_word = false;
                } else if name == b"page" {
                    if let Some(page) = current.take() {
                        pages.push(page);
                        page_no += 1;
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(pages)
}

fn parse_f64_attr(raw: &[u8]) -> f64 {
    std::str::from_utf8(raw)
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<html><body><doc>
<page width="612.000000" height="792.000000">
<flow><block xMin="10" yMin="10" xMax="200" yMax="30">
<line xMin="10" yMin="10" xMax="200" yMax="20">
<word xMin="10" yMin="10" xMax="30" yMax="20">Hello</word>
<word xMin="35" yMin="10" xMax="60" yMax="20">world</word>
</line>
</block></flow>
</page>
<page width="612.000000" height="792.000000">
<flow><block xMin="10" yMin="10" xMax="200" yMax="30">
<line xMin="10" yMin="10" xMax="200" yMax="20">
<word xMin="10" yMin="10" xMax="30" yMax="20">Second</word>
</line>
</block></flow>
</page>
</doc></body></html>"#;

    #[test]
    fn parses_two_pages_with_words_in_order() {
        let pages = parse_bbox_xml(SAMPLE.as_bytes(), 1).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].page_no, 1);
        assert_eq!(pages[0].width, 612.0);
        assert_eq!(pages[0].words.len(), 2);
        assert_eq!(pages[0].words[0].text, "Hello");
        assert_eq!(pages[0].words[0].bbox.x0, 10.0);
        assert_eq!(pages[0].words[1].text, "world");
        assert_eq!(pages[1].page_no, 2);
        assert_eq!(pages[1].words[0].text, "Second");
    }

    #[test]
    fn respects_a_nonzero_start_page_for_ranged_extraction() {
        let pages = parse_bbox_xml(SAMPLE.as_bytes(), 239).unwrap();
        assert_eq!(pages[0].page_no, 239);
        assert_eq!(pages[1].page_no, 240);
    }
}
