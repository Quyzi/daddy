//! MCP (Model Context Protocol) server for a brain.
//!
//! Wraps [`brain_query`]'s retrieval as MCP tools (`explore`, `get`,
//! `page`, `stats`) served over stdio, so an MCP-aware client calls into
//! the graph directly rather than shelling out to the `brain` CLI. See
//! [`server::BrainServer`] for the tool implementations and [`run`] for
//! how to start one.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod server;

pub use server::BrainServer;

use anyhow::{Context, Result};
use std::path::Path;

/// Starts an MCP server for the brain at `root`, serving over stdio until
/// the client disconnects. Blocks the calling thread — callers typically
/// run this as the entire body of a dedicated `brain mcp` subcommand.
pub fn run(root: &Path) -> Result<()> {
    let runtime = tokio::runtime::Runtime::new().context("starting async runtime")?;
    runtime.block_on(run_async(root))
}

async fn run_async(root: &Path) -> Result<()> {
    use rmcp::ServiceExt;

    let server = BrainServer::open(root).context("opening brain for MCP server")?;
    let transport = rmcp::transport::io::stdio();
    let running = server.serve(transport).await.context("starting MCP server")?;
    running.waiting().await.context("MCP server loop")?;
    Ok(())
}
