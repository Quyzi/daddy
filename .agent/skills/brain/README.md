# Second Brain

A plug-and-play setup skill for Karpathy's LLM Wiki pattern. Scaffolds a knowledge base that compounds instead of resetting.

---

## What's in here

```
second-brain/
├── SKILL.md                         ← the setup skill (run this in Claude Code / Cursor / Codex)
├── references/
│   └── starter-prompts.md           ← copy-paste prompts for every workflow
└── README.md                        ← you are here
```

## How to use it

### Option 1 — Claude Code

1. Drop this folder into `~/.claude/skills/` (or your project's `.claude/skills/`).
2. In Claude Code, say: `use the second-brain skill to set up a knowledge base about [your topic]`
3. Claude will ask 3-4 questions, then scaffold the full folder structure with `CLAUDE.md`, `wiki/index.md`, and `wiki/log.md`.
4. Start dropping sources into `raw/` and run the compile prompt from `references/starter-prompts.md`.

### Option 2 — Any AI tool (Cursor, Codex, ChatGPT, etc.)

1. Open `SKILL.md` and paste the contents into your AI assistant.
2. Say: "Follow this skill to set me up."
3. Answer the questions. The AI creates the folders and schema.
4. Use `references/starter-prompts.md` as your prompt library going forward.

### Option 3 — Just do it manually

Read `SKILL.md` top to bottom. It's a step-by-step recipe. Follow the steps yourself:

```bash
mkdir -p my-knowledge-base/{raw/assets,wiki,outputs}
cd my-knowledge-base
git init && git add . && git commit -m "setup"
```

Then copy the `CLAUDE.md` template from Step 3 of `SKILL.md` into the root of your new folder, fill in `{focus}` and `{interests}`, and you're ready.

---

## The core idea

Most AI tools re-derive answers from scratch every query. This is different. The LLM reads your raw sources, compiles them into an interlinked wiki once, then maintains that wiki as new sources arrive. Every new source updates multiple pages. Every good answer gets filed back in. Knowledge compounds.

You curate sources. The LLM does the bookkeeping.

## The four use cases this is built for

1. **Stakeholder memory** — a page per person, fed by meeting notes
2. **Side project continuity** — handoffs that survive the week
3. **Team onboarding** — institutional knowledge that outlasts departures
4. **Debugging memory** — solutions you solved once, findable next time

Every starter prompt in `references/starter-prompts.md` maps to one of these.

## The 30-day target

50-80 interconnected pages built from your own work. If you're behind that pace, ingest one source at a time (not in batches), file more answers back in, and run a health check to surface gaps.

---

Built on [Karpathy's LLM Wiki gist](https://gist.github.com/karpathy).
