// The activity stream is where invariant 9 becomes visible: field events are
// ordered by `seq`, never by timestamp. A merge commit diffed per parent
// produces events whose timestamps disagree with the order they happened in,
// and sorting the merged stream by time would show a field changing to two
// values at once.
import { describe, expect, it } from "vitest";
import { mergeActivity, type ActivityEntry } from "./activity";
import type { CommentDto, FieldEventDto } from "./types";

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
