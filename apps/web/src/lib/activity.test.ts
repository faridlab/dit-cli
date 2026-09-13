// The activity stream is where invariant 9 becomes visible: field events are
// ordered by `seq`, never by timestamp. A merge commit diffed per parent
// produces events whose timestamps disagree with the order they happened in,
// and sorting the merged stream by time would show a field changing to two
// values at once.
import { describe, expect, it } from "vitest";
import {
  fillDays,
  filterBucket,
  kindOf,
  mergeActivity,
  plainText,
  seqForDay,
  workspaceTimeline,
  type ActivityEntry,
  type TimelineEvent,
} from "./activity";
import type { ActivityEventDto, CommentDto, FieldEventDto, WorkspaceCommentDto } from "./types";

function event(partial: Partial<FieldEventDto> & { seq: number }): FieldEventDto {
  return {
    field: "status",
    old_value: "todo",
    new_value: "in_progress",
    author: "farid",
    ts: "2026-09-01T10:00:00Z",
    commit_sha: `sha${partial.seq}`,
    ...partial,
  };
}

function comment(id: string, created: string): CommentDto {
  return {
    id,
    issue_id: "Q2R7VN8",
    author: "budi",
    created,
    body: id,
    body_html: `<p>${id}</p>`,
  };
}

const kinds = (entries: ActivityEntry[]) =>
  entries.map((entry) => (entry.kind === "comment" ? entry.id : entry.sha));

describe("mergeActivity", () => {
  it("keeps field events in seq order even when their timestamps disagree", () => {
    // What a merge commit diffed per parent looks like: same instant, and an
    // earlier timestamp arriving after a later one.
    const events = [
      event({ seq: 1, commit_sha: "aaa", ts: "2026-09-01T12:00:00Z" }),
      event({ seq: 2, commit_sha: "bbb", ts: "2026-09-01T09:00:00Z" }),
      event({ seq: 3, commit_sha: "ccc", ts: "2026-09-01T09:00:00Z" }),
    ];
    expect(kinds(mergeActivity(events, []))).toEqual(["aaa", "bbb", "ccc"]);
  });

  it("slots comments between commits by time", () => {
    const events = [
      event({ seq: 1, commit_sha: "aaa", ts: "2026-09-01T10:00:00Z" }),
      event({ seq: 2, commit_sha: "bbb", ts: "2026-09-01T14:00:00Z" }),
    ];
    const comments = [comment("later", "2026-09-01T12:00:00Z"), comment("early", "2026-09-01T09:00:00Z")];
    expect(kinds(mergeActivity(events, comments))).toEqual(["early", "aaa", "later", "bbb"]);
  });

  it("puts comments after the last commit at the end", () => {
    const events = [event({ seq: 1, commit_sha: "aaa", ts: "2026-09-01T10:00:00Z" })];
    expect(kinds(mergeActivity(events, [comment("last", "2026-09-02T10:00:00Z")]))).toEqual([
      "aaa",
      "last",
    ]);
  });

  it("reads one commit as one entry", () => {
    const events = [
      event({ seq: 1, field: "status", commit_sha: "aaa" }),
      event({ seq: 2, field: "priority", old_value: "p2", new_value: "p1", commit_sha: "aaa" }),
    ];
    const [entry, ...rest] = mergeActivity(events, []);
    expect(rest).toHaveLength(0);
    expect(entry?.kind === "commit" && entry.events.map((e) => e.field)).toEqual([
      "status",
      "priority",
    ]);
  });

  it("separates the same sha when it is not consecutive", () => {
    const events = [
      event({ seq: 1, commit_sha: "aaa" }),
      event({ seq: 2, commit_sha: "bbb" }),
      event({ seq: 3, commit_sha: "aaa" }),
    ];
    expect(kinds(mergeActivity(events, []))).toEqual(["aaa", "bbb", "aaa"]);
  });

  it("never merges two people into one entry", () => {
    const events = [
      event({ seq: 1, commit_sha: "aaa", author: "farid" }),
      event({ seq: 2, commit_sha: "aaa", author: "budi" }),
    ];
    const entries = mergeActivity(events, []);
    expect(entries).toHaveLength(2);
  });

  it("marks the commit that created the issue", () => {
    const birth = [
      event({ seq: 1, field: "title", old_value: null, new_value: "Login timeout", commit_sha: "aaa" }),
      event({ seq: 2, field: "status", old_value: null, new_value: "todo", commit_sha: "aaa" }),
    ];
    const [entry] = mergeActivity(birth, []);
    expect(entry?.kind === "commit" && entry.creation).toBe(true);
  });

  it("does not call a mixed commit a creation", () => {
    const events = [
      event({ seq: 1, field: "sprint", old_value: null, new_value: "2026-W33", commit_sha: "aaa" }),
      event({ seq: 2, field: "status", old_value: "todo", new_value: "done", commit_sha: "aaa" }),
    ];
    const [entry] = mergeActivity(events, []);
    expect(entry?.kind === "commit" && entry.creation).toBe(false);
  });

  it("handles an issue with no history and no comments", () => {
    expect(mergeActivity([], [])).toEqual([]);
  });

  it("shows comments alone on an issue that never changed", () => {
    expect(kinds(mergeActivity([], [comment("only", "2026-09-01T10:00:00Z")]))).toEqual(["only"]);
  });
});

