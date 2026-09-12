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
  issues: readonly (Pick<IssueDto, "short_ref" | "start" | "due" | "estimate"> & {
    blocked_by?: readonly string[];
  })[],
): ReadonlySet<string> {
  const byRef = new Map(issues.map((issue) => [issue.short_ref, issue]));
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
    const chain = longest(issue.short_ref, new Set());
    if (chain.length > best.length) best = chain;
  }
  return new Set(best.path);
}

/** Back-to-back scheduling from a day, in the order given: each issue starts
 *  where the previous one ended. Returns the dates to commit, so the caller
 *  can write them as ordinary field edits. */
export function scheduleSequentially(
  issues: readonly Pick<IssueDto, "short_ref" | "estimate">[],
  from: number,
): Array<{ short_ref: string; start: string; due: string }> {
  let cursor = from;
  return issues.map((issue) => {
    const days = durationDays(issue);
    const start = cursor;
    const end = cursor + (days - 1) * DAY_MS;
    cursor = end + DAY_MS;
    return { short_ref: issue.short_ref, start: toDay(start), due: toDay(end) };
  });
}
