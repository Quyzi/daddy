//! `brain` — CLI entry point for the document-graph toolchain.
//!
//! This binary is deliberately thin: every real capability lives in a
//! library crate (`brain-extract`, `brain-layout`, `brain-index`,
//! `brain-store`, `brain-query`, `brain-wiki`) so it can also be driven
//! from `brain-mcp` or from tests without going through a subprocess.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod commands;
mod packs;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Build, query, and maintain a document knowledge graph.
#[derive(Parser)]
#[command(name = "brain", version, about)]
struct Cli {
    /// Root directory of the brain (containing `.brain/`, `raw/`, `wiki/`).
    /// Defaults to the current directory.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    /// Emit machine-readable JSON instead of human-formatted text, where
    /// the subcommand supports it.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new brain: creates `.brain/` and its database.
    Init {
        /// Rule pack to use for indexing (a name under `packs/`, e.g. `dnd5e`).
        #[arg(long, default_value = "generic")]
        pack: String,
    },
    /// Extract and cache text from source files under `raw/` (or given paths).
    Ingest {
        /// Specific files or directories to ingest; defaults to `raw/`.
        paths: Vec<PathBuf>,
        /// Number of parallel extraction workers.
        #[arg(short = 'j', long)]
        jobs: Option<usize>,
        /// Re-extract even if a cached/ingested copy already exists.
        #[arg(long)]
        force: bool,
        /// OCR behavior: auto, never, or always.
        #[arg(long)]
        ocr: Option<String>,
    },
    /// (Re)build sections, chunks, entities, and edges from cached extractions.
    Index {
        /// Rule pack to use; defaults to the brain's configured pack.
        #[arg(long)]
        pack: Option<String>,
    },
    /// Search the graph for relevant, cited passages.
    Explore {
        /// The natural-language or keyword query.
        query: String,
        /// Approximate token budget for the returned text.
        #[arg(long, default_value_t = 8000)]
        budget: usize,
        /// Number of graph hops to expand from seed matches.
        #[arg(long, default_value_t = 2)]
        hops: u32,
        /// Restrict results to one entity kind (e.g. `spell`, `monster`).
        #[arg(long)]
        kind: Option<String>,
    },
    /// Fetch one entity's definition, fields, and neighbours verbatim.
    Get {
        /// Entity name or slug.
        entity: String,
    },
    /// Print one page's reading-order text verbatim.
    Page {
        /// Document title or filename.
        doc: String,
        /// 1-based page number.
        page: u32,
    },
    /// Generate/update `wiki/*.md` from the current graph.
    Compile {
        /// Output directory for generated pages; defaults to `wiki/`.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Minimum entity centrality required to generate a page.
        #[arg(long, default_value_t = 0.0)]
        min_centrality: f64,
    },
    /// Report orphans, contradictions, and missing citations.
    Lint,
    /// Print summary statistics about the current graph.
    Stats,
    /// Run an MCP server over stdio, exposing explore/get/page/stats as
    /// tools for an MCP-aware AI client.
    Mcp,
}

fn main() -> Result<()> {
    // Every subcommand's log output goes to stderr, never stdout: `brain
    // mcp` reserves stdout entirely for the JSON-RPC transport (tracing's
    // default writer is stdout, which would otherwise interleave log
    // lines into that stream and break any MCP client's parser).
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Init { pack } => commands::init::run(&cli.root, &pack),
        Command::Ingest { paths, jobs, force, ocr } => {
            commands::ingest::run(&cli.root, &paths, jobs, force, ocr.as_deref())
        }
        Command::Index { pack } => commands::index::run(&cli.root, pack.as_deref()),
        Command::Explore { query, budget, hops, kind } => {
            commands::explore::run(&cli.root, &query, budget, hops, kind.as_deref(), cli.json)
        }
        Command::Get { entity } => commands::get::run(&cli.root, &entity, cli.json),
        Command::Page { doc, page } => commands::page::run(&cli.root, &doc, page),
        Command::Compile { out, min_centrality } => {
            commands::compile::run(&cli.root, out.as_deref(), min_centrality)
        }
        Command::Lint => commands::lint::run(&cli.root, cli.json),
        Command::Stats => commands::stats::run(&cli.root, cli.json),
        Command::Mcp => brain_mcp::run(&cli.root),
    }
}
