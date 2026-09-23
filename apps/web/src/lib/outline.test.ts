import { describe, expect, it } from "vitest";
import { outlineOf } from "./outline";

describe("a page's outline", () => {
  it("lists headings in order with their level, marks removed", () => {
    expect(outlineOf("# Workspace overview\n\ntext\n\n## The `api` [part](x.md)\n### Deep **one**\n")).toEqual([
      { level: 1, text: "Workspace overview" },
      { level: 2, text: "The api part" },
      { level: 3, text: "Deep one" },
    ]);
  });

  it("does not take a comment inside a code fence for a heading", () => {
    const doc = "# Real\n\n```bash\n# not a heading\n```\n\n~~~\n## nor this\n~~~\n## Also real\n";
    expect(outlineOf(doc).map((h) => h.text)).toEqual(["Real", "Also real"]);
  });

  it("needs a space after the hashes, as CommonMark does", () => {
    expect(outlineOf("#hashtag\n#### Four\n##### five is too deep")).toEqual([{ level: 4, text: "Four" }]);
  });
});
