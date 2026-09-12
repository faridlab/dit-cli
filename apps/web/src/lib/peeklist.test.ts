// J/K walking the list behind the panel is only as good as the order the
// views publish. These pin the stepping rules, including the case that
// actually bites: the list changing while a panel is open.
import { beforeEach, describe, expect, it } from "vitest";
import { resetPeekList, setPeekList, stepPeek } from "./peeklist";

describe("stepPeek", () => {
  beforeEach(() => {
    resetPeekList();
  });

  it("walks forward and back through the published order", () => {
    setPeekList(["a", "b", "c"]);
    expect(stepPeek("a", 1)).toBe("b");
    expect(stepPeek("b", 1)).toBe("c");
    expect(stepPeek("c", -1)).toBe("b");
  });

  it("stops at the ends instead of wrapping", () => {
    setPeekList(["a", "b"]);
    expect(stepPeek("a", -1)).toBeNull();
    expect(stepPeek("b", 1)).toBeNull();
  });

  it("starts at the top when nothing is open", () => {
    setPeekList(["a", "b"]);
    expect(stepPeek(null, 1)).toBe("a");
  });

  it("starts at the top when the open issue left the list", () => {
    setPeekList(["a", "b"]);
    expect(stepPeek("gone", 1)).toBe("a");
    expect(stepPeek("gone", -1)).toBe("a");
  });

  it("has nowhere to go in an empty list", () => {
    expect(stepPeek(null, 1)).toBeNull();
    expect(stepPeek("a", 1)).toBeNull();
  });
});
