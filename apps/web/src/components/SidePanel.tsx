// The second and third of the workbench's regions, stacked in one column:
// the chosen activity's sub-menu on top, and the open view's own sections
// split below it (PaneSection), the way VS Code stacks Explorer over
// Outline and Timeline.
//
// Only Work and Plan have a sub-menu, because only they hold more than one
// place: Work is Board, Issues and the saved lists over Issues; Plan is
// Timeline, Roadmap and Gantt. Every other activity is one place, and its
// panel is just its sections.
//
// The saved lists sit indented under Issues because that is what they are:
// `#/issues` with a filter. Presenting them as peers of Board invited the
// question "which list is the real one?".

import { useRef, type ReactNode } from "react";
import { ChartGantt, ChevronsLeft, Clock, Columns3, Layers, ListTodo, Search } from "lucide-react";
import type { Route } from "../lib/router";
import { useOpenPool, useSchema, useStatus } from "../lib/queries";
import { doneIds, isInbox } from "../lib/lists";
import { mineQuery } from "../lib/dql";
import { useStarred } from "../lib/starred";
import { cn } from "../lib/cn";
import { clampPanelWidth, type ActivityId } from "../lib/workbench";

function SubRow({
  label,
  icon: Icon,
  active,
  count,
  child = false,
  title,
  onClick,
}: {
  label: string;
  icon?: typeof Columns3;
  active: boolean;
  count?: number | null;
  child?: boolean;
  title?: string;
  onClick: () => void;
}) {
  return (
    <a
      href="#"
      title={title}
      aria-current={active ? "page" : undefined}
      className={cn("sp-row", child && "child", active && "on")}
      onClick={(event) => {
        event.preventDefault();
        onClick();
      }}
    >
      {Icon ? <Icon className="i" aria-hidden /> : null}
      <span className="lbl">{label}</span>
      {count !== undefined && count !== null ? <span className="cnt">{count}</span> : null}
    </a>
  );
}

/** Open issues nobody has picked up — the Work icon's badge and the Inbox
 *  row count the same thing, the way the Inbox list filters it. */
export function useInboxCount(): number | null {
  const schema = useSchema();
  const pool = useOpenPool();
  if (!pool.data) return null;
  const done = doneIds(schema.data?.workflow.statuses);
  return pool.data.items.filter((issue) => isInbox(issue, done)).length;
}

/** Work's sub-menu, with the live counts the old rail carried. Every count
 *  reads the same open pool the lists read. */
function WorkMenu({ route, onNavigate }: { route: Route; onNavigate: (route: Route) => void }) {
  const status = useStatus();
  const schema = useSchema();
  const starred = useStarred();
  const pool = useOpenPool();
  const me = status.data?.me ?? null;
  const statuses = schema.data?.workflow.statuses;
  const done = doneIds(statuses);
  const mine = mineQuery(statuses);
  const open = pool.data?.items ?? [];
  const isMine = route.name === "issues" && route.q === mine && !route.inbox && !route.starred;
  const issuesActive =
    ((route.name === "issues" && !route.inbox && !route.starred) || route.name === "issue" || route.name === "new-issue") &&
    !isMine;
  return (
    <>
      <SubRow label="Board" icon={Columns3} active={route.name === "board"} onClick={() => onNavigate({ name: "board" })} />
      <SubRow
        label="Issues"
        icon={ListTodo}
        title="Every open issue"
        count={pool.data ? pool.data.total : null}
        active={issuesActive}
        onClick={() => onNavigate({ name: "issues", q: null })}
      />
      <SubRow
        child
        label="Inbox"
        title="Open issues with no owner or no @context"
        count={pool.data ? open.filter((issue) => isInbox(issue, done)).length : null}
        active={route.name === "issues" && route.inbox === true}
        onClick={() => onNavigate({ name: "issues", q: null, inbox: true })}
      />
      <SubRow
        child
        label="My issues"
        title={me ? "Open issues assigned to @me" : "No alias configured for @me — set one in Settings"}
        count={pool.data && me ? open.filter((issue) => issue.assignees.includes(me)).length : null}
        active={isMine}
        onClick={() => onNavigate({ name: "issues", q: mine })}
      />
      <SubRow
        child
        label="Starred"
        title="Issues you starred — kept in this browser, never in the repo"
        count={starred.size}
        active={route.name === "issues" && route.starred === true}
        onClick={() => onNavigate({ name: "issues", q: null, starred: true })}
      />
    </>
  );
}