// The workspace Timeline reads the whole repo's history at once. The same
// two rules apply — field events keep their `seq` order, comments slot in
// by time — plus the collapsing of an issue's birth into one "created" line.

function wsEvent(partial: Partial<ActivityEventDto> & { seq: number }): ActivityEventDto {
  return {
    issue_id: "01J",
    short_ref: "Q2R7VN8",
    number: 12,
    title: "Merge driver",
    field: "status",
    old_value: "todo",
    new_value: "in_progress",
    author: "farid",
    ts: "2026-09-01T10:00:00Z",
    commit_sha: `sha${partial.seq}`,
    ...partial,
  };
}

function wsComment(id: string, created: string, body = id): WorkspaceCommentDto {
  return {
    id,
    issue_id: "01J",
    short_ref: "Q2R7VN8",
    number: 12,
    title: "Merge driver",
    author: "budi",
    created,
    body,
    body_html: `<p>${body}</p>`,
  };
}

const keys = (events: TimelineEvent[]) =>
  events.map((event) => (event.kind === "comment" ? event.key : `${event.kind}:${event.seq}`));

describe("kindOf", () => {
  it("maps every tracked field to the prototype's kind", () => {
    expect(kindOf("status")).toBe("status");
    expect(kindOf("priority")).toBe("priority");
    expect(kindOf("assignees")).toBe("assignees");
    expect(kindOf("labels")).toBe("labels");
    expect(kindOf("epic")).toBe("epic");
    expect(kindOf("estimate")).toBe("estimate");
    expect(kindOf("due")).toBe("due");
    expect(kindOf("start")).toBe("start");
    expect(kindOf("title")).toBe("title");
    expect(kindOf("sprint")).toBe("other");
  });
});

describe("filterBucket", () => {
  it("folds the small field kinds into the sidebar's Other row", () => {
    expect(filterBucket("status")).toBe("status");
    expect(filterBucket("comment")).toBe("comment");
    expect(filterBucket("created")).toBe("created");
    for (const kind of ["epic", "estimate", "due", "start", "title", "other"] as const) {
      expect(filterBucket(kind)).toBe("other");
    }
  });
});

