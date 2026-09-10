---
name: second-brain-compile
description: Compile or ingest new sources into an existing second-brain knowledge base (a folder with CLAUDE.md, raw/, wiki/, outputs/ scaffolded by the second-brain skill). Use when the user says compile, ingest, process, or update the wiki/knowledge base from raw sources.
compatibility: Works with Claude Code, Cursor, Codex. Requires the `brain` CLI (see the daddy repo's Rust workspace) on PATH.
---

# Second Brain — Compile Skill

Companion to the `second-brain` setup skill. Extraction, layout, chunking, entity
recognition, graph-building, and first-draft page generation are **the `brain` CLI's job,
not yours** — it processes an entire corpus in minutes at zero token cost. Your job is what
the CLI can't do: judgement. Reading a `status: generated` page and turning its bare quotes
into real prose. Deciding which contradiction is authoritative. Answering questions a rule
pack's regex-and-keyword recognizers were never going to catch.

If you find yourself about to open a PDF from `raw/` directly, stop — that almost always
means you should be running `brain ingest`/`brain explore` instead. Reading a raw source
directly is for the rare case a query result is missing surrounding context; it is not
routine.

## Step 0: Locate the Knowledge Base

- If `$ARGUMENTS` names a path (or a bare brain name), use it as the knowledge base root, resolving a bare name against `brains/{name}` first.
- Else, if the current directory itself contains `CLAUDE.md` + `raw/` + `wiki/`, use it as the root.
- Else, if `brains/` exists: if it has exactly one subfolder, use `brains/{that-subfolder}`; if it has several, ask which brain to compile (list the subfolder names).
- Else ask: "Which knowledge base folder should I compile? (the one containing CLAUDE.md, raw/, and wiki/)"
- Read `{root}/CLAUDE.md` in full. It is the source of truth for this knowledge base's schema — follow it exactly, even where it differs from the defaults below. Missing `CLAUDE.md` → stop and tell the user this folder wasn't scaffolded by the second-brain skill.
- If `{root}/.brain/` doesn't exist yet, the graph has never been built: run `brain --root {root} init --pack {pack}`, where `{pack}` is whatever `CLAUDE.md`'s Focus Areas suggest (e.g. a D&D-focused brain → `dnd5e`; anything else → `generic`; a fully custom domain can pass a path to a hand-written rule pack instead of a built-in name).

## Step 1: Run the Mechanical Pipeline

These three commands do the actual work — extraction, OCR fallback, layout
reconstruction, chunking, entity recognition, graph edges, and centrality, all
deterministic and all free of AI tokens:

```bash
brain --root {root} ingest   # extract + cache every new file under raw/ (skips already-ingested files automatically)
brain --root {root} index    # rebuild sections/chunks/entities/edges from what ingest cached
brain --root {root} compile  # generate/update wiki/*.md — never overwrites a page marked status: curated
```

Report what each step produced (`brain ingest` prints per-file page counts and a final
ingested/skipped/failed tally; `brain index` prints entity/edge counts; `brain compile`
prints written vs. skipped-as-curated). If `ingest` reports failures, show them — a
corrupted PDF or a missing `tesseract`/`pdftotext` binary needs a human, not a retry.

If nothing changed (`ingest` shows 0 ingested, all skipped), say so and stop — there's
nothing to compile.

## Step 2: Synthesis Pass (this is where your judgement actually matters)

`brain compile` just wrote or refreshed a batch of `status: generated` pages — bare
verbatim quotes with citations, no prose, no judgement. Don't try to synthesize all of
them; pick the ones worth it:

1. Read `wiki/index.md` to see everything `compile` produced this run.
2. Prioritize pages that are: high-centrality (the CLI already sorts `index.md` this
   way), central to what the human actually asked about, or flagged by `brain lint` (see
   Step 3).
3. For each page worth synthesizing: read it, run `brain --root {root} explore "<topic>"`
   and `brain --root {root} get "<related entity>"` for cross-referenced context instead of
   opening raw sources, then rewrite the page's prose in your own words — keep every
   `[Source: ...]` citation, keep the frontmatter fields, but make it read like a wiki page
   a person wrote, not a quote dump. Cross-link liberally (`[[other-page]]`).
4. Flip that page's frontmatter to `status: curated` when you're done. This is the *only*
   thing that protects your work: `brain compile` skips any page whose on-disk
   `status:` reads `curated`, forever, until someone changes it back — a re-run of `brain
   compile` will never clobber it.
5. Discuss key takeaways with the human as you go (1-3 sentences per page) — this is their
   chance to redirect what matters, same as the old per-source ingest flow, just now
   scoped to curation instead of raw reading.

A page you don't get to stays `status: generated` — that's fine, it's still a real,
cited, searchable page. It just reads like quotes until someone curates it.

## Step 3: Health Check

Run `brain --root {root} lint`. It reports two things mechanically:

- **Contradictions**: an entity where two or more source documents state a different
  value for the same field (e.g. an errata'd hit-point total). For any contradiction that
  touches a page you're curating, write both claims into the prose explicitly, citing
  both sources, and note which is more recent/authoritative — don't silently pick one.
- **Orphans**: entities with no edges and no recorded mentions anywhere — usually a
  low-value fragment, occasionally a real concept nothing else happens to reference yet.

`brain lint` doesn't check "stale claims" or "concepts mentioned but never explained" —
those need your judgement over the compiled wiki, not a regex. If you have context on
either, mention it in your report.

## Step 4: Report

- Show the current `wiki/index.md`.
- State what `ingest`/`index`/`compile` reported (Step 1).
- List which pages you curated this run (Step 2) and any contradictions you resolved in
  prose (Step 3).
- List any remaining contradictions/orphans you didn't get to.

## Rules

- Never modify files under `raw/` — they are immutable.
- Never hand-write a wiki page from scratch when `brain compile` would generate a correct
  skeleton for it — run the pipeline first, curate what it produces.
- Never overwrite a `status: curated` page yourself without cause — that status exists so
  curation work survives every future `brain compile` run, including your own past work.
- `{root}/CLAUDE.md` always wins over this skill's defaults — it's the per-knowledge-base
  contract; this skill is just the reusable trigger for running it.
