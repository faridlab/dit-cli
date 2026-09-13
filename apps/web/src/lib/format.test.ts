// A field event stores what the file stores. For the two fields that hold
// issue ids that is a key, not a sentence, and every surface that shows
// history has to translate it back — these pin the translation.
import { describe, expect, it } from "vitest";
import { dueInfo, resolveIdValue } from "./format";

const TITLES = new Map([
  ["01M2CANYT5CSMHVFQFP5T87XT3", "Merge safety"],
  ["01M2CANZXVF1EEQFF9NVCCSKC0", "Time-travel board from a seq cutoff"],
]);
const titleOf = (id: string) => TITLES.get(id);

describe("resolveIdValue", () => {
  it("names the epic behind an id", () => {
    expect(resolveIdValue("epic", "01M2CANYT5CSMHVFQFP5T87XT3", titleOf)).toBe("Merge safety");
  });

  it("unwraps the bracketed list a blocked_by event carries", () => {
    expect(resolveIdValue("blocked_by", "[01M2CANZXVF1EEQFF9NVCCSKC0]", titleOf)).toBe(
      "Time-travel board from a seq cutoff",
    );
  });

  it("names every issue in a list of blockers", () => {
    expect(
      resolveIdValue("blocked_by", "[01M2CANYT5CSMHVFQFP5T87XT3, 01M2CANZXVF1EEQFF9NVCCSKC0]", titleOf),
    ).toBe("Merge safety, Time-travel board from a seq cutoff");
  });

  it("keeps an id whose issue is gone — history outlives its subject", () => {
    expect(resolveIdValue("epic", "01M2CDELETED0000000000000A", titleOf)).toBe(
      "01M2CDELETED0000000000000A",
    );
  });

  it("leaves every other field alone", () => {
    expect(resolveIdValue("status", "in_progress", titleOf)).toBe("in_progress");
    expect(resolveIdValue("due", "2026-09-16", titleOf)).toBe("2026-09-16");
  });
});

describe("dueInfo", () => {
  const now = Date.parse("2026-09-13T10:00:00Z");

  it("escalates a date that has passed", () => {
    expect(dueInfo("2026-09-11", now)).toMatchObject({ cls: "over", text: "2d overdue" });
  });

  it("calls today today, and the next three days soon", () => {
    expect(dueInfo("2026-09-13", now)).toMatchObject({ cls: "soon", text: "due today" });
    expect(dueInfo("2026-09-16", now)).toMatchObject({ cls: "soon", text: "due in 3d" });
  });

  it("leaves a distant date quiet, spelled month-day", () => {
    expect(dueInfo("2026-10-16", now)).toMatchObject({ cls: "", text: "due 10-16" });
  });

  it("has nothing to say without a date", () => {
    expect(dueInfo(null, now)).toBeNull();
  });
});
