// One stream out of two sources: the field changes DIT derives from git, and
// the comment files people write. Both are read from the repo; neither is
// stored as an "activity log" (invariant 5).
//
// Two rules shape the merge, and both come from how git actually behaves:
//
//  1. Field events are never re-ordered. The server returns them by `seq`,
//     the order things actually happened; sorting them by timestamp instead
//     produces contradictions, because a merge commit diffed per parent
//     yields events with identical timestamps (invariant 9). Comments are
//     slotted *between* them by time — the events' own order is untouched.
//
//  2. A commit is one entry. Every field event carries the commit that
//     produced it, so changing status and priority together reads as one
//     act by one person, and the burst of events that creates an issue
//     reads as "created", not as eight separate lines.

import type { CommentDto, FieldEventDto } from "./types";

export type ActivityEntry =
  | {
      kind: "comment";
      /** Comment id — stable across refetches, so it keys the list. */
      id: string;
      ts: string;
      author: string;
      bodyHtml: string;
    }
  | {
      kind: "commit";
      /** The commit's first event `seq` — stable, and the sort order. */
      seq: number;
      sha: string;
      ts: string;
      author: string;
      events: FieldEventDto[];
      /** Every field went from nothing: this commit is the issue's birth. */
      creation: boolean;
    };

/** Consecutive events from the same commit, in the order the server gave
 *  them. Consecutive, not grouped globally: a sha that reappears later is a
 *  later act and deserves its own entry. */
function groupByCommit(events: readonly FieldEventDto[]): Extract<ActivityEntry, { kind: "commit" }>[] {
  const groups: Extract<ActivityEntry, { kind: "commit" }>[] = [];
  for (const event of events) {
    const open = groups[groups.length - 1];
    if (open && open.sha === event.commit_sha && open.author === event.author) {
      open.events.push(event);
      if (event.old_value !== null) open.creation = false;
      continue;
    }
    groups.push({
      kind: "commit",
      seq: event.seq,
      sha: event.commit_sha,
      ts: event.ts,
      author: event.author,
      events: [event],
      creation: event.old_value === null,
    });
  }
  return groups;
}

/** Field changes and comments as one stream, oldest first. */
export function mergeActivity(
  events: readonly FieldEventDto[],
  comments: readonly CommentDto[],
): ActivityEntry[] {
  const groups = groupByCommit(events);
  // Comments are ordinary files with a timestamp; sorting them by time is
  // safe in a way that sorting field events is not.
  const sorted = [...comments].sort((a, b) => Date.parse(a.created) - Date.parse(b.created));

  const merged: ActivityEntry[] = [];
  let c = 0;
  for (const group of groups) {
    const at = Date.parse(group.ts);
    while (c < sorted.length) {
      const comment = sorted[c];
      if (comment === undefined || Date.parse(comment.created) > at) break;
      merged.push({
        kind: "comment",
        id: comment.id,
        ts: comment.created,
        author: comment.author,
        bodyHtml: comment.body_html,
      });
      c += 1;
    }
    merged.push(group);
  }
  for (; c < sorted.length; c += 1) {
    const comment = sorted[c];
    if (comment === undefined) continue;
    merged.push({
      kind: "comment",
      id: comment.id,
      ts: comment.created,
      author: comment.author,
      bodyHtml: comment.body_html,
    });
  }
  return merged;
}

// ---------------------------------------------------------------------------
// The workspace Timeline: the same two rules, applied to every issue at once.
// ---------------------------------------------------------------------------

import type { ActivityEventDto, DayCountDto, WorkspaceCommentDto } from "./types";

/** The kinds of field change the Timeline draws with their own icon. Every
 *  other field is "other" — shown, but without a special glyph. */
export type FieldKind =
  | "status"
  | "priority"
  | "assignees"
  | "labels"
  | "epic"
  | "estimate"
  | "due"
  | "start"
  | "title"
  | "other";

export type TimelineKind = FieldKind | "comment" | "created";

/** The sidebar filters by a coarser set: the rare field kinds share one row. */
export type TimelineBucket =
  | "status"
  | "priority"
  | "assignees"
  | "labels"
  | "comment"
  | "created"
  | "other";

export interface TimelineIssue {
  id: string;
  short_ref: string;
  number: number | null;
  title: string;
}

interface TimelineBase {
  /** Stable across refetches; keys the rendered row. */
  key: string;
  /** Position in the commit graph. A comment borrows the seq of the field
   *  event it sits below, so "everything at or before seq N" can be answered
   *  for comments too — they have no seq of their own. */
  seq: number;
  ts: string;
  author: string;
  issue: TimelineIssue;
}

export type TimelineEvent =
  | (TimelineBase & { kind: "created"; type: string | null })
  | (TimelineBase & { kind: "comment"; text: string })
  | (TimelineBase & { kind: FieldKind; field: string; old: string | null; new: string | null });

const FIELD_KINDS: ReadonlySet<string> = new Set([
  "status",
  "priority",
  "assignees",
  "labels",
  "epic",
  "estimate",
  "due",
  "start",
  "title",
]);

export function kindOf(field: string): FieldKind {
  return FIELD_KINDS.has(field) ? (field as FieldKind) : "other";
}

