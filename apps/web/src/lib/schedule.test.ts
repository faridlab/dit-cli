// A plan view reads issues and draws bars. The rule that matters most is
// what it does with the issues that carry no dates — which is most of them:
// it infers an edge from the estimate and marks it, and it never writes that
// inference back. These pin the arithmetic and the marking.
import { describe, expect, it } from "vitest";
import {
  coveringSpan,
  criticalPath,
  DAY_MS,
  durationDays,
  epicSpan,
  ganttRange,
  isLate,
  monthSegments,
  roadmapRange,
  scheduleSequentially,
  shortDate,
  spanOf,
  toDay,
  weekendOffsets,
} from "./schedule";

const day = (iso: string) => Date.parse(`${iso}T00:00:00Z`);

const issue = (fields: Partial<Record<string, unknown>> = {}) =>
  ({
    id: "AAA0001",
    start: null,
    due: null,
    estimate: null,
    ...fields,
  }) as never;

describe("spanOf", () => {
  it("uses both dates when the file has both", () => {
    const span = spanOf(issue({ start: "2026-09-10", due: "2026-09-12" }))!;
    expect(span.start).toBe(day("2026-09-10"));
    // `due` is the last day of work, so the bar covers it.
    expect(span.end).toBe(day("2026-09-13"));
    expect(span.inferredStart).toBe(false);
    expect(span.inferredEnd).toBe(false);
  });

  it("infers the start backwards from the estimate", () => {
    const span = spanOf(issue({ due: "2026-09-10", estimate: 3 }))!;
    // Three points at two days each: six days before the due date.
    expect(span.start).toBe(day("2026-09-04"));
    expect(span.inferredStart).toBe(true);
    expect(span.inferredEnd).toBe(false);
  });

  it("infers the end forwards from the estimate", () => {
    const span = spanOf(issue({ start: "2026-09-10", estimate: 2 }))!;
    expect(span.end).toBe(day("2026-09-14"));
    expect(span.inferredStart).toBe(false);
    expect(span.inferredEnd).toBe(true);
  });

  it("gives an unestimated issue one visible day", () => {
    const span = spanOf(issue({ start: "2026-09-10" }))!;
    expect(span.end - span.start).toBe(DAY_MS);
    expect(durationDays(issue())).toBe(1);
  });

  it("never draws a bar with no width", () => {
    const span = spanOf(issue({ start: "2026-09-10", due: "2026-09-10" }))!;
    expect(span.end).toBeGreaterThan(span.start);
  });

  it("leaves an issue with no dates off the axis entirely", () => {
    expect(spanOf(issue({ estimate: 5 }))).toBeNull();
  });

  it("reads a full timestamp as the day it falls on", () => {
    const span = spanOf(issue({ start: "2026-09-10T13:45:00Z" }))!;
    expect(span.start).toBe(day("2026-09-10"));
  });
});

describe("coveringSpan", () => {
  it("covers every span it is given", () => {
    const span = coveringSpan([
      spanOf(issue({ start: "2026-09-10", due: "2026-09-12" })),
      spanOf(issue({ start: "2026-09-08", due: "2026-09-09" })),
      null,
    ])!;
    expect(span.start).toBe(day("2026-09-08"));
    expect(span.end).toBe(day("2026-09-13"));
  });

  it("is null when nothing is scheduled", () => {
    expect(coveringSpan([null, null])).toBeNull();
  });
});

describe("epicSpan", () => {
  const child = (start: string, due: string) => issue({ start, due });

  it("prefers the epic's own dates", () => {
    const result = epicSpan(issue({ start: "2026-09-01", due: "2026-09-30" }), [
      child("2026-09-10", "2026-09-12"),
    ])!;
    expect(result.derived).toBe(false);
    expect(result.span.start).toBe(day("2026-09-01"));
  });

  it("derives the span from its children when it has none", () => {
    const result = epicSpan(issue(), [
      child("2026-09-10", "2026-09-12"),
      child("2026-09-20", "2026-09-21"),
    ])!;
    expect(result.derived).toBe(true);
    expect(result.span.start).toBe(day("2026-09-10"));
    expect(result.span.end).toBe(day("2026-09-22"));
  });

  it("is null when neither the epic nor its children are scheduled", () => {
    expect(epicSpan(issue(), [issue(), issue()])).toBeNull();
  });
});

describe("isLate", () => {
  const span = spanOf(issue({ start: "2026-09-01", due: "2026-09-02" }))!;

  it("is late once the bar has passed and the work is not done", () => {
    expect(isLate(span, false, day("2026-09-10"))).toBe(true);
  });

  it("is never late when the work is finished", () => {
    expect(isLate(span, true, day("2026-09-10"))).toBe(false);
  });

  it("is not late while the bar still covers today", () => {
    expect(isLate(span, false, day("2026-09-02"))).toBe(false);
  });
});

