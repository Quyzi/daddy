# Starter Prompts for Your Second Brain

Copy-paste these into your AI agent. They follow the schema in CLAUDE.md.

---

## Core Operations

### Compile (Run This First)

After dropping sources into raw/:

> Read CLAUDE.md. Then process all files in raw/ sequentially. For each
> source: read it fully, create a summary page in wiki/, update index.md,
> update all relevant existing wiki pages, add backlinks, flag contradictions,
> and log the ingest. Start by showing me the updated index.md so I can see
> the shape of what you built.

### Ingest One Source (Use Every Time You Add Something New)

> Read CLAUDE.md. Process raw/{filename}. Read it fully, discuss key
> takeaways with me, then update the wiki: summary page, index, all
> relevant existing pages, backlinks, contradictions, log. Show me
> which pages you created or updated.

Start one source at a time. Read the summaries. Guide the AI on what
matters. This produces dramatically better results than batch processing.

### Scrape a URL

> Scrape [URL] into raw/. Extract the main content and save it as a
> markdown file with a descriptive filename.

Requires agent-browser. Install: npm install -g agent-browser && agent-browser install

### Health Check

> Run a full health check on wiki/ per the lint workflow in CLAUDE.md.
> Output to wiki/lint-report-[date].md. Flag contradictions, stale claims,
> orphan pages, missing cross-references, and claims without source
> citations. Suggest 3 sources that would fill the biggest gaps.

### Explore Connections

> Read wiki/index.md and identify the 5 most interesting unexplored
> connections between existing topics. For each, explain what insight
> it might reveal and what source would help confirm it. If any
> connections are strong enough, create new wiki pages or add
> cross-references and update the index.

---

## Use Case 1 — Stakeholder Memory

The wiki ends up knowing your CTO (or co-founder, manager, key customer) better than most new employees do after their first month. Drop every meeting note, Slack thread, and email thread into raw/. The wiki builds a page per stakeholder.

### Build or Update a Stakeholder Page

> Read raw/{meeting-or-thread-filename}. Update the stakeholder page for
> [Name] in wiki/. Add: what they care about, what they've pushed back on,
> what language has landed, what they've approved and why. Cite sources.
> If no page exists yet, create one.

### Pre-Meeting Prep

> Pull the stakeholder page for [Name]. What have they pushed back on
> before? What framing has landed with them? What should I lead with
> tomorrow? Flag anything where my read is based on a single source.

### Stakeholder Signals Over Time

> Based on every source in the wiki mentioning [Name], how has their
> position on [topic] evolved? What changed, and what triggered each
> shift?

---

## Use Case 2 — Side Project Continuity

You work on your main job five days a week. The side project gets weekends. Drop planning notes, decisions, and TODOs into raw/ after every session. Write a brief "where I left off" note before you close the laptop.

### End-of-Session Handoff

> I'm wrapping up a session on [project]. Here's what I did today:
> [paste or dictate]. Save this as raw/{project}-handoff-{date}.md,
> then update the wiki pages for this project with: current state,
> what's done, what's next, any open questions or blockers.

### Saturday-Morning Resume

> Where did I leave off on [project]? What was I trying to solve and
> what's the next step? Pull from the most recent handoff and project
> pages. Give me a 5-minute briefing so I can start building.

### Decision Log

> I'm deciding between [option A] vs [option B] for [project]. Based
> on everything in the wiki about this project, what evidence supports
> each option? What assumptions am I making? File the analysis back
> into wiki/ so I can reference it later.

---

## Use Case 3 — Team Onboarding That Survives Departures

Institutional memory that usually walks out when someone leaves, written down somewhere the next person can actually find it. Feed the wiki Slack threads, meeting transcripts, project documents, customer calls, architecture decisions.

### Onboarding Briefing

> I'm a new person on this team. Based on the wiki, give me the
> 80/20 briefing I need to be useful by next week. Structure it as:
> who the key people are, what they each care about, what we're
> building and why, what decisions have already been made, and what
> the open debates are.

### Why Did We Do This?

> Why did we make the [architecture / product / hiring] decision
> about [topic]? Pull the full history from the wiki: who was
> involved, what alternatives we considered, what tipped the call.
> Cite sources.

### What Changed Since Last Quarter

> What has changed in our understanding of [topic] over the last
> 90 days? Which older wiki pages have been updated or contradicted
> by newer sources?

---

## Use Case 4 — Solutions You Solved Once and Forgot

You spent four hours debugging a gnarly issue last month. You moved on. Now it's back. Drop debugging notes into raw/ when you solve something non-obvious. The wiki builds a page per problem type.

### File a Debug Session

> Here's what I just debugged: [paste notes or terminal output].
> Save to raw/{descriptive-name}-{date}.md. Then update wiki/ with
> a page per problem type: symptoms, root cause, fix, what I ruled
> out, related problems. Cross-reference any existing debugging pages.

### Query Before You Debug

> Have I hit anything like [error message / symptom / behavior]
> before? What was the fix and what caused it? Check the wiki
> and related pages. If you find a match, point me at it. If not,
> tell me explicitly — so I know I'm breaking new ground.

### Post-Mortem Compiler

> Based on all the debugging pages in wiki/, what are the most
> common root causes? What classes of bug keep biting us? File
> the analysis back into wiki/ as a meta-page.

---

## Generic Query Prompts

Useful across all four use cases.

### Explore What You Know

> Read wiki/index.md. What are the 5 biggest gaps in this knowledge
> base? What do I think I know that's only backed by a single source?
> What should I research next to strengthen the weakest areas?

> What patterns keep showing up across multiple sources? What themes
> are emerging that I might not have noticed?

### Spot Contradictions and Staleness

> What claims in the wiki contradict each other? Which source is more
> recent or reliable for each contradiction?

> Which of my older wiki pages are most likely to be outdated based
> on newer sources? Prioritize what needs refreshing.

### Generate Shareable Outputs

> Based on everything in wiki/, write a 500-word briefing on
> [TOPIC]. Cite sources. Structure it as: current state, key tensions,
> open questions, recommended next steps. Save to outputs/.

> Write a summary of [topic] as a slide deck outline (Marp format).
> 5 slides max. Save to outputs/.

> Create a comparison table of [entities] across [dimensions]. Render
> as markdown. Save to outputs/.

---

## The Compounding Loop

The most important habit: file good answers back into the wiki.

> Save that answer as a new wiki page and update the index.

> File that analysis into wiki/ so I can reference it next time.

A comparison you asked for, an analysis, a briefing, a pattern you
spotted — these are all knowledge. If they stay in chat history, they
disappear. If they go back into the wiki, every future query gets smarter.

That's the whole system. Sources go in. Wiki gets compiled. You query it.
Good answers go back. Knowledge compounds.

---

## 30-Day Target

From the newsletter: after 30 days you should have 50-80 interconnected
pages built from your own work. Live pages per project, per person, per
problem type. A growing collection of answered questions that represent
your best thinking.

If you're behind that pace after a month, the fix is usually: ingest
more one-at-a-time (not in batches), file more answers back in, and
run a health check to surface the gaps.
