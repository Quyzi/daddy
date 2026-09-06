---
name: second-brain
description: Scaffold a personal LLM knowledge base (second brain) with folders, schema, and starter prompts. Based on Karpathy's LLM Wiki pattern. Works for anybody — engineers, builders, researchers, PMs, writers, operators. Use when someone wants to build a knowledge base, personal wiki, or second brain with AI.
compatibility: Works with Claude Code, Cursor, Codex. Optional npm for agent-browser.
---

# Second Brain — Setup Skill

Scaffold a second brain that compounds every time you use it. Based on Karpathy's LLM Wiki pattern. 

**The core idea:** most AI tools (NotebookLM, ChatGPT uploads, RAG) re-derive answers from scratch every time you query them. This is different. The LLM reads your raw sources, compiles them into a structured interlinked wiki, and maintains that compiled knowledge over time. Every new source updates multiple wiki pages. Every good answer gets filed back in. Knowledge compounds instead of resetting.

The human curates sources, directs analysis, and asks the right questions. The LLM does the summarizing, cross-referencing, filing, and bookkeeping.

**Who this works for:** engineers debugging the same class of problem twice, builders juggling side projects, operators tracking stakeholders, researchers connecting papers, writers collecting references, PMs building market knowledge, anyone onboarding a team. The schema is generic. The four use cases in the newsletter all map onto it.

## Backend Code Requirements

1. Must be written in rust.  
2. Must be able to compile to at least x86_64, aarch64, apple. 
3. Must not use unsafe code. 

## Step 1: Determine Focus

**If `$ARGUMENTS` is provided:** Use it as the focus area. Slugify for the folder name (lowercase, replace spaces with hyphens, remove special characters). Skip to Step 2.

**If no arguments:** Ask the user these questions one at a time:

1. "What do you want to build a knowledge base around? (a domain, a project, a role, a market, a research area — anything you're accumulating knowledge about)"
2. "What are 3-5 things you're always trying to stay on top of within that area? (people, problems, technologies, competitors, questions)"
3. "What kind of sources will you be feeding this? (articles, meeting notes, Slack threads, papers, debugging logs, transcripts, docs)"
4. "Do you have any URLs you'd like to scrape into your knowledge base right now? (optional, you can always add more later)"

## Step 2: Scaffold the Knowledge Base

Create the folder structure in the current working directory:

```bash
mkdir -p {slug}/raw/assets {slug}/wiki {slug}/outputs
```

