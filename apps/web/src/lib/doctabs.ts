// Editor tabs for the Docs view, VS Code style. A single click opens a page
// in a preview tab (italic) that the next single click replaces; a double
// click pins it as a permanent tab. Unsaved text lives per path, so
// switching tabs never throws work away — closing a dirty tab is the only
// place that asks. The tab list and pins survive reloads (localStorage);
// drafts do not: unsaved work is never silently resurrected, and the
// upcoming always-on editor autosaves it within seconds anyway.

import { useCallback, useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { queryKeys } from "./queries";
import type { DocBodyDto } from "./types";

const TABS_KEY = "dit.docs.tabs";
const PINNED_KEY = "dit.docs.pinned";

// Closing or renaming the active tab navigates away, and the URL changes a
// beat later than the tab list. In between, the shell still sees the old
// page in the route and asks `ensure` to give it a tab again — which would
// undo the close. A just-closed path is therefore ignored by `ensure` for
// the length of that navigation; a real reopen (a click, a deep link) comes
// well after the window.
const ENSURE_GUARD_MS = 1000;

function loadList(key: string): string[] {
  try {
    const raw = window.localStorage.getItem(key);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);
    return Array.isArray(parsed) && parsed.every((v) => typeof v === "string")
      ? (parsed as string[])
      : [];
  } catch {
    return [];
  }
}

function persist(key: string, value: string[]) {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A blocked or full localStorage only loses the remembered tabs.
  }
}

export interface DocTabs {
  /** Open tabs in display order. */
  paths: string[];
  /** Paths pinned by a double click — everything else is a preview slot. */
  pinned: Set<string>;
  /** The editing buffer per open tab. With the always-on editor every open
   *  page has one; it is "unsaved" exactly when it differs from the cached
   *  saved body (`isDirty`). */
  drafts: Record<string, string>;
  /** True when the path has a draft that differs from the saved body
   *  (read from the shared query cache, so it agrees with the editor). */
  isDirty: (path: string) => boolean;
  /** Single click: replace the current preview tab with this path. */
  preview: (path: string) => void;
  /** Double click: keep this path as its own tab. */
  pin: (path: string) => void;
  /** Demote a pinned tab back to the preview slot. The tab stays open; the
   *  next single click on another page replaces it. */
  unpin: (path: string) => void;
  /** The URL names an active page that has no tab (deep link, reload) —
   *  append it without disturbing the preview slot semantics. */
  ensure: (path: string) => void;
  /** Remove the tab, its pin and its draft. Callers decide about the
   *  neighbor to activate; they can see `paths` before calling. */
  close: (path: string) => void;
  /** Keep only this tab. Callers flush any unsaved neighbour first — this
   *  drops the other buffers without asking. */
  closeOthers: (path: string) => void;
  /** Materialize the editing buffer the first time a page's content
   *  arrives. Later calls are no-ops — a buffer that exists is ahead of
   *  the server by definition, and must never be reset from under it. */
  initDraft: (path: string, body: string) => void;
  setDraft: (path: string, body: string) => void;
  /** After a save lands: adopt the server's canonical body only if the
   *  buffer still is what was sent. Typing that happened during the round
   *  trip keeps the buffer ahead, and the next autosave carries it. */
  syncIfUnchanged: (path: string, sent: string, canonical: string) => void;
  /** A page moved: retarget its tab, pin and draft so nothing about the
   *  page — not even unsaved text — is lost to the rename. */
  rekey: (from: string, to: string) => void;
}

export function useDocTabs(): DocTabs {
  const queryClient = useQueryClient();
  const [paths, setPaths] = useState<string[]>(() => loadList(TABS_KEY));
  const [pinned, setPinned] = useState<Set<string>>(
    () => new Set(loadList(PINNED_KEY)),
  );
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const guard = useRef<{ path: string; at: number } | null>(null);

  useEffect(() => persist(TABS_KEY, paths), [paths]);
  useEffect(() => persist(PINNED_KEY, [...pinned]), [pinned]);

  const isDirty = useCallback(
    (path: string) => {
      const draft = drafts[path];
      if (draft === undefined) return false;
      const saved = queryClient.getQueryData<DocBodyDto>(
        queryKeys.doc(path),
      )?.body;
      return draft !== saved;
    },
    [drafts, queryClient],
  );

  const preview = useCallback(
    (path: string) => {
      setPaths((prev) => {
        // Pinned tabs stay; every other tab was a preview slot, and this
        // click takes it over.
        const kept = prev.filter((tab) => pinned.has(tab) || tab === path);
        return kept.includes(path) ? kept : [...kept, path];
      });
    },
    [pinned],
  );

  const pin = useCallback((path: string) => {
    setPinned((prev) => {
      if (prev.has(path)) return prev;
      const next = new Set(prev);
      next.add(path);
      return next;
    });
    setPaths((prev) => (prev.includes(path) ? prev : [...prev, path]));
  }, []);

  const unpin = useCallback((path: string) => {
    setPinned((prev) => {
      if (!prev.has(path)) return prev;
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
  }, []);

  const closeOthers = useCallback((path: string) => {
    setPaths((prev) => (prev.includes(path) ? [path] : prev));
    setPinned((prev) => (prev.has(path) ? new Set([path]) : new Set()));
    setDrafts((prev) => (path in prev ? { [path]: prev[path] as string } : {}));
  }, []);

  const ensure = useCallback((path: string) => {
    const g = guard.current;
    if (g !== null && g.path === path && Date.now() - g.at < ENSURE_GUARD_MS)
      return;
    setPaths((prev) => (prev.includes(path) ? prev : [...prev, path]));
  }, []);

  const close = useCallback((path: string) => {
    guard.current = { path, at: Date.now() };
    setPaths((prev) => prev.filter((tab) => tab !== path));
    setPinned((prev) => {
      if (!prev.has(path)) return prev;
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
    setDrafts((prev) => {
      if (!(path in prev)) return prev;
      const next = { ...prev };
      delete next[path];
      return next;
    });
  }, []);

  const initDraft = useCallback((path: string, body: string) => {
    setDrafts((prev) => (path in prev ? prev : { ...prev, [path]: body }));
  }, []);

  const setDraft = useCallback((path: string, body: string) => {
    setDrafts((prev) => (path in prev ? { ...prev, [path]: body } : prev));
  }, []);

  const syncIfUnchanged = useCallback(
    (path: string, sent: string, canonical: string) => {
      setDrafts((prev) =>
        prev[path] === sent ? { ...prev, [path]: canonical } : prev,
      );
    },
    [],
  );

  const rekey = useCallback((from: string, to: string) => {
    if (from === to) return;
    guard.current = { path: from, at: Date.now() };
    setPaths((prev) => prev.map((tab) => (tab === from ? to : tab)));
    setPinned((prev) => {
      if (!prev.has(from)) return prev;
      const next = new Set(prev);
      next.delete(from);
      next.add(to);
      return next;
    });
    setDrafts((prev) => {
      if (!(from in prev)) return prev;
      const next = { ...prev };
      next[to] = next[from] as string;
      delete next[from];
      return next;
    });
  }, []);

  return {
    paths,
    pinned,
    drafts,
    isDirty,
    preview,
    pin,
    unpin,
    ensure,
    close,
    closeOthers,
    initDraft,
    setDraft,
    syncIfUnchanged,
    rekey,
  };
}
