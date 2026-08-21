// The `mermaid` editing surface (ADR 0013). Same card contract as the
// `dit-diagram` view — the drawing on top, the literal source under it, one
// toggle — but the drawing is produced by mermaid.js, loaded on demand the
// first time any mermaid fence renders. A doc with no mermaid fence never
// downloads the chunk.
//
// The gate differs by source, on purpose. A `dit-diagram` holds untrusted
// bytes, so its SVG passes the sanitizeSvg allowlist. Mermaid output comes
// from diagram *text* through the renderer's own sanitizer (DOMPurify,
// securityLevel "strict") — running it through sanitizeSvg would strip the
// <style> and <foreignObject> mermaid legitimately emits and break the
// drawing. The page CSP backs both paths.

import type { Editor } from "@tiptap/core";
import type { Node as PmNode } from "@tiptap/pm/model";
import type { NodeView } from "@tiptap/pm/view";

let renderer: Promise<(typeof import("mermaid"))["default"]> | null = null;
let initialized = false;
let renderCount = 0;

/** Renders mermaid source to an SVG string, loading mermaid exactly once. */
export async function renderMermaid(source: string): Promise<string> {
  renderer ??= import("mermaid").then((m) => m.default);
  const mermaid = await renderer;
  if (!initialized) {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      suppressErrorRendering: true,
      theme: "dark",
    });
    initialized = true;
  }
  renderCount += 1;
  const { svg } = await mermaid.render(`dit-mermaid-${renderCount}`, source);
  return svg;
}

/** First line of an error, capped — mermaid parse errors are essays. */
function reason(err: unknown): string {
  const message = err instanceof Error ? err.message : String(err);
  const line = message.split("\n").find((candidate) => candidate.trim().length > 0);
  const text = line === undefined ? "could not render" : line;
  return text.length > 120 ? `${text.slice(0, 117)}…` : text;
}

/** The mermaid NodeView: the rendered drawing above its own source text,
 *  one toggle between them. Only the source is writable; the drawing is
 *  always a re-render of those bytes, never markup the editor keeps. The
 *  block stays an ordinary fenced code block to git and to the bridge. */
export function MermaidView(
  name: string,
  editor: Editor,
  node: PmNode,
  getPos: () => number | undefined,
): NodeView {
  const wrap = document.createElement("div");
  wrap.className = "dit-diagram";

  const state = document.createElement("span");
  state.className = "dit-diagram-state";

  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "dit-diagram-btn";
  toggle.addEventListener("click", () => {
    setMode(wrap.dataset.mode === "preview" ? "source" : "preview");
  });

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "dit-diagram-btn";
  copy.textContent = "Copy source";
  copy.addEventListener("click", () => {
    void navigator.clipboard
      .writeText(source())
      .then(() => {
        copy.textContent = "Copied";
        window.setTimeout(() => {
          copy.textContent = "Copy source";
        }, 1200);
      })
      .catch(() => {
        // No clipboard access — surface the text so it can be copied.
        setMode("source");
      });
  });

  const label = document.createElement("span");
  label.className = "dit-diagram-label";
  label.textContent = "Mermaid";
  const bar = document.createElement("div");
  bar.className = "dit-diagram-bar";
  bar.append(label, state, toggle, copy);

  const canvas = document.createElement("div");
  canvas.className = "dit-diagram-canvas";

  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.className = "language-mermaid";
  pre.append(code);
  wrap.append(bar, canvas, pre);

  // The node this view was built for goes stale as the doc moves; the doc at
  // our position is the truth.
  const source = (): string => {
    const pos = getPos();
    if (typeof pos !== "number") return node.textContent;
    return editor.state.doc.nodeAt(pos)?.textContent ?? node.textContent;
  };

  // A render can land after the user typed on or the view went away; only
  // the newest request may touch the canvas.
  let drawSeq = 0;
  const draw = async (): Promise<void> => {
    const seq = (drawSeq += 1);
    state.textContent = "rendering…";
    try {
      const svg = await renderMermaid(source());
      if (seq !== drawSeq || !wrap.isConnected) return;
      canvas.innerHTML = svg;
      state.textContent = "";
    } catch (err) {
      if (seq !== drawSeq || !wrap.isConnected) return;
      // A refusal falls back to the source below with the reason next to the
      // label — never a blank card and never an unsanitized drawing.
      state.textContent = `not rendered — ${reason(err)}`;
      wrap.dataset.mode = "source";
      toggle.textContent = "Show diagram";
    }
  };

  const setMode = (mode: "preview" | "source"): void => {
    wrap.dataset.mode = mode;
    toggle.textContent = mode === "preview" ? "Edit source" : "Show diagram";
    if (mode === "preview") {
      void draw();
    } else {
      state.textContent =
        source().trim().length === 0 ? "type mermaid syntax — it renders as a diagram" : "";
    }
  };

  // First paint follows the bytes: a non-empty fence opens rendered.
  setMode(node.textContent.trim().length === 0 ? "source" : "preview");

  return {
    dom: wrap,
    contentDOM: code,
    update: (updated) => {
      if (updated.type.name !== name) return false;
      // A language change flips which surface the block needs; let the view
      // be rebuilt instead of morphing it in place.
      return updated.attrs.language === node.attrs.language;
    },
    // The bar and canvas are ours; only edits inside the code are the doc's.
    ignoreMutation: (mutation) => !code.contains(mutation.target),
    stopEvent: (event) =>
      event.target instanceof Element && (bar.contains(event.target) || canvas.contains(event.target)),
  };
}