describe("workspaceTimeline", () => {
  it("keeps field events in the order given even when timestamps disagree", () => {
    const events = [
      wsEvent({ seq: 3, ts: "2026-09-01T09:00:00Z" }),
      wsEvent({ seq: 2, ts: "2026-09-01T12:00:00Z" }),
      wsEvent({ seq: 1, ts: "2026-09-01T09:00:00Z" }),
    ];
    expect(keys(workspaceTimeline(events, []))).toEqual(["status:3", "status:2", "status:1"]);
  });

  it("collapses an issue's creation burst into one created event carrying its type", () => {
    const birth = [
      wsEvent({ seq: 5, field: "type", old_value: null, new_value: "bug", commit_sha: "aaa" }),
      wsEvent({ seq: 4, field: "priority", old_value: null, new_value: "p1", commit_sha: "aaa" }),
      wsEvent({ seq: 3, field: "title", old_value: null, new_value: "Merge driver", commit_sha: "aaa" }),
      wsEvent({ seq: 2, field: "status", old_value: null, new_value: "todo", commit_sha: "aaa" }),
    ];
    const [created, ...rest] = workspaceTimeline(birth, []);
    expect(rest).toHaveLength(0);
    expect(created?.kind).toBe("created");
    expect(created?.kind === "created" && created.type).toBe("bug");
    expect(created?.seq).toBe(5);
  });

  it("does not merge two issues born in the same commit", () => {
    const events = [
      wsEvent({ seq: 4, issue_id: "B", short_ref: "BBBBBBB", field: "title", old_value: null, commit_sha: "aaa" }),
      wsEvent({ seq: 3, issue_id: "B", short_ref: "BBBBBBB", field: "status", old_value: null, commit_sha: "aaa" }),
      wsEvent({ seq: 2, issue_id: "A", field: "title", old_value: null, commit_sha: "aaa" }),
      wsEvent({ seq: 1, issue_id: "A", field: "status", old_value: null, commit_sha: "aaa" }),
    ];
    const out = workspaceTimeline(events, []);
    expect(out.map((e) => e.issue.short_ref)).toEqual(["BBBBBBB", "Q2R7VN8"]);
    expect(out.every((e) => e.kind === "created")).toBe(true);
  });

  it("shows a lone first-time value as a normal set, not a creation", () => {
    const events = [wsEvent({ seq: 1, field: "sprint", old_value: null, new_value: "2026-W36" })];
    const [only] = workspaceTimeline(events, []);
    expect(only?.kind).toBe("other");
    expect(only?.kind === "other" && only.old).toBeNull();
  });

  it("hides bookkeeping fields nobody set by hand", () => {
    const events = [
      wsEvent({ seq: 6, field: "updated", old_value: "a", new_value: "b" }),
      wsEvent({ seq: 5, field: "number", old_value: null, new_value: "12" }),
      wsEvent({ seq: 4, field: "reporter", old_value: "a", new_value: "b" }),
      wsEvent({ seq: 3, field: "type", old_value: "task", new_value: "bug" }),
      wsEvent({ seq: 2, field: "body", old_value: "a", new_value: "b" }),
      wsEvent({ seq: 1, field: "labels", old_value: "[]", new_value: "[ux]" }),
    ];
    expect(keys(workspaceTimeline(events, []))).toEqual(["labels:1"]);
  });

  it("slots comments between field events by time, newest first", () => {
    const events = [
      wsEvent({ seq: 2, ts: "2026-09-01T14:00:00Z" }),
      wsEvent({ seq: 1, ts: "2026-09-01T10:00:00Z" }),
    ];
    const comments = [
      wsComment("early", "2026-09-01T09:00:00Z"),
      wsComment("mid", "2026-09-01T12:00:00Z"),
      wsComment("late", "2026-09-01T16:00:00Z"),
    ];
    expect(keys(workspaceTimeline(events, comments))).toEqual(["late", "status:2", "mid", "status:1", "early"]);
  });

  it("carries the comment's text, author and issue", () => {
    const [only] = workspaceTimeline([], [wsComment("c1", "2026-09-01T09:00:00Z", "Looks good")]);
    expect(only?.kind === "comment" && only.text).toBe("Looks good");
    expect(only?.author).toBe("budi");
    expect(only?.issue.number).toBe(12);
  });

  it("stamps a comment with the seq it sits below, so a cutoff can hide it", () => {
    const events = [
      wsEvent({ seq: 2, ts: "2026-09-01T14:00:00Z" }),
      wsEvent({ seq: 1, ts: "2026-09-01T10:00:00Z" }),
    ];
    const out = workspaceTimeline(events, [wsComment("mid", "2026-09-01T12:00:00Z"), wsComment("late", "2026-09-01T16:00:00Z")]);
    const seqOf = (key: string) => out.find((e) => e.key === key)?.seq;
    expect(seqOf("mid")).toBe(1);
    expect(seqOf("late")).toBe(2);
  });
});

describe("plainText", () => {
  it("strips tags and squeezes whitespace", () => {
    expect(plainText("<p>Hello <b>world</b></p>\n<p>again</p>")).toBe("Hello world again");
  });
});

describe("seqForDay", () => {
  const events = [
    wsEvent({ seq: 3, ts: "2026-09-10T08:00:00Z" }),
    wsEvent({ seq: 2, ts: "2026-09-08T08:00:00Z" }),
    wsEvent({ seq: 1, ts: "2026-09-05T08:00:00Z" }),
  ];
  it("finds the newest event on or before the day", () => {
    expect(seqForDay("2026-09-10", events)).toBe(3);
    expect(seqForDay("2026-09-09", events)).toBe(2);
    expect(seqForDay("2026-09-05", events)).toBe(1);
  });
  it("returns null for a day older than everything loaded", () => {
    expect(seqForDay("2026-09-04", events)).toBeNull();
  });
});

describe("fillDays", () => {
  it("returns exactly n days ending today, zero where the server said nothing", () => {
    const out = fillDays([{ day: "2026-09-11", count: 3 }], "2026-09-12", 4);
    expect(out).toEqual([
      { day: "2026-09-09", count: 0 },
      { day: "2026-09-10", count: 0 },
      { day: "2026-09-11", count: 3 },
      { day: "2026-09-12", count: 0 },
    ]);
  });
});