function PlanMenu({ route, onNavigate }: { route: Route; onNavigate: (route: Route) => void }) {
  return (
    <>
      <SubRow label="Timeline" icon={Clock} active={route.name === "timeline"} onClick={() => onNavigate({ name: "timeline" })} />
      <SubRow label="Roadmap" icon={Layers} active={route.name === "roadmap"} onClick={() => onNavigate({ name: "roadmap" })} />
      <SubRow label="Gantt" icon={ChartGantt} active={route.name === "gantt"} onClick={() => onNavigate({ name: "gantt" })} />
    </>
  );
}

export const PANEL_TITLES: Record<ActivityId, string> = {
  home: "Home",
  docs: "Docs",
  morse: "Morse",
  work: "Work",
  flow: "Flow",
  plan: "Plan",
  search: "Search",
  settings: "Settings",
};

export function SidePanel({
  activity,
  route,
  open,
  width,
  onWidth,
  onNavigate,
  onFold,
  onOpenPalette,
  children,
}: {
  activity: ActivityId;
  route: Route;
  open: boolean;
  width: number;
  /** Live while dragging, and once more with `done` when the drag ends. */
  onWidth: (width: number, done: boolean) => void;
  onNavigate: (route: Route) => void;
  onFold: () => void;
  onOpenPalette: () => void;
  /** The open view's own sections — a PaneSection each. */
  children: ReactNode;
}) {
  const drag = useRef<{ x: number; w: number } | null>(null);
  const menu =
    activity === "work" ? (
      <WorkMenu route={route} onNavigate={onNavigate} />
    ) : activity === "plan" ? (
      <PlanMenu route={route} onNavigate={onNavigate} />
    ) : null;

  return (
    <aside
      aria-label={`${PANEL_TITLES[activity]} panel`}
      aria-hidden={!open}
      className={cn("spanel", !open && "folded")}
      style={{ width: open ? width : 0 }}
    >
      <div className="sp-inner" style={{ width }}>
        <div className="sp-h">
          <span className="sp-title">{PANEL_TITLES[activity]}</span>
          <button type="button" className="sp-fold" title="Hide the side panel (⌘B)" onClick={onFold}>
            <ChevronsLeft className="i" aria-hidden />
          </button>
        </div>
        <button type="button" className="sb-search sp-search" onClick={onOpenPalette}>
          <Search className="i" aria-hidden />
          <span>Search or jump to…</span>
          <kbd>⌘K</kbd>
        </button>
        {menu ? (
          <nav className="sp-menu" aria-label={`${PANEL_TITLES[activity]} views`}>
            {menu}
          </nav>
        ) : null}
        <div className="sp-views">{children}</div>
      </div>
      <div
        className="sp-resize"
        role="separator"
        aria-orientation="vertical"
        aria-label="Drag to resize the side panel — double-click resets"
        onPointerDown={(event) => {
          event.preventDefault();
          (event.currentTarget as Element).setPointerCapture(event.pointerId);
          drag.current = { x: event.clientX, w: width };
          event.currentTarget.classList.add("drag");
        }}
        onPointerMove={(event) => {
          const d = drag.current;
          if (d) onWidth(clampPanelWidth(d.w + event.clientX - d.x), false);
        }}
        onPointerUp={(event) => {
          const d = drag.current;
          drag.current = null;
          event.currentTarget.classList.remove("drag");
          if (d) onWidth(clampPanelWidth(d.w + event.clientX - d.x), true);
        }}
        onDoubleClick={() => onWidth(clampPanelWidth(Number.NaN), true)}
      />
    </aside>
  );
}
