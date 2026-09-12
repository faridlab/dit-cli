// Theme preference: system (follow the OS), light, or dark. The choice is a
// per-browser convenience, never workspace data, so it lives in localStorage
// and nowhere near git. The stylesheet does the actual theming: an explicit
// choice stamps `data-theme` on <html>; "system" removes the stamp and lets
// `prefers-color-scheme` decide (see styles.css).

import { useCallback, useEffect, useState } from "react";

export type ThemePreference = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

export const THEME_KEY = "dit.theme";
const PREFERENCES: readonly ThemePreference[] = ["system", "light", "dark"];

function isPreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (PREFERENCES as readonly string[]).includes(value);
}

/** The stored preference, or `system` when nothing (or garbage) is stored. */
export function loadTheme(storage: Pick<Storage, "getItem"> | null = safeStorage()): ThemePreference {
  try {
    const raw = storage?.getItem(THEME_KEY);
    return isPreference(raw) ? raw : "system";
  } catch {
    return "system";
  }
}

export function saveTheme(
  preference: ThemePreference,
  storage: Pick<Storage, "setItem" | "removeItem"> | null = safeStorage(),
): void {
  try {
    if (preference === "system") storage?.removeItem(THEME_KEY);
    else storage?.setItem(THEME_KEY, preference);
  } catch {
    // A blocked or full localStorage only loses the remembered choice.
  }
}

/** Stamp (or unstamp) the root element. Idempotent. */
export function applyTheme(preference: ThemePreference, root: Element = document.documentElement): void {
  if (preference === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", preference);
}

/** What the viewer actually sees, given the preference and the OS setting. */
export function resolveTheme(preference: ThemePreference, prefersDark: boolean): ResolvedTheme {
  if (preference === "system") return prefersDark ? "dark" : "light";
  return preference;
}

function safeStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

function osPrefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(DARK_QUERY).matches
    : false;
}

/** React view of the preference plus the resolved theme, kept in step with
 *  the OS while the preference is `system`. Setting persists and applies. */
export function useTheme(): {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference: (preference: ThemePreference) => void;
} {
  const [preference, setPreferenceState] = useState<ThemePreference>(loadTheme);
  const [prefersDark, setPrefersDark] = useState(osPrefersDark);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => setPrefersDark(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const setPreference = useCallback((next: ThemePreference) => {
    setPreferenceState(next);
    saveTheme(next);
    applyTheme(next);
  }, []);

  return { preference, resolved: resolveTheme(preference, prefersDark), setPreference };
}
