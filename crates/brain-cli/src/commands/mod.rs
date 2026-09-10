//! One module per CLI subcommand, kept separate so each can grow (and be
//! tested) independently of `clap` parsing in `main.rs`.

pub mod compile;
pub mod explore;
pub mod get;
pub mod index;
pub mod ingest;
pub mod init;
pub mod lint;
pub mod page;
pub mod stats;
