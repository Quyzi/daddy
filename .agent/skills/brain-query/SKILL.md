---
name: second-brain-query
description: Answer a question against an existing second-brain knowledge base by querying its compiled graph (brain explore/get/page) instead of reading raw sources or wiki files directly. Use whenever the user asks a question about a brain's subject matter.
compatibility: Works with Claude Code, Cursor, Codex. Requires the `brain` CLI on PATH and a brain with `.brain/graph.db` already built (see the second-brain-compile skill).
---

# Second Brain — Query Skill

Querying a brain means asking its **graph**, not grepping its files. `brain explore`
already does full-text search + entity-name matching + graph-walk expansion + ranking in
one call, at zero token cost, and every result comes back pre-cited. Reading raw PDFs or
walking `wiki/*.md` by hand to answer a question is strictly worse: slower, more tokens,
and no ranking.

## Step 0: Locate the Knowledge Base

Same resolution as the compile skill: `$ARGUMENTS`/current directory/`brains/{name}`. If
`{root}/.brain/graph.db` doesn't exist, tell the user to run the second-brain-compile
skill first — there's nothing to query yet.

## Step 1: Query the Graph, Not the Files

For an open-ended question:

```bash
brain --root {root} explore "<the question or key terms>" --json
```

Add `--kind <entity-kind>` (e.g. `spell`, `monster`) when the question is scoped to one
kind of thing, and `--hops 1` to tighten a graph-walk that's pulling in too much tangential
material, or `--hops 3` to widen one that's coming back too narrow.

For a question about one specific, named thing ("what does Counterspell do", "Uridimmu's
stats"):

```bash
brain --root {root} get "<entity name>" --json
```

This skips ranking entirely and returns that entity's verbatim definition, every source's
value for each structured field, and its graph neighbours — more precise than `explore`
when you already know the name.

If a result's `citation` doesn't give enough surrounding context to answer confidently,
read that whole page verbatim rather than guessing:

```bash
brain --root {root} page "<document title>" <page number>
```

Only fall back to opening a file under `raw/` directly if all three of the above turn up
nothing — that usually means the source hasn't been ingested yet (check with `brain
--root {root} stats`) rather than that the graph is wrong.

## Step 2: Synthesize the Answer

1. Read the returned `items`/`fields`/`neighbours` (or the single entity's definition).
2. Synthesize an answer in your own words, keeping every `[Source: document p.N]`
   citation the tool attached — never drop a citation, never invent one that wasn't in the
   result.
3. Render the answer as whatever format fits: a paragraph, a comparison table (e.g.
   subclass differences), a build guide, a stat block, an encounter briefing.
4. If a contradiction shows up (an entity's `fields` list has the same key with two
   different documented values), surface both explicitly rather than picking one.

## Step 3: File It Back In (When It's Worth It)

If the synthesized answer is substantial and reusable — not a one-off — offer to promote
it into the wiki: either curate the relevant `wiki/{slug}.md` page directly (see the
second-brain-compile skill's Step 2), or write it to `outputs/` if it's more of a report
than an encyclopedia entry. If the question revealed a real gap (the graph has no entity
for something the human clearly cares about), say so and suggest what source would fill
it — that's a signal for what to add to `raw/` next, not something to guess at.

## Rules

- `brain explore`/`brain get`/`brain page` before any raw file, every time.
- Never fabricate a citation; only cite what the tool actually returned.
- A contradiction is a fact to report, not a coin flip to resolve silently.
