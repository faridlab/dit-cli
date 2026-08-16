---
id: 0004
title: Publish the design documents now, open the source at v0.2
status: accepted
date: 2026-08-16
supersedes: null
---

## Context

Two separable decisions were being treated as one: **which license** DIT ships
under, and **when** the repository becomes public. `DESIGN.md` §11 already
settles the first (Apache-2.0). This ADR settles the second.

The trigger for asking was a comparison to Obsidian — which turns out to be the
opposite of what it is usually assumed to be, and that changes the reasoning.

### Obsidian is not open source

Its GitHub organization holds 14 repositories — plugin API type definitions, a
sample plugin, developer docs, help content, an importer, the community plugin
list, a web clipper, the JSON Canvas format spec, and a headless sync client.
**The desktop application source is not among them.** The license page states it
plainly: *"We own and reserve rights to our content, including text, images, and
code in the app, which is protected by copyright and other laws."*

What Obsidian opens is the **file format**, not the code — *"simple, open file
formats that prevent lock-in."* The app is free for all use including commercial,
with no sign-up. Revenue comes from services: Sync ($4–5/month), Publish
($8–10/month), an optional commercial license ($50/user/year), and a $25 Catalyst
tier. Founded 2020 by Erica Xu and Shida Li; 100% user-supported, no investors.

### What that implies for DIT

**DIT already wins on the axis Obsidian actually competes on.** Markdown inside
git is not merely readable — its entire history is too. If open formats are what
earn user trust, DIT has that without opening a single line of code.

**But Obsidian's main revenue line is closed to DIT.** Sync is their largest
paid service, and in DIT sync is free because git does it. What remains is a
hosted team server, AI credits, or a commercial license — a weaker position.

That is an argument *for* open source rather than against it: when the core is
hard to monetize, opening it buys contributors instead.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Public from day one | 16–21 weeks of a public repo with no working product; schema compatibility promises (§18) become binding immediately; issue triage before there is anything to triage | Maximum "built in the open" credibility |
| Private until v1.0 | The built-in-the-open story is available only once; contributors arrive after the architecture has already hardened | Complete freedom to change everything |
| Obsidian model — closed core, open format | Forfeits contributors, which is the one asset DIT can actually trade for | Preserves paid-service optionality DIT largely does not have |
| **Staged: docs now, code at v0.2** | Two repositories to manage for a few months | **Chosen** |

## Decision

**Three moves, in order.**

**1. Publish the design documents now.** `DESIGN.md`, `ARCHITECTURE.md` and the
ADRs, as their own public repository or a static site. They are finished, they
cost nothing to share, and they are the strongest recruiting instrument available
right now. Anyone drawn to §5.3 (the merge driver) or §14 (deriving history from
git) is precisely the contributor worth having.

Idea theft is not a real risk. The moat is 67–89 weeks of execution, not the
concept — and the document says so out loud, repeatedly, including where it was
wrong.

**2. Keep the code private through v0.2.** Not out of embarrassment — because of
§18. Schema compatibility promises become binding **the moment someone else has
data**. Before that, the format can change freely. After it, every change owes a
migration. v0.2 is the point where `just check` is green with a working merge
driver and the format has stopped moving, which is when that promise can be made
honestly.

**3. Make DIT's own data repository public from day one.** This falls out of the
architecture at no cost: Mode A (§5.0) separates the DIT repo from the code repo,
so **the data repo can be public while the code repo is private.** People watch
DIT's issues, board and changelog being managed by DIT itself — the best possible
demo — without shipping half-built code.

## Consequences

The "built in the open" story is preserved, because the design *is* in the open
from the start and the dogfooding is publicly visible. Only the incomplete
implementation is withheld.

Two repositories to keep in step for a few months. Mode A was going to be
exercised anyway (it is the default), so this doubles as an early real-world test
of the polyrepo support in §5.0.

Apache-2.0 applies from the first public commit. The license decision does not
wait for the timing decision.

### Opening trigger — explicit, so this does not become an indefinite delay

The source repository goes public when **all** of these hold:

- [ ] `just check` green, including invariant I6 (`test_merge_driver_failsafe`) un-ignored
- [ ] `dit-merge-bot` working, so external PRs are not broken on arrival (Risk #1)
- [ ] `schema_version: 1` frozen, with `dit migrate` present even if it has nothing to do yet
- [ ] The crate-name conflict resolved (`dit` is taken on crates.io — see §11)
- [ ] `dit fmt` stable against a real markdown corpus, with golden files committed

If these are met before v0.2 lands, open early. If v0.2 lands without them, do
not open — the checklist governs, not the version number.

## Verification

- Obsidian's GitHub organization contents and the absence of the application
  source: <https://github.com/obsidianmd>
- Copyright reservation over the app code: <https://obsidian.md/license>
- Founding, user-funded model, open-file-format positioning: <https://obsidian.md/about>
- Pricing tiers: <https://obsidian.md/pricing>

Retrieved 16 August 2026. Pricing and repository contents change; re-check before
citing these figures anywhere public.
