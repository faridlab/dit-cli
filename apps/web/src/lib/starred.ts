// Stars are a private bookmark, not workspace data: they live in this
// browser and never reach a file, a commit or another person. That is the
// whole reason they can exist at all — "my shortlist" is not a fact about
// the issue, and writing it into the repo would make one person's triage
// everybody's diff.
//
// A module store (like lib/peeklist) because the sidebar count, the list
// filter and the star button all read it from different corners of the tree.

import { useEffect, useSyncExternalStore } from "react";

export const STARRED_KEY = "dit.starred";

let current: ReadonlySet<string> = new Set();
let loaded = false;
const listeners = new Set<() => void>();

type Storage = Pick<globalThis.Storage, "getItem" | "setItem">;

function browserStorage(): Storage | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    return null;
  }
}

/** Read the stored shortlist. Anything unexpected in storage is treated as
 *  "no stars" rather than breaking the sidebar. */
export function loadStarred(storage: Storage | null = browserStorage()): ReadonlySet<string> {
  try {
    const raw = storage?.getItem(STARRED_KEY);
    const parsed: unknown = raw === null || raw === undefined ? null : JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((value) => typeof value === "string")) {
      return new Set(parsed as string[]);
    }
  } catch {
    // Fall through: a blocked, full or corrupt storage just means no stars.
  }
  return new Set();
}

function persist(storage: Storage | null = browserStorage()): void {
  try {
    storage?.setItem(STARRED_KEY, JSON.stringify([...current]));
  } catch {
    // The stars stay in this session; losing them is not worth an error.
  }
}

function emit(): void {
  for (const listener of listeners) listener();
}

/** Load once, on first read — so importing this module touches no storage. */
function ensureLoaded(): void {
  if (loaded) return;
  loaded = true;
  current = loadStarred();
}

export function isStarred(id: string): boolean {
  ensureLoaded();
  return current.has(id);
}

/** Star or unstar an issue by its permanent short ref. Returns the new state. */
export function toggleStar(id: string): boolean {
  ensureLoaded();
  const next = new Set(current);
  const starred = !next.has(id);
  if (starred) next.add(id);
  else next.delete(id);
  current = next;
  persist();
  emit();
  return starred;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function snapshot(): ReadonlySet<string> {
  ensureLoaded();
  return current;
}

/** The whole shortlist, re-rendering whatever reads it when it changes. */
export function useStarred(): ReadonlySet<string> {
  return useSyncExternalStore(subscribe, snapshot, snapshot);
}

/** Whether one issue is starred, as a hook. */
export function useIsStarred(id: string): boolean {
  return useStarred().has(id);
}

/** Stars survive a reload, so a tab that starred something must not be
 *  contradicted by a tab that starred something else. */
export function useStarredSync(): void {
  useEffect(() => {
    const onStorage = (event: StorageEvent) => {
      if (event.key !== null && event.key !== STARRED_KEY) return;
      current = loadStarred();
      emit();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, []);
}

/** Test seam: forget everything read or written so far. */
export function resetStarred(): void {
  current = new Set();
  loaded = false;
  emit();
}
