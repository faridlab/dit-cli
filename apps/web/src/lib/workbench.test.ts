import { beforeEach, describe, expect, it } from "vitest";
import {
  activitiesFor,
  activityOf,
  clampPanelWidth,
  defaultRoute,
  hasSidePanel,
  PANEL_DEFAULT,
  PANEL_MAX,
  PANEL_MIN,
  readPanelWidth,
  readSections,
  routeInMode,
  serveMode,
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
    for (const id of ["home", "docs", "morse", "work", "flow", "code", "plan", "search", "settings"] as const) {
      expect(activityOf(defaultRoute(id))).toBe(id);
    }
  });

  it("keeps the side panel out of Morse, which has its own explorer", () => {
    expect(hasSidePanel("morse")).toBe(false);
    expect(hasSidePanel("code")).toBe(false);
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

describe("code-only mode", () => {
  it("reads the mode from the status answer, and knows nothing before it arrives", () => {
    expect(serveMode(undefined)).toBeNull();
    expect(serveMode({ mode: "code" })).toBe("code");
    expect(serveMode({ mode: "workspace" })).toBe("workspace");
    // A server from before the field existed is a workspace.
    expect(serveMode({})).toBe("workspace");
  });

  it("offers only the code map in code mode, and everything in a workspace", () => {
    expect(activitiesFor("code")).toEqual(["code"]);
    expect(activitiesFor("workspace")).toContain("code");
    expect(activitiesFor("workspace")).toContain("settings");
    expect(activitiesFor("workspace")).toHaveLength(9);
  });

  it("sends every other route to the code map in code mode", () => {
    expect(routeInMode({ name: "board" }, "code")).toEqual({ name: "code" });
    expect(routeInMode({ name: "settings" }, "code")).toEqual({ name: "code" });
    const deep = { name: "code", root: "web", folder: "src", focus: null, view: "folder" } as const;
    expect(routeInMode(deep, "code")).toBe(deep);
    expect(routeInMode({ name: "board" }, "workspace")).toEqual({ name: "board" });
  });
});
