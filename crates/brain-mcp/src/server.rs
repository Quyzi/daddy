//! `BrainServer`: exposes `brain-query`'s retrieval as MCP tools over
//! stdio, so an MCP-aware AI client can call `explore`/`get`/`page`/
//! `stats` directly instead of shelling out to the `brain` CLI.
//!
//! One server instance holds one brain's [`Store`] behind a mutex —
//! `rusqlite::Connection` isn't `Sync`, and a brief lock per tool call is
//! immaterial next to how fast SQLite reads are for this workload.
//!
//! Every tool returns plain JSON-encoded text (`String`) rather than
//! `rmcp`'s `Json<T>` wrapper: `Json<T>` requires a concrete JSON Schema
//! for MCP's `outputSchema`, which a dynamically-shaped
//! `serde_json::Value` result can't provide — the framework panics at
//! startup ("Schema is missing 'type' field") trying to derive one. A
//! plain string return sidesteps that requirement entirely (unstructured
//! text content, no output schema needed) while still handing the client
//! fully-formed JSON to parse.

use brain_core::error::Result as BrainResult;
use brain_core::BrainConfig;
use brain_query::{explore as run_explore, get_entity, ExploreOptions};
use brain_store::Store;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Mutex;

fn default_budget() -> usize {
    8000
}
fn default_hops() -> u32 {
    2
}

fn to_json_string(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| error_json(&format!("serializing result: {e}")))
}

fn error_json(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// Parameters for the `explore` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExploreParams {
    /// The search query (a question, a name, or free-text keywords).
    pub query: String,
    /// Approximate token budget for the returned text.
    #[serde(default = "default_budget")]
    pub budget: usize,
    /// Number of graph hops to expand from seed matches.
    #[serde(default = "default_hops")]
    pub hops: u32,
    /// Restrict results to one entity kind (e.g. `"spell"`, `"monster"`).
    #[serde(default)]
    pub kind: Option<String>,
}

/// Parameters for the `get` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetParams {
    /// Entity name or slug.
    pub entity: String,
}

/// Parameters for the `page` tool.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PageParams {
    /// Document title (as shown by `stats`/`brain stats`).
    pub doc: String,
    /// 1-based page number.
    pub page: u32,
}

/// The MCP server for one brain.
pub struct BrainServer {
    store: Mutex<Store>,
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

#[tool_router]
impl BrainServer {
    /// Opens the brain rooted at `root`.
    pub fn open(root: &std::path::Path) -> BrainResult<Self> {
        let db_path = BrainConfig::db_path(root);
        let store = Store::open(&db_path)?;
        Ok(Self { store: Mutex::new(store), tool_router: Self::tool_router() })
    }

    /// Searches the graph for relevant, cited passages.
    #[tool(
        description = "Search the brain's document graph for relevant, cited passages. Combines full-text search with graph-walk expansion; results include [Source: document p.N] citations. Prefer this over reading raw source files directly. Returns JSON."
    )]
    fn explore(&self, Parameters(params): Parameters<ExploreParams>) -> String {
        let store = self.store.lock().expect("store mutex poisoned");
        let opts = ExploreOptions {
            budget_tokens: params.budget,
            hops: params.hops,
            kind: params.kind,
            ..Default::default()
        };
        match run_explore(&store, &params.query, &opts) {
            Ok(result) => to_json_string(&result),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Fetches one entity's verbatim definition, fields, and neighbours.
    #[tool(
        description = "Fetch one entity's verbatim definition, structured fields (with per-source values), and graph neighbours by exact name or slug. Returns JSON."
    )]
    fn get(&self, Parameters(params): Parameters<GetParams>) -> String {
        let store = self.store.lock().expect("store mutex poisoned");
        match get_entity(&store, &params.entity) {
            Ok(Some(view)) => to_json_string(&view),
            Ok(None) => error_json(&format!("no entity found matching {:?}", params.entity)),
            Err(e) => error_json(&e.to_string()),
        }
    }

    /// Prints one document page's reading-order text verbatim.
    #[tool(
        description = "Print one document page's reading-order text verbatim. The escape hatch when a query result needs more surrounding context: go read the whole page."
    )]
    fn page(&self, Parameters(params): Parameters<PageParams>) -> String {
        let store = self.store.lock().expect("store mutex poisoned");
        let doc = match store.find_document_by_title(&params.doc) {
            Ok(Some(d)) => d,
            Ok(None) => return error_json(&format!("no document found matching {:?}", params.doc)),
            Err(e) => return error_json(&e.to_string()),
        };
        let doc_id = doc.id.expect("documents read back from storage always have an id");
        match store.get_page(doc_id, params.page) {
            Ok(page) => page.text,
            Err(_) => error_json(&format!(
                "page {} not found ({:?} has {} pages)",
                params.page, doc.title, doc.page_count
            )),
        }
    }

    /// Reports summary counts for this brain.
    #[tool(description = "Summary counts for this brain: documents, pages, and entities. Returns JSON.")]
    fn stats(&self) -> String {
        let store = self.store.lock().expect("store mutex poisoned");
        let docs = store.list_documents().unwrap_or_default();
        let total_pages: u32 = docs.iter().map(|d| d.page_count).sum();
        let entities = store.list_entities(None).unwrap_or_default();
        to_json_string(&json!({ "documents": docs.len(), "pages": total_pages, "entities": entities.len() }))
    }
}

#[tool_handler]
impl ServerHandler for BrainServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "Query a local document knowledge graph built by the `brain` CLI. Use `explore` for \
                 open-ended search, `get` for a known entity's exact definition, `page` to read a \
                 source page verbatim, and `stats` for corpus size."
                    .to_string(),
            ),
            ..Default::default()
        }
    }
}
