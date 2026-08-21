---
id: 0011
title: "The rich editor serializes through Rust in WASM; markdown is never written twice"
status: accepted
date: 2026-08-21
---

## Context

DESIGN.md §12.2 closes the decision: there is **one** markdown serializer,
in Rust, and the editor talks to it. If the browser had its own
markdown writer (or the server one "just for the editor"), every UI save
would be a formatter disagreement away from a spurious diff — the open-50,
close-50, `git status` must-stay-clean scenario is Risk #12, and this whole
product is "done in git".

The editor surface is TipTap (§12.4): a ProseMirror schema in the browser,
markdown bytes at rest. Those two worlds meet at a document bridge.

## Options considered

| Option | Cost | Consequence |
|---|---|---|
| Server API: editor posts PM JSON, server serializes | Small client; a route + auth + error taxonomy in dit-server | Every keystroke debounce is a round trip; offline-first tool gains an editing dependency on the server's liveness |
| JS markdown serializer alongside comrak | None at rest | Two formatters over one canonical form; they *will* drift (comrak upgrades, escape rules, list tightness) and the drift lands in commits |
| **Compile the Rust bridge to WASM, run it in the tab** | ~217 KB gz (measured) lazy-loaded | One serializer, byte-identity is structural, works offline; costs a CSP widening and a build prerequisite |

## Decision

**`dit-parse` owns the bridge; the browser runs it as WASM.**

- `dit_parse::prosemirror` exposes `markdown_to_doc` (comrak AST →
  ProseMirror JSON) and `doc_to_markdown` (PM JSON → comrak AST → the
  **same `format_ast` pipeline `dit fmt` uses**). Byte-identity is not
  a property we test for after the fact; it is the implementation — there
  is no second code path that could disagree.
- The mapping is a bijection with exactly three deliberate deviations from
  TipTap's stock schema, each forced by markdown semantics:
  - **`listItem.task`** (`null \| false \| "x" | "X"`) instead of
    TaskList/TaskItem, because comrak marks task-ness per item and one GFM
    list may mix task and plain items.
  - **`tableRow.isHeader`** + cell node types pinned to the row, because
    GFM's header row is exactly the first row and TipTap's toggle-header
    works on any row.
  - **`hardBreak.soft`**, because one PM node covers comrak's SoftBreak and
    LineBreak, and tight-list rendering depends on telling them apart.
- Raw HTML (`htmlBlock`, `htmlInline`) and `wikiLink` are custom nodes with
  `literal`/`target` attrs — verbatim bytes, never interpreted as markup in
  the editor (they render as a mono box and a muted pill). The editor shows
  what the file holds; it never executes it (§17).
- Incoming PM JSON is validated strictly (unknown node/mark = error, depth
  and node-count caps) and conflict markers are refused in **both**
  directions — a document that cannot round-trip is an error message, never
  a silent rewrite.
- Delivery: `crates/dit-wasm` is a thin `wasm-bindgen` wrapper (string in,
  string out; no `js-sys`/`web-sys`), built by `just wasm-build` into
  `apps/web/src/editor/wasm/` (gitignored, like `dist/`). Vite fails the
  build loudly when the artifact is missing; the bundle gate caps it at
  260 KB gz (measured 217). It loads only when a rich editor mounts.
- One CSP widening: `script-src` gains `'wasm-unsafe-eval'` — the narrowest
  token that permits WebAssembly compilation, still banning JS `eval`
  (the security test asserts the token `'unsafe-eval'` stays absent at
  token level, because the substring check would false-positive). `.wasm`
  is served `application/wasm` so `instantiateStreaming` works.

## Consequences

- The editor cannot author what markdown cannot hold: no underline, no
  footnotes, no multi-paragraph cells. The schema refuses more than the
  formatter would — "won't round-trip" is caught at save time as an error,
  not discovered as a diff.
- A shape normalizer in the editor (`ditShape`) fixes the few shapes
  TipTap's commands can produce but markdown reparse would change (a tight
  list with two paragraphs in one item reparse-merges via lazy
  continuation; those lists go loose). The rule is "the editor must not be
  able to author a document its own save path refuses or silently changes".
- Source mode (CodeMirror) stays: conflict markers, raw byte surgery, and
  image alt text live there. The rich editor hands off to it on refusal.
- Rust changes to `dit-parse`/`dit-wasm` require `just wasm-build` and a
  dev-server reload — noted in the README, enforced by the Vite guard.
