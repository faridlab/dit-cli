// The dense sortable issue table, shared by the Issues view and the DQL
// search results. Only visible rows are in the DOM (virtualized), because a
// workspace can easily hold tens of thousands of issues and a plain table
// chokes on that well before the network does.

import { useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDown, ArrowUp } from "lucide-react";
import { fullTimestamp, priorityRank, relativeTime } from "../lib/format";
import type { IssueDto, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";
import { AssigneeCircles, IssueHandle, LabelChips, PriorityDot, StatusPill, TypeBadge } from "./badges";

type SortKey = "updated" | "created" | "priority" | "title" | "status";
type SortDir = "asc" | "desc";

const ROW_HEIGHT = 36;

// Header and rows must share the exact same track list or columns drift.
const GRID_CLASS =
  "grid grid-cols-[84px_24px_16px_minmax(0,1fr)_minmax(96px,110px)_minmax(0,190px)_minmax(0,84px)_80px] items-center gap-2";

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
        "flex items-center gap-0.5 text-left text-[11px] font-medium uppercase tracking-wide hover:text-zinc-200",
        active ? "text-zinc-200" : "text-zinc-500",
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

export function IssueTable({
  issues,
  statuses,
  onOpen,
}: {
  issues: IssueDto[];
  statuses: StatusDto[];
  onOpen: (id: string) => void;
}) {
  const [sortKey, setSortKey] = useState<SortKey>("updated");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const scrollRef = useRef<HTMLDivElement>(null);

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

  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div
        className={cn(
          GRID_CLASS,
          "sticky top-0 z-10 border-b border-zinc-800 bg-zinc-950 px-3 pb-1.5 pt-2",
        )}
      >
        <span className="text-[11px] font-medium uppercase tracking-wide text-zinc-500">
          #
        </span>
        <span />
        <span />
        <SortHeader label="Title" sortKey="title" active={sortKey === "title"} dir={sortDir} onSort={onSort} />
        <SortHeader label="Status" sortKey="status" active={sortKey === "status"} dir={sortDir} onSort={onSort} />
        <span className="text-[11px] font-medium uppercase tracking-wide text-zinc-500">Labels</span>
        <span />
        <SortHeader label="Updated" sortKey="updated" active={sortKey === "updated"} dir={sortDir} onSort={onSort} />
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
                  className="absolute left-0 top-0 w-full px-3"
                  // Pixel offsets are dynamic per row; this is CSSOM styling,
                  // which the content-security policy allows.
                  style={{ transform: `translateY(${virtualRow.start}px)` }}
                >
                  <button
                    type="button"
                    onClick={() => onOpen(issue.id)}
                    className={cn(
                      GRID_CLASS,
                      "h-9 w-full rounded text-left hover:bg-zinc-900 focus-visible:bg-zinc-900 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-sky-700",
                    )}
                  >
                    <IssueHandle shortRef={issue.short_ref} number={issue.number} />
                    <TypeBadge type={issue.type} />
                    <PriorityDot priority={issue.priority} />
                    <span className="truncate text-[13px] text-zinc-200">{issue.title}</span>
                    {status ? (
                      <StatusPill status={status} />
                    ) : (
                      <span className="font-mono text-[11px] text-zinc-500">{issue.status}</span>
                    )}
                    <LabelChips labels={issue.labels} />
                    <AssigneeCircles assignees={issue.assignees} />
                    <span
                      className="text-right text-xs text-zinc-500 tabular-nums"
                      title={fullTimestamp(issue.updated)}
                    >
                      {relativeTime(issue.updated)}
                    </span>
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
