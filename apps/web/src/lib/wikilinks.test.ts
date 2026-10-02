import { describe, expect, it } from "vitest";
import { matchWikiItems, resolveWikiTarget, wikiItemsFrom } from "./wikilinks";

const docs = [{ path: "docs/flows/auth-session.md" }, { path: "docs/guide.md" }, { path: "notes/guide.md" }];
const issues = [
  { short_ref: "Q2R7VN8", title: "Fix login timeout", number: 12 },
  { short_ref: "FKZKJCW", title: "Checklist | and ] breaks", number: 3 },
];
const items = wikiItemsFrom(docs, issues);

describe("wikiItemsFrom", () => {
  it("lists issues by title and pages by path, with nothing that would break the link syntax", () => {
    expect(items.map((i) => i.target)).toEqual(["Q2R7VN8", "FKZKJCW", "docs/flows/auth-session", "docs/guide", "notes/guide"]);
    expect(items[1]?.label).toBe("Checklist   and   breaks");
    expect(items[0]?.detail).toBe("#12");
  });
});

describe("matchWikiItems", () => {
  it("ranks a word that starts a field above one found inside it", () => {
    expect(matchWikiItems(items, "log").map((i) => i.target)).toEqual(["Q2R7VN8"]);
    expect(matchWikiItems(items, "session")[0]?.target).toBe("docs/flows/auth-session");
    expect(matchWikiItems(items, "ui").map((i) => i.target)).toEqual(["docs/guide", "notes/guide"]);
  });

  it("finds an issue by its number or its ref", () => {
    expect(matchWikiItems(items, "#12")[0]?.target).toBe("Q2R7VN8");
    expect(matchWikiItems(items, "fkzk")[0]?.target).toBe("FKZKJCW");
  });

  it("offers the first few when nothing is typed yet", () => {
    expect(matchWikiItems(items, "", 2)).toHaveLength(2);
  });
});

describe("resolveWikiTarget", () => {
  it("opens an issue by its short ref", () => {
    expect(resolveWikiTarget("Q2R7VN8", docs)).toEqual({ name: "issue", id: "Q2R7VN8" });
  });

  it("opens the one page a path or a unique suffix names", () => {
    expect(resolveWikiTarget("docs/flows/auth-session", docs)).toEqual({ name: "docs", p: "docs/flows/auth-session.md" });
    expect(resolveWikiTarget("auth-session", docs)).toEqual({ name: "docs", p: "docs/flows/auth-session.md" });
  });

  it("refuses to guess between two pages, or to open one that is not there", () => {
    expect(resolveWikiTarget("guide", docs)).toBeNull();
    expect(resolveWikiTarget("missing", docs)).toBeNull();
  });
});
