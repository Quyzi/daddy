//! Newtype identifiers for every row kind the graph stores.
//!
//! Keeping these as distinct types (rather than passing bare `i64`s around)
//! means a `DocId` can never be silently passed where a `ChunkId` was
//! expected — the compiler catches the mixup instead of SQLite returning
//! zero rows at 2am.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub i64);

        impl $name {
            /// Wraps a raw row id (typically a SQLite `INTEGER PRIMARY KEY`).
            pub const fn new(raw: i64) -> Self {
                Self(raw)
            }

            /// Returns the raw row id.
            pub const fn get(self) -> i64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<i64> for $name {
            fn from(raw: i64) -> Self {
                Self(raw)
            }
        }
    };
}

id_type!(DocId, "Identifies one ingested source document.");
id_type!(PageId, "Identifies one page within a document.");
id_type!(BlockId, "Identifies one layout block (heading/body/statblock/...) on a page.");
id_type!(SectionId, "Identifies one heading-derived section of a document.");
id_type!(ChunkId, "Identifies one retrieval chunk (roughly, one indexable passage).");
id_type!(EntityId, "Identifies one graph entity (a spell, monster, person, concept, ...).");
