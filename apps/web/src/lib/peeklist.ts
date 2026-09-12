// The order the list on screen is in, published so the issue panel can walk
// it with J/K without knowing anything about boards, tables or search
// results. Each view registers the ids it is showing, in the order it shows
// them; the shell reads that list to answer "what is the next issue?".
//
// A module-level store rather than a context: the shell needs to read it and
// the views need to write it, and they sit on opposite sides of the tree.
// It is view state — nothing here is fetched, stored or committed.

import { useEffect, useSyncExternalStore } from "react";

let current: readonly string[] = [];
const listeners = new Set<() => void>();

function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, index) => id === b[index]);
}

/** Replace the on-screen order. A no-op when nothing actually changed, so
 *  the constant re-renders of a live list cost nothing. */
export function setPeekList(next: readonly string[]): void {
  if (sameOrder(current, next)) return;
  current = next;
  for (const listener of listeners) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The ids currently on screen, in display order. */
export function usePeekList(): readonly string[] {
  return useSyncExternalStore(
    subscribe,
    () => current,
    () => current,
  );
}

/** Publish the ids this view is showing, in display order. */
export function useRegisterPeekList(ids: readonly string[]): void {
  // The join is the dependency: a fresh array with the same contents on
  // every render must not re-register.
  const key = ids.join(" ");
  useEffect(() => {
    setPeekList(key.length === 0 ? [] : key.split(" "));
  }, [key]);
}

/** The neighbor of `id` in the current list, `delta` steps away. Returns
 *  null when there is nowhere to go. An id that has fallen out of the list
 *  (a filter changed under the panel) steps from the top. */
export function stepPeek(id: string | null, delta: -1 | 1): string | null {
  const ids = current;
  if (ids.length === 0) return null;
  const index = id === null ? -1 : ids.indexOf(id);
  if (index === -1) return ids[0] ?? null;
  const next = index + delta;
  if (next < 0 || next >= ids.length) return null;
  return ids[next] ?? null;
}

/** Test seam: drop the registered order. */
export function resetPeekList(): void {
  current = [];
  for (const listener of listeners) listener();
}
