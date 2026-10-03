---
id: 0031
title: "Document templates for the delivery lifecycle: eleven kinds built in, overridable per workspace, placed by stage, and taught to agents"
status: accepted
date: 2026-10-03
supersedes: null
---

## Context

Teams using DIT asked for the documents a project needs besides issues — a
PRD, a functional spec, business flows — and asked that both people and AI
agents know which documents exist and how each is written. DESIGN §13 already
reserves `docs/.templates/*.md` ("copy and fill in the placeholders") but no
template exists, nothing creates a page from one, and an agent has no way to
learn the set.

The people this serves include non-technical ones (ADR 0028, ADR 0029): a
product owner who has never written an SRS needs the document to say what goes
where. The set was chosen deliberately broad — so a team can see every
document that may be needed, then skip the ones it does not — rather than
trimmed to what one team happens to use.

## Decision

**Eleven kinds, built into DIT:**

| Kind | Id | Placed in | Answers |
|---|---|---|---|
| Business Requirements Document | `brd` | `docs/business/` | Why are we doing this, for whom, and what does success look like? |
| Product Requirements Document | `prd` | `docs/business/` | What will the product do, for which users, in what scope? |
| Software Requirements Specification | `srs` | `docs/requirements/` | What must the system do and how well — every requirement, testable |
| Functional Specification | `fsd` | `docs/requirements/` | How does each feature behave, screen by screen and rule by rule? |
| Business Flow (with BDD scenarios) | `business-flow` | `docs/requirements/` | How does work move between people and the system, and what proves it? |
| Technical Specification | `tsd` | `docs/technical/` | How will it be built: components, choices, risks |
| Data Model (ERD) | `data-model` | `docs/technical/` | What data exists, how it relates, who owns it |
| API Contract | `api-contract` | `docs/technical/` | What each endpoint takes and returns — pointing at the OpenAPI file and its Morse scenarios (ADR 0022) |
| Architecture Decision Record | `adr` | `docs/adr/` | Which option was chosen, against which alternatives, and why |
| Test Plan & UAT | `test-plan` | `docs/testing/` | What is tested, how, by whom, and what counts as accepted |
| Release Notes | `release-notes` | `changelogs/` | What changed for the people who use it |

- **Placed by stage**, so the Docs tree reads as the lifecycle: business,
  requirements, technical, decisions, testing, changes. A human-written
  business flow goes to `docs/requirements/`, never `docs/flows/` — that
  folder is the AI-generated, staleness-checked output of DESIGN §7.4.
- **Each template** opens with its purpose (who reads it, when it is written),
  then its sections. Under every heading an italic prompt says what belongs
  there; the writer replaces it. Prompts are plain Markdown, so they read the
  same in the editor, on GitHub, and to an agent. Where a kind needs a
  diagram, the template carries a `mermaid` fence to edit (ADR 0013) — a flow
  chart in a business flow, an `erDiagram` in a data model, a sequence in a
  TSD. A **Related** section links the neighbouring documents and issues with
  `[[…]]`; overlaps between kinds are cross-referenced, not repeated.
- **Frontmatter** records `title` and `kind` — what the document is, an
  authored fact. Nothing derived is written (I5): who touched it and when is
  `git log`.

**Overridable per workspace.** A file `docs/.templates/<id>.md` replaces the
built-in template of that id; a file with any other id adds a kind of the
team's own, placed in `docs/` unless its frontmatter names a `folder`. The
built-ins stay in the binary, so a fix to a template reaches every workspace
with the next DIT, and a new workspace is not filled with eleven files.

**Creating a page from one.** `dit docs templates` lists the kinds; `dit docs
new <kind> "<title>"` writes `<folder>/<slug-of-title>.md` through a
transaction like every write (I1), filling `{{title}}`, `{{date}}` and
`{{author}}`, and refuses a path that already holds a page. The browser's
Docs screen gains **New from template**, the same operation over
`GET /api/docs/templates` and `POST /api/docs/from-template`.

**Taught to agents.** `dit ai spec docs` describes the set: what each kind
answers, the order they are usually written in, how to create one, how to
fill it from issues and code without inventing facts, how to keep diagrams
editable (`mermaid`) or, for a polished drawing made elsewhere, as an SVG in
a sanitized `dit-diagram` fence (ADR 0012), and that nothing derived is ever
written into a document.

Invariant check: I1 — pages are written by a transaction. I5 — `kind` is
authored, not computed. I7 — a template is text; nothing in it is executed
or fetched. A template override is read from the workspace like any page
(ADR 0010).

## Consequences

- A product owner can make a correctly shaped PRD with two clicks and see
  what to write in each section; an agent asked for "the specs" knows the set
  and the order.
- `docs/business/`, `docs/requirements/`, `docs/technical/` and
  `docs/testing/` become conventional folders; existing workspaces are not
  rearranged.
- Templates are English. A team writing in another language overrides them.
- A built-in LLM that drafts documents (DESIGN §7) is still deferred; agents
  fill templates through the CLI today.

## Verification

`dit-core` pins: every built-in kind renders with its title and date filled
and parses as a page; an override replaces a built-in and a new id adds a
kind; creating a page commits it at its stage folder and refuses an existing
path. The server suite pins the two routes; the CLI suite pins `dit docs
new`. The agent spec pins the `docs` topic.
