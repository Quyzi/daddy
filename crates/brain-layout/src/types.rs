//! Output types for the layout pipeline: a page reduced to ordered,
//! classified blocks of text, ready for `brain-store` to persist and
//! `brain-index` to chunk.

use brain_core::{BBox, BlockKind};
use serde::{Deserialize, Serialize};

/// One block of text on a page, already in final reading-order position
/// (`col`, `ord`) relative to the rest of the page's blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaidOutBlock {
    /// 0-based column index (0 = leftmost).
    pub col: u32,
    /// Position within the page's overall reading order.
    pub ord: u32,
    /// Bounding box of the block (union of its lines').
    pub bbox: BBox,
    /// Structural role. `brain-layout` only ever produces
    /// [`BlockKind::Heading`], [`BlockKind::Body`], and
    /// [`BlockKind::Chrome`] — finer classification (stat blocks, tables)
    /// is `brain-index`'s job, working over chunk text with rule-pack
    /// patterns rather than layout geometry.
    pub kind: BlockKind,
    /// The block's rejoined, whitespace-normalized text.
    pub text: String,
}

/// One page after full layout reconstruction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaidOutPage {
    /// 1-based page number.
    pub page_no: u32,
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Every non-chrome block, in final reading order.
    pub blocks: Vec<LaidOutBlock>,
}

impl LaidOutPage {
    /// The page's full reading-order text: non-chrome block text joined by
    /// blank lines. This is what `brain page <doc> <n>` prints and what
    /// `pages.text` stores.
    pub fn reading_order_text(&self) -> String {
        self.blocks
            .iter()
            .filter(|b| b.kind != BlockKind::Chrome)
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}
