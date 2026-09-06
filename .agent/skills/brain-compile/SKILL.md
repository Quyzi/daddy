---
name: second-brain-compile
description: Compile or ingest new sources into an existing second-brain knowledge base (a folder with CLAUDE.md, raw/, wiki/, outputs/ scaffolded by the second-brain skill). Use when the user says compile, ingest, process, or update the wiki/knowledge base from raw sources.
compatibility: Works with Claude Code, Cursor, Codex.
---

# Second Brain — Compile Skill

Companion to the `second-brain` setup skill. Runs the compile/ingest workflow against an already-scaffolded knowledge base so the human never has to retype the compile prompt.

## Step 0: Locate the Knowledge Base

- If `$ARGUMENTS` names a path (or a bare brain name), use it as the knowledge base root, resolving a bare name against `brains/{name}` first.
- Else, if the current directory itself contains `CLAUDE.md` + `raw/` + `wiki/`, use it as the root.
- Else, if `brains/` exists: if it has exactly one subfolder, use `brains/{that-subfolder}`; if it has several, ask which brain to compile (list the subfolder names).
- Else ask: "Which knowledge base folder should I compile? (the one containing CLAUDE.md, raw/, and wiki/)"
- Read `{root}/CLAUDE.md` in full. It is the source of truth for this knowledge base's schema and Ingest Workflow — follow it exactly, even where it differs from the defaults below. Missing `CLAUDE.md` → stop and tell the user this folder wasn't scaffolded by the second-brain skill.

## Step 1: Determine Scope

- If `$ARGUMENTS` names a specific file under `raw/`, scope = that one file.
- Else scope = every file in `raw/` (recursing into subfolders, skipping `raw/assets/`) whose filename has no matching `ingest` entry in `wiki/log.md`. Never re-ingest an already-logged source unless the user explicitly asks for a re-ingest/refresh.
- Empty scope → report "nothing new to compile" and stop.

## Step 2: Ingest Each Source

Process sources **one at a time**, oldest/most-foundational first, following the Ingest Workflow in `CLAUDE.md` (defaults below if the KB's `CLAUDE.md` doesn't override them):

1. Read the source fully.
2. Briefly surface key takeaways (1-3 sentences) before filing anything — this is the human's chance to redirect what matters.
3. Create or update a summary page in `wiki/` (frontmatter: title, created, last_updated, source_count, status).
4. Update `wiki/index.md` with the new/changed page.
5. Update every other existing wiki page this source touches — a single source should usually move more than one page.
6. Add backlinks (`[[page-name]]`) from existing pages to the new content.
7. Flag contradictions with existing wiki content explicitly: note both claims, cite both sources ([Source: filename.md]), and flag which is more recent/authoritative. Never silently overwrite.
8. Append `## [YYYY-MM-DD] ingest | {filename} — {one-line takeaway}` to `wiki/log.md`.

After each source, state which pages were created/updated before moving to the next.

## Step 3: Report

Once scope is exhausted:
- Show the current `wiki/index.md`.
- State how many sources were ingested this run and which pages they touched.
- List any contradictions or gaps flagged during ingest.
- Check `wiki/log.md` for the last `lint` entry; if it's stale or absent, suggest running a health check (per `CLAUDE.md`'s Lint Workflow).

## Rules

- Never modify files under `raw/` — they are immutable.
- Never batch-write summaries without reading each source fully first.
- `{root}/CLAUDE.md` always wins over this skill's defaults — it's the per-knowledge-base contract; this skill is just the reusable trigger for running it.
</content>
