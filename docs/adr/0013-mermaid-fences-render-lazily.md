---
id: 0013
title: "`mermaid` fences render in the editor through a lazy-loaded renderer"
status: accepted
date: 2026-08-21
---

## Context

ADR 0012 decided `dit-diagram` (SVG bytes in a fence, sanitized and rendered
client-side) and explicitly left the mermaid door open: "a renderer could
still be added later." The case for walking through it:

- Mermaid is the ecosystem standard — GitHub renders `mermaid` fences
  natively, and real repos already carry them (this workspace's own docs do).
  With mermaid rendered, DIT has two complementary diagram kinds: `mermaid`
  (quick to author, renders everywhere including GitHub) and `dit-diagram`
  (editorial quality, renders wherever DIT's editor runs).
- Without it, the ecosystem's most common diagram syntax shows as source in
  the very tool whose promise is "your markdown, rendered."

Facts established by running mermaid 11.17.0 in jsdom before this decision
(`securityLevel: "strict"`, `htmlLabels: false`):

- Benign flowchart output contains a `<style>` block, `foreignObject`,
  `feDropShadow`, `div`/`span`/`p` — and no `<script>`, no `on*=` handlers,
  no `javascript:` URLs.
- Parse errors reject (`mermaid.render` throws), so a broken fence can fall
  back to source with the reason.
- The happy path renders under jsdom, but subsequent renders and the error
  paths hang (jsdom's SVG layer lacks transitions mermaid awaits) — so the
  vitest suite may run exactly one real mermaid render per process and must
  not lean on it for behavior tests.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Keep source-only (ADR 0012 stance) | None | The standard diagram syntax stays unrendered in DIT |
| Bundle mermaid eagerly | The entry bundle grows by the renderer | Every doc pays, even ones with no diagram |
| Server-side rendering | comrak `unsafe_` off, and a JS runtime in the server | Violates the render-side rules §17 / invariant 10 |
| **Lazy chunk + strict mode, client-side** | A lazy chunk loaded only when a mermaid fence mounts | Docs without mermaid never download it; the fence stays plain text to git |

## Decision

**A `mermaid` fence gets the same card UX as `dit-diagram` — rendered
drawing above, source below, one toggle — but the drawing is produced by
mermaid.js loaded on demand, and the security gates are per-source:**

- `dit-diagram` (untrusted bytes from the file) passes through the
  `sanitizeSvg` allowlist, unchanged (ADR 0012).
- `mermaid` output is **not** run through `sanitizeSvg`: the probe above
  shows strict-mode output legitimately needs `<style>` and `foreignObject`,
  which the allowlist strips by design. Its gate is mermaid's own built-in
  DOMPurify sanitizer under `securityLevel: "strict"` (click interactions
  disabled, labels sanitized). The page CSP remains the backstop for both.
- The renderer loads via dynamic `import("mermaid")` the first time a
  mermaid fence renders; vite emits it as a separate chunk, gated by its own
  bundle-budget lane. A doc with no mermaid fence never downloads it.
- Initialization is pinned: `startOnLoad: false`, `securityLevel: "strict"`,
  `suppressErrorRendering: true`, dark theme. A render failure or a parse
  error shows the source with the reason — never blank, never unsanitized
  passthrough. Stale async results are dropped (if the source changed or the
  view was destroyed while rendering, the result is not inserted).
- The file format does not change: a `mermaid` fence is already legal
  CommonMark that round-trips through the bridge verbatim, so no crate moves
  and §18 does not move. Server-rendered HTML (comments) keeps showing the
  source, as with `dit-diagram`.

## Consequences

- New runtime dependency: `mermaid` 11.17.0, MIT (recorded here per the
  no-silent-dependency rule, §9; the JS license gate must stay green).
- A second diagram surface to keep honest: the two gates are now distinct by
  design and documented as such. Adding a third diagram kind later must
  declare its gate the same way.
- The lazy chunk is large for a browser dependency (measured below); it is
  acceptable because it is paid only by docs that actually contain a mermaid
  fence, and GitHub sets the same precedent.
- jsdom cannot exercise mermaid's error paths (they hang), so the NodeView's
  failure behavior is verified against the running app, and the vitest suite
  pins exactly one real render per process (the strict-mode output contract)
  rather than behavior tests through mermaid.

## Verification

- `npm run build --prefix apps/web` → the mermaid chunk is emitted as its own
  lazy asset; `node scripts/check-bundle-size.mjs` → all lanes green
  including the new mermaid lane (entry/css/wasm budgets unchanged).
- `npm test` (vitest, jsdom) → the strict-mode contract test passes: a
  rendered flowchart contains no `<script`, no `on…=` handler, no
  `javascript:` URL; the full pre-existing suite stays green.
- Live smoke on the dev server: a valid fence renders, a syntactically
  broken fence shows source plus the parse-error reason, and the toggle,
  copy, and stale-render drop behave as decided.
