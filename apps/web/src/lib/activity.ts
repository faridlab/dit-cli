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
