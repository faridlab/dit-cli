// The dense sortable issue table, shared by the Issues view and the DQL
// search results. Only visible rows are in the DOM (virtualized), because a
// workspace can easily hold tens of thousands of issues and a plain table
// chokes on that well before the network does.

import { KeyboardEvent, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp, Check } from "lucide-react";
import { fullTimestamp, priorityRank, relativeTime } from "../lib/format";
import type { IssueDto, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";
import { AssigneeCircles, IssueHandle, LabelChips, PriorityDot, StatusPill, TypeBadge } from "./badges";

type SortKey = "updated" | "created" | "priority" | "title" | "status";
type SortDir = "asc" | "desc";

const ROW_HEIGHT = 44;

// Header and rows must share the exact same track list or columns drift, so
// the grid is built once per mode and reused verbatim on both. Below 1060px
// the status column drops; below 1240px the labels column drops — each cell
// (header and body alike) carries the matching visibility classes.
// Narrow: checkbox | handle | type | prio | title | updated | assignees.
const GRID_FULL =
  "grid items-center gap-2 grid-cols-[56px_20px_12px_minmax(0,1fr)_96px_84px] min-[1060px]:grid-cols-[62px_20px_12px_minmax(0,1fr)_104px_84px_84px] min-[1240px]:grid-cols-[62px_20px_12px_minmax(0,1fr)_104px_190px_84px_84px]";
const GRID_SELECT =
  "grid items-center gap-2 grid-cols-[28px_56px_20px_12px_minmax(0,1fr)_96px_84px] min-[1060px]:grid-cols-[28px_62px_20px_12px_minmax(0,1fr)_104px_84px_84px] min-[1240px]:grid-cols-[28px_62px_20px_12px_minmax(0,1fr)_104px_190px_84px_84px]";
// Search results: handle | type | prio | title | status | updated.
const GRID_COMPACT =
  "grid grid-cols-[62px_20px_12px_minmax(0,1fr)_104px_84px] items-center gap-2";

function compare(key: SortKey, a: IssueDto, b: IssueDto, statusOrder: string[]): number {
  switch (key) {
    case "updated":
      return Date.parse(a.updated) - Date.parse(b.updated);
    case "created":
      return Date.parse(a.created) - Date.parse(b.created);
    case "priority":
      return priorityRank(a.priority) - priorityRank(b.priority);
    case "title":
      return a.title.localeCompare(b.title);
    case "status": {
      const ia = statusOrder.indexOf(a.status);
      const ib = statusOrder.indexOf(b.status);
      return (ia < 0 ? Number.MAX_SAFE_INTEGER : ia) - (ib < 0 ? Number.MAX_SAFE_INTEGER : ib);
    }
  }
}

function SortHeader({
  label,
  sortKey,
  active,
  dir,
  onSort,
  className,
}: {
  label: string;
  sortKey: SortKey;
  active: boolean;
  dir: SortDir;
  onSort: (key: SortKey) => void;
  className?: string;
}) {
  return (
    <button
      type="button"
      onClick={() => onSort(sortKey)}
      className={cn(
        "flex items-center gap-0.5 text-left text-[10px] font-medium uppercase tracking-[0.07em] hover:text-zinc-300",
        active ? "text-zinc-300" : "text-zinc-500",
        className,
      )}
    >
      {label}
      {active ? (
        dir === "asc" ? (
          <ArrowUp className="size-3" aria-hidden />
        ) : (
          <ArrowDown className="size-3" aria-hidden />
        )
      ) : null}
    </button>
  );
}

function RowCheckbox({
  checked,
  onToggle,
}: {
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-label={checked ? "Deselect issue" : "Select issue"}
      onClick={(event) => {
        // The row underneath is also clickable; a checkbox click selects,
        // it does not open.
        event.stopPropagation();
        onToggle();
      }}
      className={cn(
        "flex size-[14px] shrink-0 items-center justify-center rounded-[3px] border transition-colors",
        checked ? "border-accent bg-accent text-white" : "border-ctl hover:border-zinc-500",
      )}
    >
      {checked ? <Check className="size-3" strokeWidth={3} aria-hidden /> : null}
    </button>
  );
}

export function IssueTable({
  issues,
  statuses,
  onOpen,
  columns = "full",
  selectable = false,
  selected,
  onToggleSelect,
}: {
  issues: IssueDto[];
  statuses: StatusDto[];
  onOpen: (id: string) => void;
  /** `full` is the Issues view grid; `compact` is the search-results grid. */
  columns?: "full" | "compact";
  /** Adds the leading checkbox column for bulk actions. */
  selectable?: boolean;
  selected?: ReadonlySet<string>;
  onToggleSelect?: (id: string) => void;
}) {
  const [sortKey, setSortKey] = useState<SortKey>("updated");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const scrollRef = useRef<HTMLDivElement>(null);

  const compact = columns === "compact";
  const gridClass = compact ? GRID_COMPACT : selectable ? GRID_SELECT : GRID_FULL;
  // The full table bleeds to the view's 20px gutters; the compact search
  // results sit inside a card and use tighter ones.
  const padClass = compact ? "px-3" : "px-5";
  // Visibility classes pair with the track lists above: same breakpoints on
  // the header cell and the body cell of the same column.
  const statusVisibility = compact ? "" : "hidden min-[1060px]:block";
  const labelsVisibility = compact ? "hidden" : "hidden min-[1240px]:block";
  const assigneeVisibility = compact ? "hidden" : "";

  const statusOrder = useMemo(() => statuses.map((status) => status.id), [statuses]);
  const statusById = useMemo(() => {
    const map = new Map<string, StatusDto>();
    for (const status of statuses) map.set(status.id, status);
    return map;
  }, [statuses]);

  const rows = useMemo(() => {
    const sorted = [...issues].sort((a, b) => compare(sortKey, a, b, statusOrder));
    if (sortDir === "desc") sorted.reverse();
    return sorted;
  }, [issues, sortKey, sortDir, statusOrder]);

  const onSort = (key: SortKey) => {
    if (key === sortKey) {
      setSortDir((dir) => (dir === "asc" ? "desc" : "asc"));
    } else {
      setSortKey(key);
      // Text sorts read naturally ascending; recency and priority read
      // naturally as "most/highest first".
      setSortDir(key === "title" || key === "status" ? "asc" : "desc");
    }
  };

  const onRowKeyDown = (event: KeyboardEvent<HTMLDivElement>, id: string) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onOpen(id);
    }
  };

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className={cn(gridClass, "border-b border-edge bg-app pb-1.5 pt-2", padClass)}>
        {selectable ? <span /> : null}
        <span className="text-[10px] font-medium uppercase tracking-[0.07em] text-zinc-500">
          #
        </span>
        <span />
        <span />
        <SortHeader
          label="Title"
          sortKey="title"
          active={sortKey === "title"}
          dir={sortDir}
          onSort={onSort}
        />
        <SortHeader
          label="Status"
          sortKey="status"
          active={sortKey === "status"}
          dir={sortDir}
          onSort={onSort}
          className={statusVisibility}
        />
        <span
          className={cn(
            "text-[10px] font-medium uppercase tracking-[0.07em] text-zinc-500",
            labelsVisibility,
          )}
        >
          Labels
        </span>
        <SortHeader
          label="Updated"
          sortKey="updated"
          active={sortKey === "updated"}
          dir={sortDir}
          onSort={onSort}
          className="justify-end text-right"
        />
        <span className={assigneeVisibility} />
      </div>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
        {rows.length === 0 ? null : (
          <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
            {virtualizer.getVirtualItems().map((virtualRow) => {
              const issue = rows[virtualRow.index];
              if (!issue) return null;
              const status = statusById.get(issue.status);
              return (
                <div
                  key={issue.id}
                  className={cn("absolute left-0 top-0 w-full", padClass)}
                  // Pixel offsets are dynamic per row; this is CSSOM styling,
                  // which the content-security policy allows.
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <div
                    role="button"
                    tabIndex={0}
                    onClick={() => onOpen(issue.id)}
                    onKeyDown={(event) => onRowKeyDown(event, issue.id)}
                    className={cn(
                      gridClass,
                      "h-11 w-full cursor-default border-b border-rowline text-left hover:bg-card focus-visible:bg-card focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-inset focus-visible:ring-accent",
                    )}
                  >
                    {selectable ? (
                      <RowCheckbox
                        checked={selected?.has(issue.id) ?? false}
                        onToggle={() => onToggleSelect?.(issue.id)}
                      />
                    ) : null}
                    <IssueHandle shortRef={issue.short_ref} number={issue.number} />
                    <TypeBadge type={issue.type} />
                    <PriorityDot priority={issue.priority} />
                    <span className="truncate text-[13px] text-zinc-200">{issue.title}</span>
                    <span className={statusVisibility}>
                      {status ? (
                        <StatusPill status={status} />
                      ) : (
                        <span className="font-mono text-[11px] text-zinc-500">{issue.status}</span>
                      )}
                    </span>
                    <span className={labelsVisibility}>
                      <LabelChips labels={issue.labels} />
                    </span>
                    <span
                      className="text-right font-mono text-[11px] tabular-nums text-zinc-500"
                      title={fullTimestamp(issue.updated)}
                    >
                      {relativeTime(issue.updated)}
                    </span>
                    <span className={cn("flex justify-end", assigneeVisibility)}>
                      <AssigneeCircles assignees={issue.assignees} />
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
