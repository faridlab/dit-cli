// Small presentation helpers shared by every view. Kept pure so they are
// trivial to reason about: no dates are parsed with anything heavier than
// Date.parse, no dependency is worth pulling in for this.

import type { IssueType, Priority } from "./types";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

export function relativeTime(iso: string, now: number = Date.now()): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return iso;
  const delta = now - ts;
  if (delta < 0) return "just now";
  if (delta < MINUTE) return "just now";
  if (delta < HOUR) return `${Math.floor(delta / MINUTE)}m ago`;
  if (delta < DAY) return `${Math.floor(delta / HOUR)}h ago`;
  if (delta < 7 * DAY) return `${Math.floor(delta / DAY)}d ago`;
  // Older than a week: unambiguous calendar date, timezone-independent.
  return iso.slice(0, 10);
}

export function fullTimestamp(iso: string): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return iso;
  return new Date(ts).toLocaleString();
}

// Two-letter monogram for assignee circles: "Farid Hidayat" -> "FH",
// "farid" -> "FA". Aliases are free-form, so degrade gracefully.
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "?";
  if (parts.length === 1) {
    const word = parts[0] ?? "";
    return (word.slice(0, 2) || "?").toUpperCase();
  }
  const first = parts[0] ?? "";
  const second = parts[1] ?? "";
  return `${first.charAt(0)}${second.charAt(0)}`.toUpperCase();
}

// Deterministic background class per person so the same alias always gets
// the same color, without storing anything.
const CIRCLE_COLORS = [
  "bg-sky-600",
  "bg-violet-600",
  "bg-emerald-600",
  "bg-rose-600",
  "bg-amber-600",
  "bg-teal-600",
];

export function circleColor(name: string): string {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  const index = Math.abs(hash) % CIRCLE_COLORS.length;
  return CIRCLE_COLORS[index] ?? "bg-sky-600";
}

// Priority is a colored dot, never a word — the board and list stay
// scannable. p0 is on fire; p4 is whenever; unset is a hollow dot.
export function priorityDot(priority: Priority | null): string {
  switch (priority) {
    case "p0":
      return "bg-red-500";
    case "p1":
      return "bg-orange-400";
    case "p2":
      return "bg-sky-400";
    case "p3":
      return "bg-zinc-500";
    case "p4":
      return "bg-zinc-700";
    default:
      return "border border-zinc-600";
  }
}

// One letter per non-default type; plain tasks carry no badge so the
// board reads calm — most work is tasks.
export function typeLetter(type: IssueType): string | null {
  switch (type) {
    case "bug":
      return "B";
    case "story":
      return "S";
    case "spike":
      return "K";
    case "chore":
      return "C";
    default:
      return null;
  }
}

export function typeBadgeClass(type: IssueType): string {
  switch (type) {
    case "bug":
      return "bg-red-900 text-red-400";
    case "story":
      return "bg-violet-950 text-violet-400";
    case "spike":
      return "bg-amber-900 text-amber-400";
    case "chore":
      return "bg-zinc-700 text-zinc-400";
    default:
      return "bg-zinc-700 text-zinc-500";
  }
}

/** Due dates read as neutral until they are today — then they turn orange,
 * the only escalation color outside priorities. Past due escalates too. */
export function dueTone(iso: string | null): string {
  if (!iso) return "text-zinc-500";
  const today = new Date().toISOString().slice(0, 10);
  return iso.slice(0, 10) <= today ? "text-orange-400" : "text-zinc-500";
}

/** Compact due copy for dense rows: "today", "in 3d", "2d overdue". */
export function dueText(iso: string | null): string {
  if (!iso) return "—";
  const today = new Date().toISOString().slice(0, 10);
  const day = iso.slice(0, 10);
  if (day === today) return "today";
  const diff = Math.round((Date.parse(day) - Date.parse(today)) / 86_400_000);
  if (Number.isNaN(diff)) return iso;
  if (diff > 0) return `in ${diff}d`;
  return `${-diff}d overdue`;
}

/** The context a piece of work happens in (@computer, @home, …), encoded as
 * a label so it stays plain data in git. Feeds the Home grouping. */
export function contextOf(labels: ReadonlyArray<string>): string | null {
  for (const label of labels) {
    if (label.startsWith("context:")) return label.slice("context:".length) || null;
  }
  return null;
}

/** The effort an issue takes right now (high/medium/low), as a label. */
export function energyOf(labels: ReadonlyArray<string>): string {
  for (const label of labels) {
    if (label.startsWith("energy:")) return label.slice("energy:".length);
  }
  return "—";
}

// Numeric rank so "sort by priority" means something; higher = hotter.
// Unset sorts as the coldest — work with no priority is not urgent work.
export function priorityRank(priority: Priority | null): number {
  switch (priority) {
    case "p0":
      return 4;
    case "p1":
      return 3;
    case "p2":
      return 2;
    case "p3":
      return 1;
    case "p4":
      return 0;
    default:
      return -1;
  }
}

export function parseCsvList(value: string): string[] {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter((item) => item.length > 0);
}

export function shortSha(sha: string | null, length = 7): string {
  if (!sha) return "—";
  return sha.slice(0, length);
}
