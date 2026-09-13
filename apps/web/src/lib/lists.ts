// The lists the sidebar names — Inbox, My issues, the open pool — as pure
// predicates over issues. They are computed from one shared query (the open
// pool) rather than from a private filter language, and each is shown next
// to the plain-language definition the design uses, never as a fake DQL.

import type { IssueDto, StatusDto } from "./types";
import { contextOf } from "./format";

/** The open pool: every issue whose status is not in the `done` category.
 *  Bounded so Home and the sidebar stay fast on big workspaces; the counts
 *  are labeled from it, not pretended to be exhaustive. */
export const POOL_LIMIT = 500;

export function doneIds(statuses: readonly StatusDto[] | undefined): ReadonlySet<string> {
  return new Set((statuses ?? []).filter((status) => status.category === "done").map((status) => status.id));
}

export function isDone(issue: Pick<IssueDto, "status">, done: ReadonlySet<string>): boolean {
  return done.has(issue.status);
}

/** Inbox: open, not an epic, and either nobody owns it or it has no
 *  @context yet — decide, delegate, or drop. */
export function isInbox(issue: IssueDto, done: ReadonlySet<string>): boolean {
  if (isDone(issue, done) || issue.type === "story") return false;
  return issue.assignees.length === 0 || contextOf(issue.labels) === null;
}

export const INBOX_DEFINITION = "no owner or no @context — decide, delegate, or drop";

/** Next actions: open, not an epic, assigned to me, with a context. */
export function isNext(issue: IssueDto, done: ReadonlySet<string>, me: string | null): boolean {
  if (me === null || isDone(issue, done) || issue.type === "story") return false;
  return issue.assignees.includes(me) && contextOf(issue.labels) !== null;
}

export const NEXT_DEFINITION = 'status != done and assignee = @me and label ~ "context:"';

/** The urgent-first order every list defaults to: p0 before p4, unset last. */
export function byPriority(a: Pick<IssueDto, "priority">, b: Pick<IssueDto, "priority">): number {
  return (a.priority ?? "p9").localeCompare(b.priority ?? "p9");
}

/** Sidebar filters shared by Board, Issues, Search and Gantt. */
export interface ListFilters {
  mine: boolean;
  contexts: ReadonlySet<string>;
  types: ReadonlySet<string>;
}

export const NO_FILTERS: ListFilters = { mine: false, contexts: new Set(), types: new Set() };

export function filterCount(filters: ListFilters): number {
  return (filters.mine ? 1 : 0) + filters.contexts.size + filters.types.size;
}

export function matchesFilters(issue: IssueDto, filters: ListFilters, me: string | null): boolean {
  if (filters.mine && (me === null || !issue.assignees.includes(me))) return false;
  if (filters.contexts.size > 0) {
    const context = contextOf(issue.labels);
    if (context === null || !filters.contexts.has(context)) return false;
  }
  if (filters.types.size > 0 && !filters.types.has(issue.type)) return false;
  return true;
}

/** The filters as the DQL a person would type — shown in the filters bar
 *  and runnable on the Search page. */
export function filtersToDql(filters: ListFilters): string[] {
  const parts: string[] = [];
  if (filters.mine) parts.push("assignee = @me");
  for (const context of [...filters.contexts].sort()) parts.push(`label = context:${context}`);
  for (const type of [...filters.types].sort()) parts.push(`type = ${type}`);
  return parts;
}

/** Every context present on the open pool, sorted. */
export function contextsOf(pool: readonly IssueDto[]): string[] {
  const set = new Set<string>();
  for (const issue of pool) {
    const context = contextOf(issue.labels);
    if (context !== null) set.add(context);
  }
  return [...set].sort();
}

export const ISSUE_TYPES = ["bug", "task", "story", "spike", "chore"] as const;
