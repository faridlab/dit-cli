// The theme preference is the one piece of UI state that must survive a
// reload without touching git. These pin the contract the stylesheet relies
// on: `system` means no stamp on <html>, an explicit choice stamps it, and
// garbage in storage falls back to `system` instead of breaking the page.
import { describe, expect, it } from "vitest";
import { applyTheme, loadTheme, resolveTheme, saveTheme, THEME_KEY } from "./theme";

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(initial));
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key) => map.get(key) ?? null,
    key: (index) => Array.from(map.keys())[index] ?? null,
    removeItem: (key) => void map.delete(key),
    setItem: (key, value) => void map.set(key, value),
  };
}

describe("theme preference", () => {
  it("defaults to system when nothing is stored", () => {
    expect(loadTheme(memoryStorage())).toBe("system");
  });

  it("falls back to system on garbage", () => {
    expect(loadTheme(memoryStorage({ [THEME_KEY]: "neon" }))).toBe("system");
  });

  it("round-trips an explicit choice and clears it for system", () => {
    const storage = memoryStorage();
    saveTheme("dark", storage);
    expect(loadTheme(storage)).toBe("dark");
    saveTheme("system", storage);
    expect(storage.getItem(THEME_KEY)).toBeNull();
    expect(loadTheme(storage)).toBe("system");
  });

  it("survives a storage that throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
    };
    expect(loadTheme(broken)).toBe("system");
  });
});

describe("applyTheme", () => {
  it("stamps an explicit choice and unstamps system", () => {
    const root = document.createElement("div");
    applyTheme("dark", root);
    expect(root.getAttribute("data-theme")).toBe("dark");
    applyTheme("light", root);
    expect(root.getAttribute("data-theme")).toBe("light");
    applyTheme("system", root);
    expect(root.hasAttribute("data-theme")).toBe(false);
  });
});

describe("resolveTheme", () => {
  it("follows the OS only for system", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});
