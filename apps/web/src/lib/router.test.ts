// The URL is the app's shareable state: a filtered list, an open page, an
// open issue panel. These pin that every route survives a round trip through
// the hash, that the panel can be opened and closed over any list, and that
// a link someone pasted from an older build still lands somewhere sensible.
import { describe, expect, it } from "vitest";
import { parseHash, peekHost, peekOf, routeToHash, withPeek, type Route } from "./router";

const ROUTES: Route[] = [
  { name: "home", issue: null },
  { name: "home", issue: "Q2R7VN8" },
  { name: "board", issue: null },
  { name: "board", issue: "Q2R7VN8" },
  { name: "issues", q: null, issue: null },
  { name: "issues", q: "status != done", issue: null },
  { name: "issues", q: "status != done", issue: "Q2R7VN8" },
  { name: "issues", q: null, starred: true, issue: null },
  { name: "issues", q: null, starred: true, issue: "Q2R7VN8" },
  { name: "docs", p: null },
  { name: "docs", p: "docs/flows/auth-session.md" },
  { name: "search", q: "", issue: null },
  { name: "search", q: 'title ~ "merge driver"', issue: "Q2R7VN8" },
  { name: "timeline", issue: null, seq: null },
  { name: "timeline", issue: "Q2R7VN8", seq: 412 },
  { name: "roadmap", issue: null },
  { name: "gantt", issue: "Q2R7VN8" },
  { name: "issue", id: "Q2R7VN8", from: null },
  { name: "issue", id: "Q2R7VN8", from: "board" },
  { name: "new-issue" },
  { name: "settings" },
];

describe("route round trip", () => {
  it.each(ROUTES)("survives hash → parse → hash for %o", (route) => {
    const hash = routeToHash(route);
    expect(routeToHash(parseHash(hash))).toBe(hash);
  });

  it("keeps query values intact through encoding", () => {
    const route: Route = { name: "issues", q: 'label = context:computer AND title ~ "a&b"', issue: null };
    expect(parseHash(routeToHash(route))).toMatchObject(route);
  });

  it("omits empty values instead of writing bare keys", () => {
    expect(routeToHash({ name: "issues", q: null, issue: null })).toBe("#/issues");
    expect(routeToHash({ name: "board" })).toBe("#/board");
  });
});

describe("parseHash", () => {
  it("lands on home for an unknown or empty hash", () => {
    expect(parseHash("")).toEqual({ name: "home", issue: null });
    expect(parseHash("#/nowhere")).toEqual({ name: "home", issue: null });
  });

  it("reads an issue page opened before `from` existed", () => {
    expect(parseHash("#/issue/Q2R7VN8")).toEqual({ name: "issue", id: "Q2R7VN8", from: null });
  });

  it("ignores a `from` that names no panel host", () => {
    expect(parseHash("#/issue/Q2R7VN8?from=settings")).toEqual({
      name: "issue",
      id: "Q2R7VN8",
      from: null,
    });
  });

  it("treats an empty parameter as absent", () => {
    expect(parseHash("#/issues?q=&issue=")).toEqual({
      name: "issues",
      q: null,
      issue: null,
      starred: false,
    });
  });

  it("reads the starred flag only when it is set", () => {
    expect(parseHash("#/issues?starred=1")).toMatchObject({ starred: true });
    expect(parseHash("#/issues")).toMatchObject({ starred: false });
    expect(parseHash("#/issues?starred=0")).toMatchObject({ starred: false });
  });
});

describe("the open panel", () => {
  it("is readable from every list route and from none of the others", () => {
    expect(peekOf({ name: "board", issue: "Q2R7VN8" })).toBe("Q2R7VN8");
    expect(peekOf({ name: "issues", q: null, issue: "Q2R7VN8" })).toBe("Q2R7VN8");
    expect(peekOf({ name: "board" })).toBeNull();
    expect(peekOf({ name: "docs", p: null })).toBeNull();
    expect(peekOf({ name: "issue", id: "Q2R7VN8" })).toBeNull();
  });

  it("opens and closes over a list without disturbing its filter", () => {
    const filtered: Route = { name: "issues", q: "status != done", issue: null };
    const opened = withPeek(filtered, "Q2R7VN8");
    expect(opened).toMatchObject({ name: "issues", q: "status != done", issue: "Q2R7VN8" });
    expect(routeToHash(withPeek(opened, null))).toBe(routeToHash(filtered));
  });

  it("keeps the starred list starred while a panel is open over it", () => {
    const starred: Route = { name: "issues", q: null, starred: true, issue: null };
    expect(withPeek(starred, "Q2R7VN8")).toMatchObject({ starred: true, issue: "Q2R7VN8" });
  });

  it("falls back to the list the page was opened from", () => {
    expect(withPeek({ name: "issue", id: "A", from: "board" }, "A")).toEqual({
      name: "board",
      issue: "A",
    });
    expect(withPeek({ name: "issue", id: "A", from: null }, "A")).toMatchObject({
      name: "issues",
      q: null,
      issue: "A",
    });
  });

  it("hosts the panel over the plan views too", () => {
    expect(peekOf({ name: "gantt", issue: "Q2R7VN8" })).toBe("Q2R7VN8");
    expect(withPeek({ name: "gantt" }, "A")).toMatchObject({ name: "gantt", issue: "A" });
    expect(peekHost({ name: "issue", id: "A", from: "gantt" })).toBe("gantt");
  });

  it("keeps the point in history while a panel is open over the timeline", () => {
    expect(withPeek({ name: "timeline", seq: 412 }, "A")).toMatchObject({ seq: 412, issue: "A" });
  });

  it("ignores a point in history that is not a number", () => {
    expect(parseHash("#/timeline?seq=yesterday")).toMatchObject({ seq: null });
  });

  it("sends views that cannot host a panel to the issues list", () => {
    expect(withPeek({ name: "settings" }, "A")).toMatchObject({ name: "issues", q: null, issue: "A" });
    expect(peekHost({ name: "docs", p: null })).toBe("issues");
    expect(peekHost({ name: "board" })).toBe("board");
    expect(peekHost({ name: "issue", id: "A", from: "search" })).toBe("search");
  });
});
