// The workbench's left column: workspace identity, the search box, labeled
// navigation with counts, then the active view's own section (filters,
// columns, the pages tree…), with "New issue" and Settings pinned to the
// bottom edge. One scroll region holds nav + section so a short window
// never lets the section run under the pinned row.
//
// Two modes: expanded (resizable by dragging the right edge) and hidden
// (⌘B), which leaves a slim strip carrying the logo so the way back is
// always on screen.

import {
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import {
  Columns3,
  FileText,
  GitBranch,
  House,
  Inbox,
  ListTodo,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  ChartGantt,
  Clock,
  Layers,
  Settings,
  Star,
  UserRound,
} from "lucide-react";
import * as Tooltip from "@radix-ui/react-tooltip";
import type { Route } from "../lib/router";
import { useIssues, useSchema, useStatus } from "../lib/queries";
import { INBOX_QUERY, mineQuery, openQuery } from "../lib/dql";
import { useStarred } from "../lib/starred";
import { cn } from "../lib/cn";
import { Kbd, SectionHeading } from "./chrome";
import logo from "../assets/dit-logo.png";

export type SidebarMode = "expanded" | "hidden";

export const SIDEBAR_HIDDEN_WIDTH = 44;
export const SIDEBAR_MIN_WIDTH = 220;
export const SIDEBAR_MAX_WIDTH = 420;
export const SIDEBAR_DEFAULT_WIDTH = 252;

/** The rail order is also the ⌘1..⌘5 shortcut order; AppShell's shortcut
 *  table and this list must agree. Inbox and My issues are filtered lists,
 *  so they carry no number of their own. */
export const SHORTCUT_VIEWS: Route[] = [
  { name: "home" },
  { name: "search", q: "" },
  { name: "board" },
  { name: "issues", q: null },
  { name: "docs", p: null },
  { name: "timeline" },
  { name: "roadmap" },
  { name: "gantt" },
];

function NavLink({
  label,
  icon: Icon,
  active,
  shortcut,
  count,
  onClick,
  title,
}: {
  label: string;
  icon: typeof House;
  active: boolean;
  shortcut?: string;
  count?: number | null;
  onClick: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-current={active ? "page" : undefined}
      className={cn(
        "group flex h-[30px] w-full items-center gap-2.5 rounded-md px-2 text-left text-[13px] transition-colors",
        active ? "bg-accent-soft font-medium text-context" : "text-ink-2 hover:bg-hover hover:text-ink",
      )}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="flex-1 truncate">{label}</span>
      {count !== undefined && count !== null ? (
        <span
          className={cn(
            "font-mono text-[11px] tabular-nums",
            active ? "text-context" : "text-muted",
          )}
        >
          {count}
        </span>
      ) : null}
      {shortcut ? (
        <Kbd className="opacity-0 transition-opacity group-hover:opacity-100">{shortcut}</Kbd>
      ) : null}
    </button>
  );
}

export function Sidebar({
  route,
  mode,
  width,
  section,
  onNavigate,
  onNewIssue,
  onOpenPalette,
  onToggle,
  onResizeStart,
  onResize,
  onResizeEnd,
}: {
  route: Route;
  mode: SidebarMode;
  width: number;
  /** The active view's own surface: filters, columns, the pages tree. */
  section: { title: string; node: ReactNode } | null;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
  onOpenPalette: () => void;
  onToggle: () => void;
  onResizeStart: () => void;
  /** Live width while the handle is dragged; already clamped. */
  onResize: (width: number) => void;
  onResizeEnd: (width: number) => void;
}) {
  const status = useStatus();
  const schema = useSchema();
  const starred = useStarred();
  const statuses = schema.data?.workflow.statuses;
  const workspace = status.data
    ? (status.data.repo.split("/").filter(Boolean).pop() ?? status.data.repo)
    : "…";

  // Counts ride the same index the lists use: a `limit: 1` query costs one
  // row and still reports the total.
  const mine = mineQuery(statuses);
  const open = openQuery(statuses);
  const inboxCount = useIssues({ q: INBOX_QUERY, limit: 1 });
  const mineCount = useIssues({ q: mine, limit: 1 }, status.data?.me != null);
  const openCount = useIssues(open ? { q: open, limit: 1 } : { limit: 1 });

  const [resizing, setResizing] = useState(false);
  const drag = useRef({ startX: 0, startWidth: 0, lastWidth: 0 });

  const beginResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    event.preventDefault();
    drag.current = { startX: event.clientX, startWidth: width, lastWidth: width };
    onResizeStart();
    setResizing(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const moveResize = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!resizing) return;
    const next = Math.min(
      SIDEBAR_MAX_WIDTH,
      Math.max(SIDEBAR_MIN_WIDTH, drag.current.startWidth + event.clientX - drag.current.startX),
    );
    drag.current.lastWidth = next;
    onResize(next);
  };
  const endResize = () => {
    if (!resizing) return;
    setResizing(false);
    onResizeEnd(drag.current.lastWidth);
  };

  const isIssuesWith = (q: string | null) =>
    route.name === "issues" && route.q === q && route.starred !== true;
  const issuesActive =
    (route.name === "issues" &&
      route.q !== INBOX_QUERY &&
      route.q !== mine &&
      route.starred !== true) ||
    route.name === "issue" ||
    route.name === "new-issue";

  if (mode === "hidden") {
    return (
      <Tooltip.Provider delayDuration={350}>
        <aside
          aria-label="Sidebar (hidden)"
          style={{ width: SIDEBAR_HIDDEN_WIDTH }}
          className="flex shrink-0 flex-col items-center gap-1 border-r border-edge bg-panel pt-2.5"
        >
          <button
            type="button"
            onClick={() => onNavigate({ name: "home" })}
            title={`${workspace} — Home`}
            className="flex size-8 items-center justify-center rounded-md hover:bg-hover"
          >
            <img src={logo} alt="" width={22} height={22} className="size-[22px]" draggable={false} />
          </button>
          <button
            type="button"
            onClick={onToggle}
            title="Show the sidebar (⌘B)"
            className="flex size-8 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-ink"
          >
            <PanelLeftOpen className="size-4" aria-hidden />
          </button>
        </aside>
      </Tooltip.Provider>
    );
  }

  return (
    <aside
      style={{ width }}
      aria-label="Sidebar"
      className="relative flex shrink-0 flex-col border-r border-edge bg-panel"
    >
      {/* Identity + search: fixed at the top. */}
      <div className="flex shrink-0 flex-col gap-2 px-3 pb-1.5 pt-3">
        <div className="flex items-center gap-1">
          <button
            type="button"
            onClick={() => onNavigate({ name: "home" })}
            title={status.data ? `${status.data.repo}\nHome` : "Home"}
            className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 py-1 text-left hover:bg-hover"
          >
            <img src={logo} alt="" width={24} height={24} className="size-6 shrink-0" draggable={false} />
            <span className="truncate text-[13px] font-semibold text-ink">{workspace}</span>
            {status.data ? (
              <span className="ml-auto flex shrink-0 items-center gap-1 font-mono text-[11px] text-muted">
                <GitBranch className="size-3" aria-hidden />
                {status.data.branch}
              </span>
            ) : null}
          </button>
          <button
            type="button"
            onClick={onToggle}
            title="Hide the sidebar (⌘B)"
            className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted hover:bg-hover hover:text-ink"
          >
            <PanelLeftClose className="size-4" aria-hidden />
          </button>
        </div>
        <button
          type="button"
          onClick={onOpenPalette}
          className="flex h-8 items-center gap-2 rounded-md border border-edge bg-card px-2.5 text-left text-[13px] text-muted transition-colors hover:border-ctl"
        >
          <Search className="size-4 shrink-0" aria-hidden />
          <span className="flex-1 truncate">Search or jump to…</span>
          <Kbd>⌘K</Kbd>
        </button>
      </div>

      {/* Nav + the view's section share one scroll region. */}
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden">
        <nav aria-label="Views" className="flex shrink-0 flex-col gap-px px-2 py-1">
          <NavLink label="Home" icon={House} shortcut="⌘1" active={route.name === "home"} onClick={() => onNavigate({ name: "home" })} />
          <NavLink
            label="Inbox"
            icon={Inbox}
            title={INBOX_QUERY}
            count={inboxCount.data?.total ?? null}
            active={isIssuesWith(INBOX_QUERY)}
            onClick={() => onNavigate({ name: "issues", q: INBOX_QUERY })}
          />
          <NavLink
            label="My issues"
            icon={UserRound}
            title={status.data?.me ? mine : "No git identity configured for @me"}
            count={status.data?.me ? (mineCount.data?.total ?? null) : null}
            active={isIssuesWith(mine)}
            onClick={() => onNavigate({ name: "issues", q: mine })}
          />
          <NavLink label="Board" icon={Columns3} shortcut="⌘3" active={route.name === "board"} onClick={() => onNavigate({ name: "board" })} />
          <NavLink
            label="Issues"
            icon={ListTodo}
            shortcut="⌘4"
            title={open ?? "all issues"}
            count={openCount.data?.total ?? null}
            active={issuesActive}
            onClick={() => onNavigate({ name: "issues", q: null })}
          />
          <NavLink label="Docs" icon={FileText} shortcut="⌘5" active={route.name === "docs"} onClick={() => onNavigate({ name: "docs", p: null })} />
          <NavLink
            label="Starred"
            icon={Star}
            title="Issues you starred — kept in this browser, never in the repo"
            count={starred.size}
            active={route.name === "issues" && route.starred === true}
            onClick={() => onNavigate({ name: "issues", q: null, starred: true })}
          />
          <NavLink label="Search" icon={Search} shortcut="⌘2" active={route.name === "search"} onClick={() => onNavigate({ name: "search", q: "" })} />
        </nav>

        <div className="mt-1.5 px-2">
          <SectionHeading size="sm" className="px-2 pb-1">
            Plan
          </SectionHeading>
        </div>
        <nav aria-label="Plan views" className="flex shrink-0 flex-col gap-px px-2 pb-1">
          <NavLink
            label="Timeline"
            icon={Clock}
            shortcut="⌘6"
            title="Every field change, and the board as it stood at any point"
            active={route.name === "timeline"}
            onClick={() => onNavigate({ name: "timeline" })}
          />
          <NavLink
            label="Roadmap"
            icon={Layers}
            shortcut="⌘7"
            title="Epics across quarters"
            active={route.name === "roadmap"}
            onClick={() => onNavigate({ name: "roadmap" })}
          />
          <NavLink
            label="Gantt"
            icon={ChartGantt}
            shortcut="⌘8"
            title="Issues on a time axis, dragged to reschedule"
            active={route.name === "gantt"}
            onClick={() => onNavigate({ name: "gantt" })}
          />
        </nav>

        {section ? (
          <section
            aria-label={`${section.title} section`}
            className="flex min-h-[180px] flex-1 flex-col border-t border-edge"
          >
            {section.node}
          </section>
        ) : null}
      </div>

      {/* Pinned bottom row. */}
      <div className="flex shrink-0 flex-col gap-px border-t border-edge bg-panel px-2 py-2">
        <NavLink label="New issue" icon={Plus} shortcut="C" active={false} onClick={onNewIssue} />
        <NavLink label="Settings" icon={Settings} shortcut="⌘," active={route.name === "settings"} onClick={() => onNavigate({ name: "settings" })} />
      </div>

      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize the sidebar"
        onPointerDown={beginResize}
        onPointerMove={moveResize}
        onPointerUp={endResize}
        onPointerCancel={endResize}
        className={cn(
          "absolute inset-y-0 right-0 z-10 w-[5px] cursor-col-resize touch-none",
          resizing ? "bg-accent/60" : "hover:bg-accent/30",
        )}
      />
    </aside>
  );
}