describe("criticalPath", () => {
  it("picks the longest chain of blockers, not the longest single bar", () => {
    const chain = [
      issue({ id: "A", start: "2026-09-01", due: "2026-09-02", blocked_by: [] }),
      issue({ id: "B", start: "2026-09-03", due: "2026-09-04", blocked_by: ["A"] }),
      issue({ id: "C", start: "2026-09-05", due: "2026-09-06", blocked_by: ["B"] }),
      // Longer on its own, but nothing depends on it.
      issue({ id: "D", start: "2026-09-01", due: "2026-09-05", blocked_by: [] }),
    ];
    expect([...criticalPath(chain)].sort()).toEqual(["A", "B", "C"]);
  });

  it("survives a dependency cycle instead of recursing forever", () => {
    const cyclic = [
      issue({ id: "A", start: "2026-09-01", due: "2026-09-02", blocked_by: ["B"] }),
      issue({ id: "B", start: "2026-09-03", due: "2026-09-04", blocked_by: ["A"] }),
    ];
    expect(criticalPath(cyclic).size).toBeGreaterThan(0);
  });

  it("ignores a blocker that is not on screen", () => {
    const dangling = [
      issue({ id: "A", start: "2026-09-01", due: "2026-09-02", blocked_by: ["GONE"] }),
    ];
    expect([...criticalPath(dangling)]).toEqual(["A"]);
  });

  it("has no path through an empty plan", () => {
    expect(criticalPath([]).size).toBe(0);
  });
});

describe("scheduleSequentially", () => {
  it("lays issues back to back from a starting day", () => {
    const plan = scheduleSequentially(
      [
        issue({ id: "A", estimate: 1 }),
        issue({ id: "B", estimate: 2 }),
        issue({ id: "C" }),
      ],
      day("2026-09-10"),
    );
    expect(plan).toEqual([
      // One point: two days, the 10th and the 11th.
      { id: "A", start: "2026-09-10", due: "2026-09-11" },
      { id: "B", start: "2026-09-12", due: "2026-09-15" },
      // No estimate: one day.
      { id: "C", start: "2026-09-16", due: "2026-09-16" },
    ]);
  });

  it("produces dates in the shape the files store", () => {
    expect(toDay(day("2026-09-10"))).toBe("2026-09-10");
  });
});

// 2026-09-13 is a Sunday: the awkward case for "the Monday three weeks back".
const TODAY = day("2026-09-13");

describe("ganttRange", () => {
  it("at day zoom starts a week ago and shows four weeks at 40px a day", () => {
    expect(ganttRange("day", TODAY)).toEqual({ start: day("2026-09-06"), days: 28, ppd: 40 });
  });

  it("at week zoom starts on the Monday three weeks back", () => {
    expect(ganttRange("week", TODAY)).toEqual({ start: day("2026-08-17"), days: 84, ppd: 14 });
  });

  it("at month zoom starts on the first of the month two months back", () => {
    expect(ganttRange("month", TODAY)).toEqual({ start: day("2026-07-01"), days: 183, ppd: 6 });
  });
});

describe("roadmapRange", () => {
  it("quarter is the current calendar quarter", () => {
    expect(roadmapRange("quarter", TODAY)).toEqual({ start: day("2026-07-01"), end: day("2026-10-01") });
  });

  it("half looks two months back and six months long", () => {
    expect(roadmapRange("half", TODAY)).toEqual({ start: day("2026-07-01"), end: day("2027-01-01") });
  });

  it("year looks three months back and twelve months long", () => {
    expect(roadmapRange("year", TODAY)).toEqual({ start: day("2026-06-01"), end: day("2027-06-01") });
  });
});

describe("monthSegments", () => {
  it("clips the first and last month to the range", () => {
    const segments = monthSegments(day("2026-08-17"), day("2026-11-09"));
    expect(segments.map((segment) => segment.label)).toEqual(["Aug 2026", "Sep 2026", "Oct 2026", "Nov 2026"]);
    expect(segments[0]).toMatchObject({ start: day("2026-08-17"), end: day("2026-09-01"), month: 7, year: 2026 });
    expect(segments[3]).toMatchObject({ start: day("2026-11-01"), end: day("2026-11-09") });
  });

  it("is empty for an empty range", () => {
    expect(monthSegments(TODAY, TODAY)).toEqual([]);
  });
});

describe("weekendOffsets", () => {
  it("names the Saturdays and Sundays as day offsets from the start", () => {
    // Monday the 17th: the weekend is days 5 and 6, then 12 and 13.
    expect(weekendOffsets(day("2026-08-17"), 14)).toEqual([5, 6, 12, 13]);
  });
});

describe("shortDate", () => {
  it("reads as the design labels dates", () => {
    expect(shortDate(day("2026-09-07"))).toBe("Sep 7");
  });
});
