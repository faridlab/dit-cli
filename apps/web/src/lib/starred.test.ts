// Stars are private to a browser, so storage is the whole contract: they
// must survive a reload, never reach git, and never break the sidebar when
// the stored value is missing, blocked or garbage.
import { beforeEach, describe, expect, it } from "vitest";
import { isStarred, loadStarred, resetStarred, STARRED_KEY, toggleStar } from "./starred";

describe("loadStarred", () => {
  it("reads a stored shortlist", () => {
    const storage = { getItem: () => JSON.stringify(["A", "B"]), setItem: () => undefined };
    expect([...loadStarred(storage)].sort()).toEqual(["A", "B"]);
  });

  it("treats nothing stored as no stars", () => {
    expect(loadStarred({ getItem: () => null, setItem: () => undefined }).size).toBe(0);
  });

  it("treats garbage as no stars rather than breaking", () => {
    const cases = ["not json", "42", '{"a":1}', '["ok", 7]'];
    for (const raw of cases) {
      expect(loadStarred({ getItem: () => raw, setItem: () => undefined }).size).toBe(0);
    }
  });

  it("survives storage that throws", () => {
    const blocked = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => undefined,
    };
    expect(loadStarred(blocked).size).toBe(0);
  });
});

describe("toggleStar", () => {
  beforeEach(() => {
    window.localStorage.clear();
    resetStarred();
  });

  it("stars, unstars, and persists both", () => {
    expect(isStarred("Q2R7VN8")).toBe(false);

    expect(toggleStar("Q2R7VN8")).toBe(true);
    expect(isStarred("Q2R7VN8")).toBe(true);
    expect(loadStarred(window.localStorage).has("Q2R7VN8")).toBe(true);

    expect(toggleStar("Q2R7VN8")).toBe(false);
    expect(isStarred("Q2R7VN8")).toBe(false);
    expect(loadStarred(window.localStorage).has("Q2R7VN8")).toBe(false);
  });

  it("keeps other stars when one is removed", () => {
    toggleStar("A");
    toggleStar("B");
    toggleStar("A");
    expect(isStarred("B")).toBe(true);
    expect(isStarred("A")).toBe(false);
  });

  it("picks up a shortlist stored before this session", () => {
    window.localStorage.setItem(STARRED_KEY, JSON.stringify(["M4WQ7TB"]));
    resetStarred();
    expect(isStarred("M4WQ7TB")).toBe(true);
  });
});
