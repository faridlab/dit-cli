// The workbench's three regions, VS Code-style: an activity bar of icons, a
// side panel with the chosen activity's sub-menu on top, and the view's own
// sections split below it.
//
// This module is the pure half: which activity owns which route, and what the
// reader has set by hand — the panel's width, which sections are folded, how
// tall a dragged section is. All of it is remembered per browser and never in
// the repo: how someone arranges their screen says nothing about the plan.

import type { Route } from "./router";

export type ActivityId = "home" | "docs" | "morse" | "work" | "flow" | "plan" | "search" | "settings";

/** The activity each route belongs to — the icon lit up while it is open. */
export function activityOf(route: Route): ActivityId {
  switch (route.name) {
    case "home":
      return "home";
    case "docs":
      return "docs";
    case "morse":
      return "morse";
    case "board":
    case "issues":
    case "issue":
    case "new-issue":
      return "work";
    case "flow":
      return "flow";
    case "timeline":
    case "roadmap":
    case "gantt":
      return "plan";
    case "search":
      return "search";
    case "settings":
      return "settings";
  }
}

/** Where an activity lands when nothing else was open in it yet. */
export function defaultRoute(activity: ActivityId): Route {
  switch (activity) {
    case "home":
      return { name: "home" };
    case "docs":
      return { name: "docs", p: null };
    case "morse":
      return { name: "morse" };
    case "work":
      return { name: "board" };
    case "flow":
      return { name: "flow" };
    case "plan":
      return { name: "timeline" };
    case "search":
      return { name: "search", q: "" };
    case "settings":
      return { name: "settings" };
  }
}

/** A route as an activity should return to it: the same screen, without the
 *  issue that happened to be open in the side panel over it. Only routes
 *  that carry an `issue` are touched — the rest are returned as they are,
 *  so Docs comes back to the page it was on. */
export function withoutPeek(route: Route): Route {
  if (route.name === "issue" || !("issue" in route) || route.issue == null) return route;
  const { issue: _gone, ...rest } = route;
  return rest as Route;
}

/** The ⌘1..⌘9 and ⌘0 order. Kept exactly as the labelled rail had it, so
 *  nobody's muscle memory moves with the layout — the activity bar's
 *  tooltips print these keys. */
export const SHORTCUT_VIEWS: Route[] = [
  { name: "home" },
  { name: "search", q: "" },
  { name: "board" },
  { name: "issues", q: null },
  { name: "docs", p: null },
  { name: "timeline" },
  { name: "roadmap" },
  { name: "gantt" },
  { name: "flow" },
  { name: "morse" },
];

/** Morse keeps its own explorer and tabs inside the screen (ADR 0023), so
 *  the side panel stays out of its way rather than stacking a second one. */
export function hasSidePanel(activity: ActivityId): boolean {
  return activity !== "morse";
}

// ---- the side panel's width ------------------------------------------------

const WIDTH_KEY = "dit.panel.width";
export const PANEL_MIN = 200;
export const PANEL_MAX = 520;
export const PANEL_DEFAULT = 272;

export function clampPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return PANEL_DEFAULT;
  return Math.round(Math.max(PANEL_MIN, Math.min(PANEL_MAX, width)));
}

export function readPanelWidth(): number {
  try {
    const raw = localStorage.getItem(WIDTH_KEY);
    return raw === null ? PANEL_DEFAULT : clampPanelWidth(Number(raw));
  } catch {
    return PANEL_DEFAULT;
  }
}

export function writePanelWidth(width: number): void {
  try {
    localStorage.setItem(WIDTH_KEY, String(clampPanelWidth(width)));
  } catch {
    /* a private window keeps nothing */
  }
}

// ---- sections in the lower split ---------------------------------------------

const SECTIONS_KEY = "dit.panel.sections";
/** Enough to show a heading and a couple of rows, or dragging a section
 *  smaller would hide the thing the drag exists to reveal. */
export const SECTION_MIN = 64;

export interface SectionState {
  collapsed?: boolean;
  /** Pixels, once the reader has dragged it. Absent means "its content". */
  height?: number;
}

export function readSections(): Record<string, SectionState> {
  try {
    const raw = localStorage.getItem(SECTIONS_KEY);
    const parsed: unknown = raw === null ? {} : JSON.parse(raw);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, SectionState>) : {};
  } catch {
    return {};
  }
}

export function writeSection(id: string, patch: SectionState): Record<string, SectionState> {
  const all = readSections();
  const next = { ...all[id], ...patch };
  if (next.height !== undefined) next.height = Math.max(SECTION_MIN, Math.round(next.height));
  all[id] = next;
  try {
    localStorage.setItem(SECTIONS_KEY, JSON.stringify(all));
  } catch {
    /* a private window keeps nothing */
  }
  return all;
}

/** Split a drag between two neighbouring sections: what one gains the
 *  other gives, and neither goes below the minimum. */
export function shareDrag(above: number, below: number, delta: number): [number, number] {
  const total = above + below;
  const nextAbove = Math.max(SECTION_MIN, Math.min(total - SECTION_MIN, above + delta));
  return [nextAbove, total - nextAbove];
}
