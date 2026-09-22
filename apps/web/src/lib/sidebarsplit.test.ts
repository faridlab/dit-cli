// The split decides how much of the rail each half gets, so the arithmetic
// has to hold at the edges: a window too short for both halves, a stored
// value from a taller window, and a browser that refuses to store anything.
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MIN_NAV, MIN_PANE, clampSplit, readSplit, writeSplit } from "./sidebarsplit";

describe("clampSplit", () => {
  it("leaves a workable number alone", () => {
    expect(clampSplit(300, 800)).toBe(300);
  });

  it("never lets the section be squeezed out of existence", () => {
    // Dragging to the floor still leaves the pane its minimum.
    expect(clampSplit(9999, 800)).toBe(800 - MIN_PANE);
  });

  it("never lets the nav be squeezed out of existence", () => {
    expect(clampSplit(0, 800)).toBe(MIN_NAV);
  });

  it("gives the nav what is left when the window cannot hold both", () => {
    // A very short window: there is no split that satisfies both minimums,
    // so the nav takes what there is rather than either side going negative.
    const tiny = MIN_NAV + 10;
    expect(clampSplit(9999, tiny)).toBe(Math.max(0, tiny - MIN_PANE));
    expect(clampSplit(9999, 10)).toBe(0);
  });

  it("survives a value stored from a taller window", () => {
    const stored = 600;
    expect(clampSplit(stored, 400)).toBeLessThanOrEqual(400 - MIN_PANE);
  });
});

describe("the remembered split", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("is absent until someone drags, so nothing looks pre-dragged", () => {
    expect(readSplit()).toBeNull();
  });

  it("round-trips a dragged value", () => {
    writeSplit(280);
    expect(readSplit()).toBe(280);
  });

  it("forgets on reset rather than storing a zero", () => {
    writeSplit(280);
    writeSplit(null);
    expect(readSplit()).toBeNull();
  });

  it("ignores a stored value that is not a usable height", () => {
    window.localStorage.setItem("dit.sidebarSplit", "not-a-number");
    expect(readSplit()).toBeNull();
    window.localStorage.setItem("dit.sidebarSplit", "-40");
    expect(readSplit()).toBeNull();
  });

  it("keeps working when the browser refuses to store anything", () => {
    const boom = () => {
      throw new Error("blocked");
    };
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(boom);
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(boom);
    expect(readSplit()).toBeNull();
    expect(() => writeSplit(200)).not.toThrow();
    getItem.mockRestore();
    setItem.mockRestore();
  });
});
