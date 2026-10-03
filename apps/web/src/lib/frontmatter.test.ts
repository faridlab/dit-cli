// Found in use: a page made from a template carries frontmatter (`kind`,
// `title`, ADR 0031), and the rich editor read the whole file as Markdown —
// the opening `---` a rule, the fields a heading. The first autosave would
// have written them back as a heading. The editor now edits the body only;
// the frontmatter rides along untouched.

import { describe, expect, it } from "vitest";
import { joinFrontmatter, propertiesOf, splitFrontmatter } from "./frontmatter";

const page = "---\nkind: fsd\ntitle: Approvals\n---\n# Approvals\n\n*Prompt.*\n";

describe("a page's frontmatter in the rich editor", () => {
  it("is kept apart from the body and put back exactly", () => {
    const { front, body } = splitFrontmatter(page);
    expect(front).toBe("---\nkind: fsd\ntitle: Approvals\n---\n");
    expect(body).toBe("# Approvals\n\n*Prompt.*\n");
    expect(joinFrontmatter(front, body)).toBe(page);
    // An edit to the body keeps the frontmatter as it was.
    expect(joinFrontmatter(front, "# Approvals\n\nWritten.\n")).toBe(
      "---\nkind: fsd\ntitle: Approvals\n---\n# Approvals\n\nWritten.\n",
    );
  });

  it("leaves a page without frontmatter, or with a rule later on, alone", () => {
    expect(splitFrontmatter("# Title\n\n---\n\nAfter a rule.\n")).toEqual({
      front: "",
      body: "# Title\n\n---\n\nAfter a rule.\n",
    });
    // An opening delimiter that never closes is body, not frontmatter.
    expect(splitFrontmatter("---\nno end\n").front).toBe("");
  });

  it("names the fields for the line above the editor", () => {
    expect(propertiesOf("---\nkind: fsd\ntitle: \"A: B\"\n---\n")).toEqual([
      ["kind", "fsd"],
      ["title", "A: B"],
    ]);
    expect(propertiesOf("")).toEqual([]);
  });
});
