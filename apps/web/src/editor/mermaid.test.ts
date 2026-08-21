// The mermaid render contract (ADR 0013). jsdom supports exactly ONE real
// mermaid render per process — later renders hang awaiting SVG transitions
// jsdom never fires — so this file renders once, through the production
// `renderMermaid` configuration, and pins what comes back. The NodeView's
// failure paths (parse error → source + reason) are verified live against
// the dev server for the same jsdom reason.
import { beforeAll, describe, expect, it } from "vitest";
import { renderMermaid } from "./mermaidView";

beforeAll(() => {
  // jsdom's SVG layer lacks the measurement APIs mermaid needs; constants
  // are enough for it to lay out a two-node graph.
  (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox = () =>
    ({ x: 0, y: 0, width: 100, height: 20 }) as DOMRect;
  const textEl = document.createElementNS("http://www.w3.org/2000/svg", "text");
  const proto = Object.getPrototypeOf(textEl) as unknown as Record<string, unknown>;
  proto.getComputedTextLength = () => 40;
  proto.getExtentOfChar = () => ({ x: 0, y: 0, width: 40, height: 12 }) as DOMRect;
});

describe("renderMermaid", () => {
  it(
    "returns SVG with nothing executable in it under the pinned strict configuration",
    async () => {
      const svg = await renderMermaid('graph LR\n  Write["markdown"] --> Draw["drawing"]');
      expect(svg).toContain("<svg");
      // The gate for mermaid output is mermaid's own DOMPurify under
      // securityLevel "strict" — NOT sanitizeSvg, whose allowlist strips the
      // <style> and <foreignObject> a real drawing legitimately contains
      // (ADR 0013). The contract that must hold regardless: nothing
      // executable survives the renderer.
      expect(svg).not.toMatch(/<script/i);
      expect(svg).not.toMatch(/\son\w+=/i);
      expect(svg).not.toMatch(/javascript:/i);
    },
    20000,
  );
});
