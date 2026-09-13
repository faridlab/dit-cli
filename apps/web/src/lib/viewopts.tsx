// View options shared between surfaces that sit far apart in the tree: the
// sidebar section, the header's Filter / Display / Sort menus and the view
// itself all read and write the same state. None of it is a fact about the
// plan — "I am looking at bugs, grouped by status" — so it lives here, in
// memory, and nowhere near the repo. The two exceptions that a person would
// expect to survive a reload (how an issue opens, whether docs show source)
// are remembered per browser.

import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { NO_FILTERS, type ListFilters } from "./lists";

export type SortKey = "number" | "title" | "priority" | "status" | "due" | "owner" | "updated";
export type SortDir = "asc" | "desc";
export type BoardGroupBy = "status" | "assignee" | "epic" | "context";
export type CardSort = "priority" | "updated" | "due";
export type OpenAs = "panel" | "page";
export type TimelineRange = "7d" | "30d" | "90d" | "all";

/** What the Timeline shows: how far back, which kinds of event, whose. */
export interface TimelineOptions {
  range: TimelineRange;
  /** Empty means every kind. */
  kinds: ReadonlySet<string>;
  who: string | null;
}

export interface BoardOptions {
  groupBy: BoardGroupBy;
  cards: { labels: boolean; due: boolean; epic: boolean };
  colSort: CardSort;
  hidden: ReadonlySet<string>;
}

interface ViewOptions {
  filters: ListFilters;
  setFilters: (next: ListFilters | ((current: ListFilters) => ListFilters)) => void;
  toggleMine: () => void;
  toggleContext: (context: string) => void;
  toggleType: (type: string) => void;
  clearFilters: () => void;

  sort: { key: SortKey; dir: SortDir };
  setSort: (key: SortKey) => void;

  board: BoardOptions;
  setGroupBy: (groupBy: BoardGroupBy) => void;
  toggleCardOption: (key: keyof BoardOptions["cards"]) => void;
  setColSort: (sort: CardSort) => void;
  toggleColumn: (id: string) => void;
  hideColumn: (id: string) => void;
  showAllColumns: () => void;

  /** Bulk selection on the Issues table. */
  selected: ReadonlySet<string>;
  toggleSelected: (id: string) => void;
  setSelected: (ids: ReadonlySet<string>) => void;

  timeline: TimelineOptions;
  setTimelineRange: (range: TimelineRange) => void;
  toggleTimelineKind: (kind: string) => void;
  setTimelineWho: (who: string | null) => void;

  /** Docs: the always-on editor shows rendered blocks or the markdown source. */
  docSource: boolean;
  setDocSource: (source: boolean) => void;

  /** How a click on an issue opens it: beside the list, or as the page. */
  openAs: OpenAs;
  setOpenAs: (openAs: OpenAs) => void;
}

const ViewOptionsContext = createContext<ViewOptions | null>(null);

const OPEN_AS_KEY = "dit.openAs";
const DOC_SOURCE_KEY = "dit.docSource";

function remembered<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key);
    return raw !== null && (allowed as readonly string[]).includes(raw) ? (raw as T) : fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // A blocked or full localStorage only loses the remembered choice.
  }
}

function toggled<T>(set: ReadonlySet<T>, value: T): ReadonlySet<T> {
  const next = new Set(set);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

export function ViewOptionsProvider({ children }: { children: ReactNode }) {
  const [filters, setFilters] = useState<ListFilters>(NO_FILTERS);
  const [sort, setSortState] = useState<{ key: SortKey; dir: SortDir }>({ key: "priority", dir: "asc" });
  const [board, setBoard] = useState<BoardOptions>({
    groupBy: "status",
    cards: { labels: true, due: true, epic: false },
    colSort: "priority",
    hidden: new Set(),
  });
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [timeline, setTimeline] = useState<TimelineOptions>({ range: "30d", kinds: new Set(), who: null });
  const [docSource, setDocSourceState] = useState(() => remembered(DOC_SOURCE_KEY, ["0", "1"], "0") === "1");
  const [openAs, setOpenAsState] = useState<OpenAs>(() => remembered(OPEN_AS_KEY, ["panel", "page"], "panel"));

  const setSort = useCallback((key: SortKey) => {
    setSortState((current) =>
      current.key === key ? { key, dir: current.dir === "asc" ? "desc" : "asc" } : { key, dir: "asc" },
    );
  }, []);

  const value = useMemo<ViewOptions>(
    () => ({
      filters,
      setFilters,
      toggleMine: () => setFilters((f) => ({ ...f, mine: !f.mine })),
      toggleContext: (context) => setFilters((f) => ({ ...f, contexts: toggled(f.contexts, context) })),
      toggleType: (type) => setFilters((f) => ({ ...f, types: toggled(f.types, type) })),
      clearFilters: () => setFilters(NO_FILTERS),

      sort,
      setSort,

      board,
      // Changing the grouping makes the hidden set meaningless: clear it.
      setGroupBy: (groupBy) => setBoard((b) => ({ ...b, groupBy, hidden: new Set() })),
      toggleCardOption: (key) => setBoard((b) => ({ ...b, cards: { ...b.cards, [key]: !b.cards[key] } })),
      setColSort: (colSort) => setBoard((b) => ({ ...b, colSort })),
      toggleColumn: (id) => setBoard((b) => ({ ...b, hidden: toggled(b.hidden, id) })),
      hideColumn: (id) => setBoard((b) => ({ ...b, hidden: new Set([...b.hidden, id]) })),
      showAllColumns: () => setBoard((b) => ({ ...b, hidden: new Set() })),

      selected,
      toggleSelected: (id) => setSelected((s) => toggled(s, id)),
      setSelected,

      timeline,
      setTimelineRange: (range) => setTimeline((t) => ({ ...t, range })),
      toggleTimelineKind: (kind) => setTimeline((t) => ({ ...t, kinds: toggled(t.kinds, kind) })),
      setTimelineWho: (who) => setTimeline((t) => ({ ...t, who: t.who === who ? null : who })),

      docSource,
      setDocSource: (source) => {
        setDocSourceState(source);
        remember(DOC_SOURCE_KEY, source ? "1" : "0");
      },

      openAs,
      setOpenAs: (next) => {
        setOpenAsState(next);
        remember(OPEN_AS_KEY, next);
      },
    }),
    [filters, sort, setSort, board, selected, timeline, docSource, openAs],
  );

  return <ViewOptionsContext.Provider value={value}>{children}</ViewOptionsContext.Provider>;
}

export function useViewOptions(): ViewOptions {
  const value = useContext(ViewOptionsContext);
  if (value === null) throw new Error("useViewOptions must be used inside ViewOptionsProvider");
  return value;
}

/** The board's hidden-column state, in the shape the board pane and view
 *  already consume. */
export function useBoardColumns(): {
  hidden: ReadonlySet<string>;
  toggle: (id: string) => void;
  showAll: () => void;
} {
  const { board, toggleColumn, showAllColumns } = useViewOptions();
  return { hidden: board.hidden, toggle: toggleColumn, showAll: showAllColumns };
}