export function filterBucket(kind: TimelineKind): TimelineBucket {
  switch (kind) {
    case "status":
    case "priority":
    case "assignees":
    case "labels":
    case "comment":
    case "created":
      return kind;
    default:
      return "other";
  }
}

/** Fields the indexer records but nobody sets by hand. They would double
 *  every real change with a line about its bookkeeping. */
const HIDDEN_FIELDS: ReadonlySet<string> = new Set(["updated", "number", "reporter", "type", "body"]);

/** Fields that exist on every issue from the moment it is written. A burst
 *  of first-time values is a creation only if it includes one of them;
 *  otherwise it is somebody filling in a blank, and reads as a normal set. */
const BIRTH_FIELDS: ReadonlySet<string> = new Set(["title", "status"]);

/** Comment HTML as one line of text, for the feed's quoted excerpt. */
export function plainText(html: string): string {
  return html
    .replace(/<[^>]+>/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function issueOf(event: ActivityEventDto): TimelineIssue {
  return { id: event.issue_id, short_ref: event.short_ref, number: event.number, title: event.title };
}

/** Field events (newest first, as the feed endpoint returns them) turned
 *  into Timeline rows, with an issue's birth collapsed into one "created". */
function fieldRows(events: readonly ActivityEventDto[]): TimelineEvent[] {
  const rows: TimelineEvent[] = [];
  let i = 0;
  while (i < events.length) {
    const head = events[i];
    if (head === undefined) break;
    // A run of first-time values for one issue in one commit.
    let j = i;
    while (j < events.length) {
      const e = events[j];
      if (
        e === undefined ||
        e.old_value !== null ||
        e.issue_id !== head.issue_id ||
        e.commit_sha !== head.commit_sha
      ) {
        break;
      }
      j += 1;
    }
    const burst = events.slice(i, j);
    if (burst.some((e) => BIRTH_FIELDS.has(e.field))) {
      const type = burst.find((e) => e.field === "type")?.new_value ?? null;
      rows.push({
        kind: "created",
        key: `created:${head.issue_id}:${head.commit_sha}:${head.seq}`,
        seq: head.seq,
        ts: head.ts,
        author: head.author,
        issue: issueOf(head),
        type,
      });
      i = j;
      continue;
    }
    if (!HIDDEN_FIELDS.has(head.field)) {
      rows.push({
        kind: kindOf(head.field),
        key: `f:${head.seq}`,
        seq: head.seq,
        ts: head.ts,
        author: head.author,
        issue: issueOf(head),
        field: head.field,
        old: head.old_value,
        new: head.new_value,
      });
    }
    i += 1;
  }
  return rows;
}

/** The whole workspace as one stream, newest first. Field events stay in
 *  the order given (their `seq` order — invariant 9); comments are slotted
 *  between them by time, which is safe because a comment is a file with one
 *  timestamp and no parent to disagree with. */
export function workspaceTimeline(
  events: readonly ActivityEventDto[],
  comments: readonly WorkspaceCommentDto[],
): TimelineEvent[] {
  const rows = fieldRows(events);
  const sorted = [...comments].sort((a, b) => Date.parse(b.created) - Date.parse(a.created));

  const commentRow = (comment: WorkspaceCommentDto, seq: number): TimelineEvent => ({
    kind: "comment",
    key: comment.id,
    seq,
    ts: comment.created,
    author: comment.author,
    issue: {
      id: comment.issue_id,
      short_ref: comment.short_ref,
      number: comment.number,
      title: comment.title,
    },
    text: plainText(comment.body_html) || comment.body,
  });

  const merged: TimelineEvent[] = [];
  let c = 0;
  for (const row of rows) {
    const at = Date.parse(row.ts);
    while (c < sorted.length) {
      const comment = sorted[c];
      if (comment === undefined || Date.parse(comment.created) <= at) break;
      merged.push(commentRow(comment, row.seq));
      c += 1;
    }
    merged.push(row);
  }
  // Older than every loaded field event: nothing to sit below, so seq 0 —
  // they survive any cutoff, which is right, since they predate it.
  for (; c < sorted.length; c += 1) {
    const comment = sorted[c];
    if (comment !== undefined) merged.push(commentRow(comment, 0));
  }
  return merged;
}

/** The newest field event on or before a `YYYY-MM-DD` day, as a `seq` —
 *  how a day on the density strip becomes a point in history. Null when the
 *  day is older than everything loaded: the answer may exist, but not here. */
export function seqForDay(day: string, events: readonly ActivityEventDto[]): number | null {
  let best: number | null = null;
  for (const event of events) {
    if (event.ts.slice(0, 10) <= day && (best === null || event.seq > best)) best = event.seq;
  }
  return best;
}

/** Exactly `n` consecutive days ending on `today`, zero where the server
 *  reported nothing — the density strip needs a bar per day, not per event. */
export function fillDays(days: readonly DayCountDto[], today: string, n: number): DayCountDto[] {
  const counts = new Map(days.map((d) => [d.day, d.count]));
  const end = Date.parse(`${today}T00:00:00Z`);
  const out: DayCountDto[] = [];
  for (let k = n - 1; k >= 0; k -= 1) {
    const day = new Date(end - k * 86_400_000).toISOString().slice(0, 10);
    out.push({ day, count: counts.get(day) ?? 0 });
  }
  return out;
}
