// The header's view actions: Filter (shared by Board, Issues, Search and
// Gantt), Display (Board) and Sort (Issues). Each is a menu over the shared
// view options, so the sidebar rows, the chips in the filters bar and these
// buttons are three views of one state.

import { ChevronsUpDown, Filter, SlidersHorizontal, X } from "lucide-react";
import { useOpenPool, useStatus } from "../lib/queries";
import { contextsOf, filterCount, ISSUE_TYPES } from "../lib/lists";
import { useViewOptions, type BoardGroupBy, type CardSort, type SortKey } from "../lib/viewopts";
import { TypeBadge } from "./badges";
import { Btn, MenuButton, type MenuItem } from "./chrome";

export const SORT_LABELS: Record<SortKey, string> = {
  number: "Number",
  title: "Title",
  priority: "Priority",
  status: "Status",
  due: "Due",
  owner: "Owner",
  updated: "Updated",
};

export function FilterButton() {
  const { filters, toggleMine, toggleContext, toggleType, clearFilters } = useViewOptions();
  const pool = useOpenPool();
  const status = useStatus();
  const contexts = contextsOf(pool.data?.items ?? []);
  const count = filterCount(filters);
  const items: MenuItem[] = [
    { kind: "head", label: "Filter" },
    {
      label: "Assigned to me",
      check: filters.mine,
      disabled: !status.data?.me,
      run: toggleMine,
    },
    { kind: "head", label: "Context" },
    ...(contexts.length > 0
      ? contexts.map((context): MenuItem => ({
          label: `@${context}`,
          check: filters.contexts.has(context),
          run: () => toggleContext(context),
        }))
      : [{ kind: "text" as const, node: "No context: labels yet." }]),
    { kind: "head", label: "Type" },
    ...ISSUE_TYPES.map((type): MenuItem => ({
      label: (
        <span className="flex items-center gap-2">
          <TypeBadge type={type} />
          {type}
        </span>
      ),
      check: filters.types.has(type),
      run: () => toggleType(type),
    })),
    { kind: "sep" },
    { label: "Clear all", icon: <X className="i" aria-hidden />, run: clearFilters },
  ];
  return (
    <MenuButton items={items} align="end">
      <Btn title="Filter the list">
        <Filter className="i" aria-hidden />
        Filter
        {count > 0 ? <span className="cnt mono" style={{ color: "var(--accent-ink)" }}>{count}</span> : null}
      </Btn>
    </MenuButton>
  );
}

const GROUPS: Array<[BoardGroupBy, string]> = [
  ["status", "Status"],
  ["assignee", "Assignee"],
  ["epic", "Epic"],
  ["context", "Context"],
];
const CARD_SORTS: Array<[CardSort, string]> = [
  ["priority", "Priority"],
  ["updated", "Recently updated"],
  ["due", "Due date"],
];

export function DisplayButton() {
  const { board, setGroupBy, toggleCardOption, setColSort } = useViewOptions();
  const items: MenuItem[] = [
    { kind: "head", label: "Group by" },
    ...GROUPS.map(([key, label]): MenuItem => ({ label, on: board.groupBy === key, run: () => setGroupBy(key) })),
    { kind: "head", label: "Cards" },
    { label: "Show labels", check: board.cards.labels, run: () => toggleCardOption("labels") },
    { label: "Show due dates", check: board.cards.due, run: () => toggleCardOption("due") },
    { label: "Show epic on cards", check: board.cards.epic, run: () => toggleCardOption("epic") },
    { kind: "head", label: "Order cards by" },
    ...CARD_SORTS.map(([key, label]): MenuItem => ({ label, on: board.colSort === key, run: () => setColSort(key) })),
  ];
  return (
    <MenuButton items={items} align="end">
      <Btn title="Display options">
        <SlidersHorizontal className="i" aria-hidden />
        Display
      </Btn>
    </MenuButton>
  );
}

export function SortButton() {
  const { sort, setSort } = useViewOptions();
  const items: MenuItem[] = [
    { kind: "head", label: "Sort by" },
    ...(Object.keys(SORT_LABELS) as SortKey[]).map((key): MenuItem => ({
      label: SORT_LABELS[key],
      on: sort.key === key,
      meta: sort.key === key ? (sort.dir === "asc" ? "↑" : "↓") : undefined,
      run: () => setSort(key),
    })),
  ];
  return (
    <MenuButton items={items} align="end">
      <Btn title="Sort the list">
        <ChevronsUpDown className="i" aria-hidden />
        Sort · {sort.key}
      </Btn>
    </MenuButton>
  );
}

/** The mono hint some headers carry instead of a button. */
export function HeaderHint({ children }: { children: string }) {
  return <span className="mono text-[11px] text-faint">{children}</span>;
}
