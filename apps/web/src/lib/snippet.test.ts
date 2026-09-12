// A search result's title often does not say why it matched — the words
// were three paragraphs down. These pin what the preview line shows, and
// that it never invents a match that is not in the body.
import { describe, expect, it } from "vitest";
import { plainText, snippet } from "./snippet";

const text = (segments: ReturnType<typeof snippet>) => segments.map((s) => s.text).join("");
const marked = (segments: ReturnType<typeof snippet>) =>
  segments.filter((s) => s.match).map((s) => s.text);

describe("plainText", () => {
  it("drops code fences, inline code and link markup", () => {
    const md = "See [the driver](docs/x.md) and `%A`.\n\n```rust\nfn main() {}\n```\n";
    expect(plainText(md)).toBe("See the driver and %A.");
  });

  it("drops heading, quote and bullet markers", () => {
    expect(plainText("## Context\n\n> quoted\n\n- one\n- two")).toBe("Context quoted one two");
  });

  it("drops emphasis without eating the words", () => {
    expect(plainText("**bold** and _italic_ and ~~struck~~")).toBe("bold and italic and struck");
  });
});

describe("snippet", () => {
  const body = "## Context\n\nThe merge driver keeps the marker when the parser panics.";

  it("marks the matched run and leaves the rest plain", () => {
    const segments = snippet(body, "merge driver");
    expect(marked(segments)).toEqual(["merge driver"]);
    expect(text(segments)).toContain("keeps the marker");
  });

  it("matches regardless of case but shows the body's own casing", () => {
    expect(marked(snippet(body, "MERGE DRIVER"))).toEqual(["merge driver"]);
  });

  it("shows nothing when the query is not in the body", () => {
    expect(snippet(body, "kubernetes")).toEqual([]);
    expect(snippet("", "anything")).toEqual([]);
    expect(snippet(body, "   ")).toEqual([]);
  });

  it("keeps the preview near the requested width", () => {
    const long = `${"padding words ".repeat(40)}needle${" trailing words".repeat(40)}`;
    const segments = snippet(long, "needle", 60);
    expect(text(segments).length).toBeLessThan(90);
    expect(marked(segments)).toEqual(["needle"]);
  });

  it("ellipses only the side it actually cut", () => {
    const long = `needle${" word".repeat(60)}`;
    const segments = snippet(long, "needle");
    expect(text(segments).startsWith("needle")).toBe(true);
    expect(text(segments).endsWith("…")).toBe(true);
  });

  it("finds words that markdown had wrapped in markup", () => {
    expect(marked(snippet("the `%A` side", "%A"))).toEqual(["%A"]);
  });
});
