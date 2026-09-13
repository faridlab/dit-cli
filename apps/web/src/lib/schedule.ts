// Turning issues into bars on a time axis, without inventing data.
//
// `start` and `due` are real fields in the file. Most issues carry neither,
// and demanding both before an issue can appear on a plan would make the
// plan a second place to maintain — so a missing side is *inferred* from the
// estimate and drawn as an open edge. Inference is never written back: the
// bar is a reading of the issue, and only a drag commits anything.
//
// Everything here is pure: dates in, geometry out. The clock arrives as an
// argument so the views stay testable.

import type { IssueDto } from "./types";

export const DAY_MS = 86_400_000;

/** Points to days. An estimate is effort, not duration, and two days per
 *  point is the conventional starting guess — visible in the UI as an
 *  inferred (dashed) edge, never as a stored date. */
export const DAYS_PER_POINT = 2;

export interface Span {
  /** Start of day, UTC. */
  start: number;
  /** End of the last day, exclusive. Always at least one day after `start`. */
  end: number;
  /** The start was inferred from the estimate rather than read from a field. */
  inferredStart: boolean;
  inferredEnd: boolean;
}

/** Midnight UTC of a `YYYY-MM-DD` (or of the day an RFC3339 stamp falls on). */
export function dayStart(iso: string): number {
  return Date.parse(`${iso.slice(0, 10)}T00:00:00Z`);
}