**Recommended setup (optional but valuable):**
- Open this folder in **Obsidian** for the best viewing experience. The [[links]] become clickable navigation, and the graph view shows you the shape of your wiki at a glance (which pages are hubs, which are orphans). Any text editor works, but Obsidian is purpose-built for this.
- Initialize a **git repo** (`git init && git add . && git commit -m "setup"`). Your wiki is just markdown files. Git gives you full version history, the ability to undo anything the AI messes up, and collaboration for free.
- **Scale note:** The index-file approach works well up to ~100 sources and a few hundred wiki pages. Beyond that, consider adding a search tool like [qmd](https://github.com/tobi/qmd) (local markdown search with hybrid BM25/vector search, works as CLI and MCP server).

## Step 3: Generate the Schema File

Write `{slug}/CLAUDE.md` with the following content. Replace `{focus}` with the user's focus area, `{interests}` with their listed interests, and `{source_types}` with the source types they named:

```markdown
# Knowledge Base Schema

## What This Is
A personal knowledge base about {focus}. Maintained by an LLM. The human curates sources and asks questions. The LLM writes and maintains the entire wiki.

## Architecture
- raw/ contains source documents (articles, notes, docs, transcripts, images, debugging logs, meeting notes). These are immutable. Never modify files in raw/.
- raw/assets/ contains images, screenshots, and diagrams referenced by sources.
- wiki/ contains the compiled wiki. The LLM owns this entirely. It creates pages, updates them when new sources arrive, maintains cross-references, and keeps everything consistent.
- outputs/ contains generated reports, briefings, analyses, and answers worth keeping. When an output is valuable enough to inform future queries, promote it into wiki/ as a new page. Your explorations and queries should always add up in the knowledge base.

## Wiki Conventions
- Every wiki page gets its own .md file in wiki/.
- Every page starts with YAML frontmatter (title, created, last_updated, source_count, status) and a one-paragraph summary.
- Use [[page-name]] links between wiki pages. Cross-reference aggressively. The connections between pages are as valuable as the pages themselves.
- Every factual claim cites its source: [Source: filename.md]
- When new information contradicts existing content, flag it explicitly. Don't silently overwrite. Note both claims, cite both sources, flag which is more recent.

## Index and Log
- wiki/index.md catalogs every page with a one-line description, organized by category. The LLM updates this on every ingest. When answering queries, read the index first to find relevant pages, then drill into them.
- wiki/log.md is an append-only chronological record. Format: `## [YYYY-MM-DD] action | Description` where action is ingest, query, lint, or update. Parseable with grep.

## Ingest Workflow
When I add a new source to raw/:
1. Read the full source
2. Discuss key takeaways with me
3. Create a summary page in wiki/
4. Update wiki/index.md
5. Update ALL relevant existing pages across the wiki (a single source should touch multiple pages)
6. Add backlinks from existing pages to new content
7. Flag any contradictions with existing wiki content
8. Append entry to wiki/log.md

Process one source at a time. Batch ingesting produces worse results — you lose the ability to guide what matters.

## Query Workflow
When I ask a question:
1. Read wiki/index.md to find relevant pages
2. Read those pages, following [[links]] for connected context
3. Synthesize an answer with citations to wiki pages
4. Render answers as the format that fits best: a wiki page, a comparison table, a briefing, a slide deck (Marp), or a visualization
5. If the answer is substantial, offer to file it back into wiki/ so it compounds
6. If the question reveals a gap, flag it and suggest what sources would fill it

## Explore Workflow
Periodically, find unexplored connections:
1. Read wiki/index.md and scan all pages for topics that appear in multiple contexts but aren't directly linked
2. Identify the most interesting unexplored connections
3. For each, explain what insight it might reveal and what source would confirm it
4. Offer to create new pages or add cross-references for confirmed connections

## Lint Workflow
Periodically check for:
- Contradictions between pages
- Stale claims that newer sources have superseded
- Orphan pages with no inbound [[links]]
- Important concepts mentioned but never explained in their own page
- Missing cross-references
- Claims without source citations
Output: wiki/lint-report-[date].md

## Focus Areas
- {interest 1}
- {interest 2}
- {interest 3}

## Expected Source Types
{source_types}
```

## Step 4: Create INDEX.md and LOG.md

Write `{slug}/wiki/index.md`:

```markdown
# Wiki Index

_Last updated: {today's date}_

Maintained by the LLM. Every page listed with a one-line summary.

_(No pages yet. Add sources to raw/ and run the compile prompt to build the wiki.)_
```

Write `{slug}/wiki/log.md`:

```markdown
# Wiki Log

Append-only record of all wiki activity.

## [{today's date}] init | Knowledge base scaffolded
Focus: {focus}. Interests: {interests}. Ready for first sources.
```

## Step 5: Scrape Initial URLs (If Provided)

If the user provided URLs in the interactive flow:

For each URL:
1. Check if agent-browser is available: `which agent-browser`
2. If not available, try to install: `npm install -g agent-browser && agent-browser install`
3. If install fails, fall back to web fetch or tell the user to manually save the page content into raw/
4. If agent-browser is available:
   - Open the page: `agent-browser open {url}`
   - Wait for content: `agent-browser wait --load networkidle`
   - Extract the main content: `agent-browser get text "article"` (fall back to `agent-browser get text "main"`, then `agent-browser get text "body"`)
   - Get the page title: `agent-browser get title`
   - Save as `{slug}/raw/{slugified-title}.md` with the extracted text
   - Close when done: `agent-browser close`

Print: "Scraped {n} sources into raw/."

## Step 6: Print Starter Prompts

Read `${CLAUDE_SKILL_DIR}/references/starter-prompts.md` and print the full contents so the user can see all prompts.

## Step 7: Print Next Steps

Print:

```
Your knowledge base is ready at: {slug}/

Here's how to start:

1. Drop sources into raw/. Don't organize them. That's the LLM's job.
   Articles, notes, docs, transcripts, screenshots, meeting notes,
   debugging logs, Slack threads — anything relevant.

2. Run the compile prompt to build your wiki. The LLM reads everything,
   creates interlinked pages, and builds the index. One source can touch
   10-15 pages.

3. Ask questions against the wiki. Good answers get filed back in, so
   your explorations compound just like your sources do.

4. Run a health check periodically to catch contradictions and gaps.

5. (Optional) Install agent-browser for automated web scraping:
   npm install -g agent-browser && agent-browser install
   Then: "scrape [URL] into raw/" and the LLM handles the rest.

The four use cases from the newsletter that map onto this:
  - Stakeholder memory (a page per person, fed by meeting notes)
  - Side project continuity (drop a "where I left off" note each session)
  - Team onboarding (institutional knowledge that survives departures)
  - Solutions you solved once (debugging notes → searchable next time)

All prompts are in the starter-prompts reference above.
```
