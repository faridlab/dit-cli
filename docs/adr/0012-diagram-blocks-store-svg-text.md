---
id: 0012
title: "Diagrams are SVG source in a `dit-diagram` fence, rendered sanitized in the editor"
status: accepted
date: 2026-08-21
---

## Context

Users want editorial diagrams in docs and issue bodies — the quality bar set
by agent-authored diagram kits like `diagram-design` (MIT,
cathrynlavery/diagram-design), not auto-layout boxes. The constraints that
shape any answer:

- §12.5: custom blocks are fenced code blocks — the doc schema stays a
  strict subset of CommonMark + `dit-*` fences, with graceful degradation in
  every other viewer.
- §17 / invariant 10: comrak's `render.unsafe_` stays off, so the server
  never renders inline HTML/SVG; anything rendered happens client-side in
  the editor.
- The CSP is strict (`script-src 'self' 'wasm-unsafe-eval'`, `font-src
  'self'`, no external fetches) and stays that way.
- Invariant 7: no field in a DIT file may name an executable or a
  auto-fetched URL — DIT must never *execute* a diagram tool.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Render `mermaid` fences with mermaid.js | ~500 KB renderer shipped to the tab | A layout engine executes in the editor; the aesthetic is auto-layout boxes — the thing the user is trying to escape; one more runtime dependency to track |
| Store rendered `.svg`/`.png` attachments, link from docs | Attachment storage + serving outside the `.md`-only doc sandbox | Binary-ish assets: poor diffs, no merge story, links rot on moves |
| **Store the SVG source itself in a `dit-diagram` fence** | None at rest — it is already legal CommonMark | Text at rest, zero runtime dependencies, render is a sanitizer away; any authoring path (hand, tool, agent) produces the same storage |

## Decision

**A diagram is an ordinary fenced code block with info string
`dit-diagram` whose bytes are a standalone SVG document.**

- The file format does not change: comrak already preserves fenced blocks
  with any info string verbatim, so `dit-model`/`dit-parse`/`dit-store`
  are untouched and §18's schema version does not move. Pinned by the
  `dit_diagram_block_round_trips_verbatim` fixture in `dit-parse`
  (a `<script>` payload inside the fence survives as literal bytes).
- The editor renders it: a `codeBlock` with `language="dit-diagram"` gets a
  NodeView with the sanitized drawing above its own source, one toggle
  between them, and a copy button. The source is the only writable half;
  the drawing is always a re-render of those bytes.
- Sanitization is a single gate, client-side
  (`apps/web/src/editor/sanitizeSvg.ts`): allowlist of drawing elements and
  presentation attributes, local `#fragment` references only, no `script`,
  `style`, `foreignObject`, `image`, external `href`, or SMIL animation, a
  20,000-element cap. A refusal renders the block as source with the
  reason — never blank, never unsanitized. The CSP remains the backstop,
  not the gate.
- Fonts: `font-src 'self'` means an SVG naming Google Fonts renders in the
  page's font stack. That is the accepted trade — no CSP widening for
  diagrams.
- Server-rendered HTML (comment bodies via comrak) shows the block as an
  ordinary code fence: readable source, never a drawing. The always-on
  rich editor is the reading surface for docs and issue bodies, so the
  diagram renders wherever editing happens.
- Authoring is outside DIT by design (invariant 7): paste SVG from any
  tool, or generate it with an agent using the diagram-design skill — its
  static output is script-free by construction, which is exactly what the
  allowlist expects. A future `dit-ai` action can generate the same bytes
  through the configured provider without changing this design.

## Consequences

- Two new dev-only npm dependencies: vitest + jsdom, because the sanitizer
  is the XSS gate for the editor and its behavior is pinned by tests that
  need a real DOM. No runtime dependency; the bundle budget is untouched.
- A diagram costs ~5–15 KB of SVG text in the doc. Two concurrent diagram
  edits merge as a text conflict, resolved in source mode — the same terms
  as any code block.
- GitHub and every non-DIT viewer degrade to readable SVG source, which is
  the §12.5 rule for custom blocks.
- `mermaid` fences remain what they were — plain code blocks; a renderer
  for them can still be added later without touching this design.

## Verification

- `cargo test -p dit-parse dit_diagram` →
  `test prosemirror::tests::dit_diagram_block_round_trips_verbatim ... ok`
  (byte-identical round-trip; the PM JSON carries
  `attrs.language == "dit-diagram"` and the literal text unchanged).
- `npm test` (vitest, jsdom) → `12 passed (12)`: script/`on*`/`style`/
  `foreignObject`/external-ref/animation stripping, comment stripping,
  malformed-XML and non-SVG-root refusal, the element cap, and the filter/
  gradient/marker vocabulary real editorial SVG uses.
- `npm run build && npm run size` → all lanes under budget.
