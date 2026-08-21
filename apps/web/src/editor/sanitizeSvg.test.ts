// The sanitizer is the single gate between diagram bytes in a doc and the
// editor's DOM, so its behavior is pinned here: what survives, what is
// silently dropped, and what refuses outright (and falls back to source).

import { describe, expect, it } from "vitest";
import { sanitizeSvg } from "./sanitizeSvg";

const ok = (source: string): string => {
  const result = sanitizeSvg(source);
  expect(result.ok).toBe(true);
  return result.ok ? result.svg : "";
};

describe("sanitizeSvg", () => {
  it("keeps an ordinary drawing intact", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 40" role="img" aria-label="demo">` +
        `<title>demo</title><rect x="0" y="0" width="100" height="40" fill="#111"/>` +
        `<text x="50" y="20" text-anchor="middle" font-size="8" fill="#fff">hello</text></svg>`,
    );
    expect(out).toContain("<rect");
    expect(out).toContain("viewBox");
    expect(out).toContain("aria-label");
  });

  it("drops script elements entirely", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/>` +
        `<script>alert('nope')</script></svg>`,
    );
    expect(out).not.toContain("script");
    expect(out).not.toContain("alert");
  });

  it("drops event handler attributes", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" onload="alert(1)" onclick="x()"/></svg>`,
    );
    expect(out).not.toContain("onload");
    expect(out).not.toContain("onclick");
  });

  it("drops style attributes and foreignObject subtrees", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg"><rect style="fill:red" width="1" height="1"/>` +
        `<foreignObject width="10" height="10"><div xmlns="http://www.w3.org/1999/xhtml">html</div></foreignObject></svg>`,
    );
    expect(out).not.toContain("style=");
    expect(out).not.toContain("foreignObject");
    expect(out).not.toContain("html");
  });

  it("drops external references but keeps local ones", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">` +
        `<defs><linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient></defs>` +
        `<rect fill="url(#g)" href="https://evil.example/x#frag" width="1" height="1"/>` +
        `<use xlink:href="#g"/></svg>`,
    );
    expect(out).toContain("url(#g)");
    expect(out).toContain('xlink:href="#g"');
    expect(out).not.toContain("evil.example");
  });

  it("drops animation elements — diagrams are static", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1">` +
        `<animate attributeName="width" to="100"/></rect>` +
        `<set attributeName="fill" to="red"/></svg>`,
    );
    expect(out).not.toContain("animate");
    expect(out).not.toContain("<set");
  });

  it("strips comments", () => {
    const out = ok(`<svg xmlns="http://www.w3.org/2000/svg"><!-- secret --></svg>`);
    expect(out).not.toContain("secret");
    expect(out).not.toContain("<!--");
  });

  it("keeps data-* attributes but not data-on ones", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg" data-kind="loop"><rect data-onx="1" width="1" height="1"/></svg>`,
    );
    expect(out).toContain('data-kind="loop"');
    expect(out).not.toContain("data-onx");
  });

  it("refuses malformed XML", () => {
    const result = sanitizeSvg("<svg><rect></svg>");
    expect(result).toEqual({ ok: false, error: "not well-formed XML" });
  });

  it("refuses a non-svg root", () => {
    const result = sanitizeSvg("<div xmlns=\"http://www.w3.org/1999/xhtml\">hi</div>");
    expect(result).toEqual({ ok: false, error: "root element is not <svg>" });
  });

  it("refuses a pathologically large tree", () => {
    // Wide, not deep: the cap guards the sanitizer's walk, and a deep nest
    // would be measuring the XML parser, not the gate.
    const wide = "<g/>".repeat(21_000);
    const result = sanitizeSvg(`<svg xmlns="http://www.w3.org/2000/svg">${wide}</svg>`);
    expect(result.ok).toBe(false);
  });

  it("keeps filter and gradient vocabulary used by editorial diagrams", () => {
    const out = ok(
      `<svg xmlns="http://www.w3.org/2000/svg"><defs>` +
        `<filter id="rough"><feTurbulence type="fractalNoise" baseFrequency="0.02" numOctaves="2" result="n"/>` +
        `<feDisplacementMap in="SourceGraphic" in2="n" scale="3"/></filter>` +
        `<marker id="arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto">` +
        `<path d="M0,0 L8,4 L0,8 z" fill="#333"/></marker></defs>` +
        `<line x1="0" y1="4" x2="40" y2="4" stroke="#333" marker-end="url(#arrow)"/></svg>`,
    );
    expect(out).toContain("feTurbulence");
    expect(out).toContain("feDisplacementMap");
    expect(out).toContain("marker-end");
  });
});
