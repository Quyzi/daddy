//! SQLite-backed storage for the brain document graph.
//!
//! This crate owns the schema (`schema.rs`) and every piece of SQL in the
//! toolchain (`store.rs`). Downstream crates depend on this one and never
//! write raw SQL themselves — they call [`Store`]'s typed methods.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod schema;
pub mod store;

pub use store::{ContradictingValue, EntityField, FieldContradiction, FtsHit, Store};
