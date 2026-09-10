# daddy

A Rust workspace implementing `brain` — a document-graph indexer for Karpathy's "second
brain" LLM wiki pattern — plus the Claude Code / Cursor / Codex **skills** built on top of
it, and the knowledge bases ("brains") those skills produce.

`brain` does the mechanical work of building a second brain (PDF/OCR extraction, layout
reconstruction, entity recognition, graph edges, wiki page generation) entirely in Rust, at
zero AI token cost — an AI agent's job becomes querying the graph and curating what it
produces, not reading every source PDF by hand.

## What's in here

```
daddy/
├── Cargo.toml                              ← workspace manifest
├── crates/
│   ├── brain-core/                         ← shared types, IDs, config, errors
│   ├── brain-extract/                      ← PDF/OCR/plain-text extraction + content-addressed cache
│   ├── brain-layout/                       ← pure-geometry page reconstruction (columns, headings, rejoin)
│   ├── brain-index/                        ← rule-pack recognition, chunking, gazetteer, edges, PageRank
│   ├── brain-store/                        ← SQLite (FTS5) schema + all SQL
│   ├── brain-query/                        ← ranked, cited, graph-walk retrieval
│   ├── brain-wiki/                         ← deterministic wiki page generation + lint
│   ├── brain-mcp/                          ← MCP server exposing explore/get/page/stats
│   └── brain-cli/                          ← the `brain` binary (clap)
├── packs/
│   ├── generic.toml                        ← headings-only entity recognizer, works for any brain
│   └── dnd5e.toml                          ← spell/monster/item recognizers
├── .agent/skills/
│   ├── brain/                              ← "second-brain" skill: scaffolds a new knowledge base
│   │   ├── SKILL.md
│   │   ├── README.md
│   │   └── references/starter-prompts.md   ← copy-paste prompts for every wiki workflow
│   ├── brain-compile/                      ← "second-brain-compile" skill: runs ingest/index/compile, then curates
│   │   └── SKILL.md
│   └── brain-query/                        ← "second-brain-query" skill: answers questions via `brain explore`/`get`
│       └── SKILL.md
├── brains/                                 ← gitignored; one subfolder per knowledge base
│   └── {name}/                             ← each brain is its own independent git repo
│       ├── CLAUDE.md                       ← that brain's schema + workflows
│       ├── .brain/                         ← gitignored within the brain repo; graph.db + extraction cache
│       ├── raw/                            ← immutable source material
│       ├── wiki/                           ← generated + curated interlinked pages (index.md, log.md, ...)
│       └── outputs/                        ← generated reports/briefings worth keeping
└── .gitignore                              ← ignores /target and brains/
```

Two things live in this one repo on purpose:
- A **Rust workspace** (`brain`) — build/run it like any cargo project.
- **Agent skills** for building and maintaining "second brain" knowledge bases on top of it. These skills aren't Rust code; they're markdown instructions an AI coding agent (Claude Code, Cursor, Codex, ...) reads and follows.

`brains/` is gitignored at this repo's level because each brain underneath it is its own git repository with its own history — they are not meant to be tracked as part of `daddy`.

## Using the `brain` CLI

```bash
cargo build --release
./target/release/brain --root brains/{name} init --pack dnd5e   # or `generic` for any other domain
./target/release/brain --root brains/{name} ingest               # extract + cache every file under raw/
./target/release/brain --root brains/{name} index                # rebuild entities/edges from the cache
./target/release/brain --root brains/{name} compile               # generate wiki/*.md
./target/release/brain --root brains/{name} explore "<question>"  # ranked, cited search
./target/release/brain --root brains/{name} get "<entity name>"   # verbatim entity lookup
./target/release/brain --root brains/{name} lint                  # contradictions + orphans
./target/release/brain --root brains/{name} mcp                   # MCP server over stdio
```

No unsafe code anywhere in the workspace (`#![forbid(unsafe_code)]` in every crate); every
crate is documented and unit/integration tested. See each crate's doc comments
(`cargo doc --workspace --no-deps --open`) for how the pipeline fits together.

## Using the second-brain skills

These are AI-agent skills, not CLI tools directly — you invoke them by asking your agent
to use them; the agent then drives the `brain` CLI underneath.

### 1. Create a new brain

> "Use the second-brain skill to set up a knowledge base about [topic]"

This scaffolds `brains/{topic}/` with `CLAUDE.md`, `raw/`, `wiki/`, `outputs/`, initializes it as its own git repo, and prints starter prompts. See `.agent/skills/brain/README.md` for full details and manual (non-agent) setup steps.

### 2. Add sources and compile the wiki

Drop source material (PDFs, articles, notes, transcripts, logs, etc.) into `brains/{topic}/raw/`, then:

> "Use the second-brain-compile skill to compile brains/{topic}"

This runs `brain ingest && brain index && brain compile` — extraction, OCR fallback,
layout, entity recognition, and first-draft wiki pages, all mechanical and free of AI
tokens — then curates the highest-value generated pages into real prose, cross-references
them, and flags contradictions `brain lint` surfaces.

### 3. Query and maintain

> "Use the second-brain-query skill to ask [question] about brains/{topic}"

The agent runs `brain explore`/`brain get` against the compiled graph (ranked, cited, zero
extra tokens) rather than grepping raw files or wiki pages by hand, then synthesizes an
answer with citations. Full workflow reference: `.agent/skills/brain/references/starter-prompts.md`.
