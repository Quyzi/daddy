# daddy

A Rust project scaffold that doubles as the home for a set of Claude Code / Cursor / Codex **skills** implementing Karpathy's "second brain" LLM wiki pattern, plus the knowledge bases ("brains") those skills produce.

## What's in here

```
daddy/
├── Cargo.toml / Cargo.lock / src/main.rs   ← Rust package "daddy" (currently a stub binary)
├── .agent/skills/
│   ├── brain/                              ← "second-brain" skill: scaffolds a new knowledge base
│   │   ├── SKILL.md
│   │   ├── README.md
│   │   └── references/starter-prompts.md   ← copy-paste prompts for every wiki workflow
│   └── brain-compile/                      ← "second-brain-compile" skill: ingests raw/ sources into wiki/
│       └── SKILL.md
├── brains/                                 ← gitignored; one subfolder per knowledge base
│   └── {name}/                             ← each brain is its own independent git repo
│       ├── CLAUDE.md                       ← that brain's schema + workflows
│       ├── raw/                            ← immutable source material
│       ├── wiki/                           ← LLM-maintained interlinked pages (index.md, log.md, ...)
│       └── outputs/                        ← generated reports/briefings worth keeping
└── .gitignore                              ← ignores /target and brains/
```

Two things live in this one repo on purpose:
- A **Rust binary** (`daddy`) — build/run it like any cargo project.
- **Agent skills** for building and maintaining "second brain" knowledge bases. These skills aren't Rust code; they're markdown instructions an AI coding agent (Claude Code, Cursor, Codex, ...) reads and follows.

`brains/` is gitignored at this repo's level because each brain underneath it is its own git repository with its own history — they are not meant to be tracked as part of `daddy`.

## Using the Rust project

```bash
cargo build
cargo run
```

Currently a hello-world stub (`src/main.rs`); extend it like any other cargo binary crate.

## Using the second-brain skills

These are AI-agent skills, not CLI tools — you invoke them by asking your agent to use them.

### 1. Create a new brain

> "Use the second-brain skill to set up a knowledge base about [topic]"

This scaffolds `brains/{topic}/` with `CLAUDE.md`, `raw/`, `wiki/`, `outputs/`, initializes it as its own git repo, and prints starter prompts. See `.agent/skills/brain/README.md` for full details and manual (non-agent) setup steps.

### 2. Add sources and compile the wiki

Drop source material (articles, notes, transcripts, logs, etc.) into `brains/{topic}/raw/`, then:

> "Use the second-brain-compile skill to compile brains/{topic}"

This reads each new source, files it into interlinked wiki pages under `brains/{topic}/wiki/`, updates the index, cross-links related pages, flags contradictions, and logs the ingest — without you having to retype the compile prompt each time.

### 3. Query and maintain

Ask questions directly against a brain's wiki (the agent reads `wiki/index.md`, follows `[[links]]`, and answers with citations), and periodically ask for a health/lint check to catch orphan pages, stale claims, and missing citations. Full workflow reference: `.agent/skills/brain/references/starter-prompts.md`.
</content>
