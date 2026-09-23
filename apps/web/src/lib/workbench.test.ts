import { beforeEach, describe, expect, it } from "vitest";
import {
  activityOf,
  clampPanelWidth,
  defaultRoute,
  hasSidePanel,
  PANEL_DEFAULT,
  PANEL_MAX,
  PANEL_MIN,
  readPanelWidth,
  readSections,
  SECTION_MIN,
  shareDrag,
  writePanelWidth,
  withoutPeek,
  writeSection,
} from "./workbench";

beforeEach(() => localStorage.clear());

describe("which icon a screen belongs to", () => {
  it("lights Work for every issue screen and Plan for the three plan views", () => {
    expect(activityOf({ name: "board" })).toBe("work");
    expect(activityOf({ name: "issues", q: null, inbox: true })).toBe("work");
    expect(activityOf({ name: "issue", id: "X" })).toBe("work");
    expect(activityOf({ name: "new-issue" })).toBe("work");
    expect(activityOf({ name: "gantt" })).toBe("plan");
    expect(activityOf({ name: "roadmap" })).toBe("plan");
    expect(activityOf({ name: "docs", p: "docs/a.md" })).toBe("docs");
  });

  it("lands each icon somewhere it owns", () => {
    for (const id of ["home", "docs", "morse", "work", "flow", "plan", "search", "settings"] as const) {
      expect(activityOf(defaultRoute(id))).toBe(id);
    }
  });

  it("keeps the side panel out of Morse, which has its own explorer", () => {
    expect(hasSidePanel("morse")).toBe(false);
    expect(hasSidePanel("work")).toBe(true);
  });
});

describe("coming back to an activity", () => {
  it("returns to the same screen without the issue panel, and leaves other routes alone", () => {
    expect(withoutPeek({ name: "board", issue: "Q2R7" })).toEqual({ name: "board" });
    expect(withoutPeek({ name: "issues", q: "type = bug", issue: "Q2R7" })).toEqual({ name: "issues", q: "type = bug" });
    expect(withoutPeek({ name: "docs", p: "docs/a.md" })).toEqual({ name: "docs", p: "docs/a.md" });
    expect(withoutPeek({ name: "morse" })).toEqual({ name: "morse" });
    expect(withoutPeek({ name: "issue", id: "Q2R7" })).toEqual({ name: "issue", id: "Q2R7" });
  });
});

describe("what the reader arranged", () => {
  it("remembers the panel width within bounds", () => {
    expect(readPanelWidth()).toBe(PANEL_DEFAULT);
    writePanelWidth(9999);
    expect(readPanelWidth()).toBe(PANEL_MAX);
    writePanelWidth(10);
    expect(readPanelWidth()).toBe(PANEL_MIN);
    expect(clampPanelWidth(Number.NaN)).toBe(PANEL_DEFAULT);
  });

  it("remembers a folded section and a dragged height, one section at a time", () => {
    writeSection("board.columns", { collapsed: true });
    writeSection("board.cards", { height: 10 });
    const s = readSections();
    expect(s["board.columns"]).toEqual({ collapsed: true });
    expect(s["board.cards"]).toEqual({ height: SECTION_MIN });
    writeSection("board.columns", { height: 200 });
    expect(readSections()["board.columns"]).toEqual({ collapsed: true, height: 200 });
  });

  it("shares a drag between two neighbours without either going under the minimum", () => {
    expect(shareDrag(200, 200, 50)).toEqual([250, 150]);
    expect(shareDrag(200, 200, 500)).toEqual([400 - SECTION_MIN, SECTION_MIN]);
    expect(shareDrag(200, 200, -500)).toEqual([SECTION_MIN, 400 - SECTION_MIN]);
  });

  it("survives storage that holds something unreadable", () => {
    localStorage.setItem("dit.panel.sections", "{not json");
    expect(readSections()).toEqual({});
  });
});
