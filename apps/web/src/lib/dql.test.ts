// The palette has to tell two kinds of typing apart: words to search for,
// and a query to run. Getting it wrong in either direction is annoying —
// a query silently treated as words returns nothing useful, and words
// treated as a query hand the server something it will reject.
import { describe, expect, it } from "vitest";
import { looksLikeDql, mineQuery, openFragment, openQuery } from "./dql";
import type { StatusDto } from "./types";

const status = (id: string, category: StatusDto["category"]): StatusDto => ({
  id,
  label: id,
  category,
  terminal: category === "done",
  wip_limit: null,
});

const WORKFLOW = [status("todo", "todo"), status("doing", "doing"), status("done", "done")];

describe("looksLikeDql", () => {
  it("recognizes comparisons", () => {
    expect(looksLikeDql("status = done")).toBe(true);
    expect(looksLikeDql("priority <= p1")).toBe(true);
    expect(looksLikeDql("assignee != farid")).toBe(true);
    expect(looksLikeDql('title ~ "merge driver"')).toBe(true);
  });

  it("recognizes the connectives, whatever their case", () => {
    expect(looksLikeDql("label = next AND assignee = @me")).toBe(true);
    expect(looksLikeDql("type = bug or type = spike")).toBe(true);
  });

  it("leaves ordinary words alone", () => {
    expect(looksLikeDql("merge driver")).toBe(false);
    expect(looksLikeDql("login timeout on slow networks")).toBe(false);
    expect(looksLikeDql("")).toBe(false);
  });

  it("does not mistake a word containing and/or for a query", () => {
    expect(looksLikeDql("android")).toBe(false);
    expect(looksLikeDql("ordering")).toBe(false);
  });

  it("does not mistake an arrow or a hyphen for an operator", () => {
    expect(looksLikeDql("drop-down menu")).toBe(false);
    expect(looksLikeDql("a -> b")).toBe(false);
  });
});

describe("open and mine queries", () => {
  it("spells out open as every status the workflow does not call done", () => {
    expect(openFragment(WORKFLOW)).toBe("status != done");
  });

  it("excludes every done status when a workflow has several", () => {
    const shipped = [...WORKFLOW, status("wontfix", "done")];
    expect(openFragment(shipped)).toBe("status != done AND status != wontfix");
  });

  it("filters nothing until the workflow is known", () => {
    expect(openFragment(undefined)).toBeNull();
    expect(openQuery([])).toBeNull();
  });

  it("scopes my issues to me and to open work", () => {
    expect(mineQuery(WORKFLOW)).toBe("assignee = @me AND status != done ORDER BY priority ASC");
  });

  it("still scopes to me with no workflow loaded", () => {
    expect(mineQuery(undefined)).toBe("assignee = @me ORDER BY priority ASC");
  });
});

describe("looksLikeDql — set membership and bare match", () => {
  it("reads IN and NOT IN as queries, not as words", () => {
    expect(looksLikeDql('id IN ("01K3M", "01K3N")')).toBe(true);
    expect(looksLikeDql("label NOT IN (auth, api)")).toBe(true);
  });

  it("reads a leading ~ as full text spelled as a query", () => {
    expect(looksLikeDql('~ "merge driver"')).toBe(true);
  });

  it("still reads ordinary words as words", () => {
    expect(looksLikeDql("merge driver")).toBe(false);
    expect(looksLikeDql("what goes in a release")).toBe(false);
  });
});
