---
id: 0003
title: Radix primitives + Tailwind (shadcn pattern), not a component library
status: accepted
date: 2026-08-16
supersedes: null
---

## Context

The UI needs to feel like Linear — dense, keyboard-first, fast — and it ships
**embedded inside a ~4 MB binary** (ADR 0002). Bundle size and runtime cost carry
more consequence here than in a typical web app: they are part of the install.

It also needs pieces most component libraries do not provide: a kanban board, a
50,000-row virtualized table, a block editor, a dependency graph, and diff views.

## Options considered

| Option | Verdict |
|---|---|
| **Radix primitives + Tailwind (shadcn pattern)** | **Chosen** |
| Mantine (`@mantine/core` 9.5.1, MIT) | Excellent DX, but an opinionated look that fights a dense dev-tool feel, and a heavier runtime |
| MUI (`@mui/material` 9.3.1, MIT) | Material Design is the wrong aesthetic here; Emotion runtime cost |
| Chakra (`@chakra-ui/react` 3.36.1, MIT) | Runtime CSS-in-JS — the wrong trade when the bundle is the install |
| Base UI (`@base-ui-components/react`) | Promising, but `1.0.0-rc.0` — pre-1.0 in the UI foundation is one risk too many alongside `gix`, `sqlite-vec` and comrak's experimental flag |

## Decision

**Radix primitives + Tailwind, with components copied into the repo (the shadcn
pattern) rather than imported from a component library.**

The decisive property is that the components are *ours*. There is no version to
track, no vendor to be locked to, no license to re-check, and no upgrade that
silently restyles the app. That is the same reasoning ARCHITECTURE.md §4.5 uses
to reject speculative traits: do not add a layer you do not control.

Radix contributes the part that is genuinely hard and genuinely load-bearing for
a keyboard-first tool: focus management, roving tabindex, escape/dismiss
semantics, screen-reader wiring. Tailwind contributes zero runtime and a CSS file
small enough to embed without thinking about it.

### Supporting picks

| Need | Choice | Why not the obvious alternative |
|---|---|---|
| Command palette | `cmdk` 1.1.1 | — |
| Kanban | `@dnd-kit/core` + `sortable` | — |
| 50k-row table | `@tanstack/react-table` + `react-virtual` | — |
| Icons | `lucide-react` (ISC) | Tree-shakeable |
| Toasts | `sonner` | — |
| **Dependency graph** | **`d3-force` (89 KB)** | `cytoscape` is 5.7 MB unpacked — 64x larger for a force layout we already control |
| **Analytics charts** (§14) | **`uPlot` (545 KB)** | `recharts` is 7.5 MB unpacked; CFD and burndown are time series, which is exactly uPlot's case |
| **Mermaid diagrams** | **lazy import only** | `mermaid` is **83.5 MB unpacked**. It must never be reachable from the entry graph |

Chart *design* (color, axes, density) is deliberately out of scope here — that is
a design-system decision to make when the analytics views are actually built, not
a library decision.

## Consequences

We maintain the copied components. That is real work, and it is the price of not
having a vendor in the render path.

Tailwind becomes a build-step dependency for the web app — but not for Rust:
`cargo run -p dit-server` still needs no Node toolchain (ADR 0002).

Because the bundle is part of the install, it is now gated:
`scripts/check-bundle-size.mjs` fails CI at 350 KB gzipped for the entry chunk,
60 KB for CSS, and 900 KB for any single lazy chunk. A CSS budget blowup is the
signal that a runtime CSS-in-JS library crept back in.

## Verification

Sizes from the npm registry, 16 August 2026 (`dist.unpackedSize`):

```
mermaid    83,547,314   cytoscape  5,719,339   recharts  7,452,998
uplot         545,468   d3-force      89,551   sonner      174,012
```

Licenses of every added dependency: MIT, except `lucide-react` (ISC) and
`class-variance-authority` (Apache-2.0). All compatible with Apache-2.0 and all
pass `scripts/check-js-licenses.mjs`.