/** `YYYY-MM-DD` for a timestamp, UTC — the shape the files store. */
export function toDay(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

/** How long an issue is expected to take, in whole days. Without an estimate
 *  the assumption is one day: enough to be visible, small enough to be
 *  obviously a placeholder. */
export function durationDays(issue: Pick<IssueDto, "estimate">): number {
  return Math.max(1, (issue.estimate ?? 0) * DAYS_PER_POINT || 1);
}

/** The bar for one issue, or null when it has no dates at all — an
 *  unscheduled issue belongs in the tray, not somewhere on the axis. */
export function spanOf(issue: Pick<IssueDto, "start" | "due" | "estimate">): Span | null {
  if (!issue.start && !issue.due) return null;
  const length = durationDays(issue) * DAY_MS;
  const start = issue.start ? dayStart(issue.start) : dayStart(issue.due ?? "") - length;
  // `due` names the last day of work, so the bar covers it.
  const end = issue.due ? dayStart(issue.due) + DAY_MS : start + length;
  return {
    start,
    end: Math.max(end, start + DAY_MS),
    inferredStart: !issue.start,
    inferredEnd: !issue.due,
  };
}

/** The smallest span covering all of them, or null when none has dates. */
export function coveringSpan(spans: readonly (Span | null)[]): Span | null {
  const real = spans.filter((s): s is Span => s !== null);
  if (real.length === 0) return null;
  return {
    start: Math.min(...real.map((s) => s.start)),
    end: Math.max(...real.map((s) => s.end)),
    inferredStart: real.every((s) => s.inferredStart),
    inferredEnd: real.every((s) => s.inferredEnd),
  };
}

/** An epic's bar: its own dates when it has them, otherwise the span of its
 *  children — derived on read, never stored (invariant 5). */
export function epicSpan(
  epic: Pick<IssueDto, "start" | "due" | "estimate">,
  children: readonly Pick<IssueDto, "start" | "due" | "estimate">[],
): { span: Span; derived: boolean } | null {
  if (epic.start && epic.due) {
    const own = spanOf(epic);
    return own ? { span: own, derived: false } : null;
  }
  const fromChildren = coveringSpan(children.map(spanOf));
  if (fromChildren) return { span: fromChildren, derived: true };
  const own = spanOf(epic);
  return own ? { span: own, derived: false } : null;
}

/** Is this bar overdue: past its end, and not finished. */
export function isLate(span: Span, done: boolean, now: number): boolean {
  return !done && span.end <= now;
}

/** The longest chain of blocked-by dependencies, by total duration — the
 *  run of work where one day of slip is one day of slip for the whole plan.
 *
 *  Cycles are data, not a crash: a chain that revisits an issue stops there
 *  rather than recursing forever, because `blocked_by` is two people's edits
 *  merged and nothing forbids them from disagreeing.
 */
export function criticalPath(
  issues: readonly (Pick<IssueDto, "id" | "start" | "due" | "estimate"> & {
    blocked_by?: readonly string[];
  })[],
): ReadonlySet<string> {
  // `blocked_by` names issues by id, not by short ref, so the walk is keyed
  // the same way.
  const byRef = new Map(issues.map((issue) => [issue.id, issue]));
  const memo = new Map<string, { length: number; path: string[] }>();

  const longest = (ref: string, seen: ReadonlySet<string>): { length: number; path: string[] } => {
    const cached = memo.get(ref);
    if (cached) return cached;
    if (seen.has(ref)) return { length: 0, path: [] };
    const issue = byRef.get(ref);
    if (!issue) return { length: 0, path: [] };

    const span = spanOf(issue);
    const own = span ? (span.end - span.start) / DAY_MS : durationDays(issue);
    const walked = new Set(seen).add(ref);

    let best = { length: 0, path: [] as string[] };
    for (const blocker of issue.blocked_by ?? []) {
      const chain = longest(blocker, walked);
      if (chain.length > best.length) best = chain;
    }
    const result = { length: own + best.length, path: [ref, ...best.path] };
    // Only cacheable when the walk did not stop early on a cycle.
    if (best.path.every((step) => !seen.has(step))) memo.set(ref, result);
    return result;
  };

  let best = { length: 0, path: [] as string[] };
  for (const issue of issues) {
    const chain = longest(issue.id, new Set());
    if (chain.length > best.length) best = chain;
  }
  return new Set(best.path);
}

/** Back-to-back scheduling from a day, in the order given: each issue starts
 *  where the previous one ended. Returns the dates to commit, so the caller
 *  can write them as ordinary field edits. */
export function scheduleSequentially(
  issues: readonly Pick<IssueDto, "id" | "estimate">[],
  from: number,
): Array<{ id: string; start: string; due: string }> {
  let cursor = from;
  return issues.map((issue) => {
    const days = durationDays(issue);
    const start = cursor;
    const end = cursor + (days - 1) * DAY_MS;
    cursor = end + DAY_MS;
    return { id: issue.id, start: toDay(start), due: toDay(end) };
  });
}

// ---------------------------------------------------------------------------
// Axes. The two plan views each pick a window around today and split it into
// months; the arithmetic lives here so the windows are testable and the views
// only place things.
// ---------------------------------------------------------------------------

export const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "Sep 7" — the shape every axis label and tooltip uses. */
export function shortDate(ms: number): string {
  const date = new Date(ms);
  return `${MONTHS[date.getUTCMonth()]} ${date.getUTCDate()}`;
}

export type GanttZoom = "day" | "week" | "month";

export interface GanttRange {
  /** First day on the canvas, midnight UTC. */
  start: number;
  days: number;
  /** Pixels per day. */
  ppd: number;
}

/** The Gantt window at each zoom. Day zoom starts a week ago so the recent
 *  past is visible; week zoom snaps to a Monday so the ticks read as weeks;
 *  month zoom starts on the first of a month so the month bands line up. */
export function ganttRange(zoom: GanttZoom, today: number): GanttRange {
  if (zoom === "day") return { start: today - 7 * DAY_MS, days: 28, ppd: 40 };
  if (zoom === "month") {
    const date = new Date(today);
    date.setUTCDate(1);
    date.setUTCMonth(date.getUTCMonth() - 2);
    return { start: date.getTime(), days: 183, ppd: 6 };
  }
  const monday = new Date(today);
  const weekday = (monday.getUTCDay() + 6) % 7;
  monday.setUTCDate(monday.getUTCDate() - weekday - 21);
  return { start: monday.getTime(), days: 84, ppd: 14 };
}

export type RoadmapHorizon = "quarter" | "half" | "year";

/** The roadmap window: whole months, so quarter boundaries land on edges. */
export function roadmapRange(horizon: RoadmapHorizon, today: number): { start: number; end: number } {
  const start = new Date(today);
  start.setUTCDate(1);
  if (horizon === "quarter") {
    start.setUTCMonth(Math.floor(start.getUTCMonth() / 3) * 3);
    const end = new Date(start);
    end.setUTCMonth(end.getUTCMonth() + 3);
    return { start: start.getTime(), end: end.getTime() };
  }
  if (horizon === "year") {
    start.setUTCMonth(start.getUTCMonth() - 3);
    const end = new Date(start);
    end.setUTCMonth(end.getUTCMonth() + 12);
    return { start: start.getTime(), end: end.getTime() };
  }
  start.setUTCMonth(start.getUTCMonth() - 2);
  const end = new Date(start);
  end.setUTCMonth(end.getUTCMonth() + 6);
  return { start: start.getTime(), end: end.getTime() };
}

export interface MonthSegment {
  /** Clipped to the range at both ends. */
  start: number;
  end: number;
  /** 0-based, as `Date` counts. */
  month: number;
  year: number;
  /** "Sep 2026" */
  label: string;
}

/** The months a range touches, each clipped to the range. */
export function monthSegments(start: number, end: number): MonthSegment[] {
  const out: MonthSegment[] = [];
  let cursor = start;
  while (cursor < end) {
    const date = new Date(cursor);
    const monthStart = Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1);
    const nextMonth = Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1);
    out.push({
      start: Math.max(monthStart, start),
      end: Math.min(nextMonth, end),
      month: date.getUTCMonth(),
      year: date.getUTCFullYear(),
      label: `${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}`,
    });
    cursor = nextMonth;
  }
  return out;
}

/** Day offsets from `start` that fall on a Saturday or Sunday. */
export function weekendOffsets(start: number, days: number): number[] {
  const out: number[] = [];
  for (let offset = 0; offset < days; offset += 1) {
    const weekday = new Date(start + offset * DAY_MS).getUTCDay();
    if (weekday === 0 || weekday === 6) out.push(offset);
  }
  return out;
}
