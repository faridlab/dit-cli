// The workbench's left column: workspace identity, the search box, labeled
// navigation with live counts, then the active view's own section (columns,
// filters, the pages tree…), with Settings pinned to the bottom edge.
//
// The navigation does not scroll. Only the view's own section does, because
// that is the part that grows without limit — a lane's waiting list, a
// board's columns, a docs tree. Scrolling them together meant reaching the
// bottom of a long list pushed Home and Board off the screen, so getting
// back to another view began with scrolling up to find it.
//
// "New issue" is not here. The header already draws it as the one coloured
// action on every screen, with the same `C` shortcut; a second copy pinned
// down here was the same button twice, and it cost the section the height
// it actually needed.
//
// The nav is grouped because a flat list of eleven rows hid what kind of
// thing each row was. Three kinds live here and they are not equals:
//
//   - **Material**: Home and Docs. Different content, not a view of issues.
//   - **Lenses**: Board, Issues, Flow, Timeline, Roadmap, Gantt — six
//     projections of one set. Grouped Work and Plan by what the reader came
//     to do, not by how the screen is drawn.
//   - **Saved queries**: Inbox, My issues, Starred are literally
//     `#/issues?…` with a filter. They are drawn as children of Issues,
//     indented and quieter, because that is what they are — presenting them
//     as peers of Board invited the question "which list is the real one?".
//
// ⌘B (or the header's toggle) collapses it to nothing; the header is the way
// back. The pointer never does either — a sweep across the screen must not
// undo a decision.

import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import {
  ChartGantt,
  Clock,
  Columns3,
  FileText,
  GitBranch,
  House,
  Layers,
  ListTodo,
  Radio,
  Search,
  Settings,
  Waypoints,
} from "lucide-react";
import type { Route } from "../lib/router";
import { useOpenPool, useSchema, useStatus } from "../lib/queries";
import { doneIds, isInbox } from "../lib/lists";
import { mineQuery } from "../lib/dql";
import { useStarred } from "../lib/starred";
import { cn } from "../lib/cn";
import { clampSplit, readSplit, writeSplit } from "../lib/sidebarsplit";
import { MenuButton, type MenuItem } from "./chrome";
import logo from "../assets/dit-logo.png";

export type SidebarMode = "expanded" | "hidden";

/** The ⌘1..⌘9 and ⌘0 shortcut order (⌘2 is Search, reached through the
 *  palette). AppShell's shortcut table and this list must agree — and every
 *  label a rail row prints must have an entry here, or the row advertises a
 *  key that does nothing. */
export const SHORTCUT_VIEWS: Route[] = [
  { name: "home" },
  { name: "search", q: "" },
  { name: "board" },
  { name: "issues", q: null },
  { name: "docs", p: null },
  { name: "timeline" },
  { name: "roadmap" },
  { name: "gantt" },
  { name: "flow" },
  { name: "morse" },
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
    <a
      href="#"
      onClick={(event) => {
        event.preventDefault();
        onClick();
      }}
      title={title}
      aria-current={active ? "page" : undefined}
      className={cn(active && "on")}
    >
      <Icon className="i" aria-hidden />
      <span className="lbl">{label}</span>
      {count !== undefined && count !== null ? <span className="cnt">{count}</span> : null}
      {shortcut ? <kbd>{shortcut}</kbd> : null}
    </a>
  );
}

/** The draggable line between the navigation and the view's own section.
 *  The nav is a fixed list and the section is not, so who gets the height is
 *  the reader's call, not a constant someone picked once. Dragged with the
 *  pointer, nudged with the arrow keys, and reset by double-clicking — and
 *  remembered per browser, because a window's height is not a fact about the
 *  plan. */
function Splitter({
  onDrag,
  onNudge,
  onReset,
}: {
  onDrag: (clientY: number, phase: "start" | "move" | "end") => void;
  onNudge: (delta: number) => void;
  onReset: () => void;
}) {
  return (
    <div
      className="sb-split"
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize the navigation — arrow keys nudge, double-click resets"
      title="Drag to give the section below more room · double-click to reset"
      tabIndex={0}
      onPointerDown={(event) => {
        event.preventDefault();
        (event.currentTarget as Element).setPointerCapture(event.pointerId);
        onDrag(event.clientY, "start");
      }}
      onPointerMove={(event) => {
        if (!(event.currentTarget as Element).hasPointerCapture(event.pointerId)) return;
        onDrag(event.clientY, "move");
      }}
      onPointerUp={(event) => onDrag(event.clientY, "end")}
      onDoubleClick={onReset}
      onKeyDown={(event) => {
        if (event.key === "ArrowUp") {
          event.preventDefault();
          onNudge(-16);
        } else if (event.key === "ArrowDown") {
          event.preventDefault();
          onNudge(16);
        } else if (event.key === "Escape") {
          event.preventDefault();
          onReset();
        }
      }}
    >
      <span className="grip" aria-hidden />
    </div>
  );
}

/** A saved query over the issue list: the same destination as Issues with a
 *  filter applied. Indented under it and drawn quieter, so the rail says
 *  which rows are places and which are questions. */
function NavQuery({
  label,
  active,
  count,
  onClick,
  title,
}: {
  label: string;
  active: boolean;
  count?: number | null;
  onClick: () => void;
  title?: string;
}) {
  return (
    <a
      href="#"
      onClick={(event) => {
        event.preventDefault();
        onClick();
      }}
      title={title}
      aria-current={active ? "page" : undefined}
      className={cn("q", active && "on")}
    >
      <span className="lbl">{label}</span>
      {count !== undefined && count !== null ? <span className="cnt">{count}</span> : null}
    </a>
  );
}

export function Sidebar({
  route,
  mode,
  section,
  onNavigate,
  onOpenPalette,
  workspaceMenu,
}: {
  route: Route;
  mode: SidebarMode;
  /** The active view's own surface: filters, columns, the pages tree. */
  section: { title: string; node: ReactNode } | null;
  onNavigate: (route: Route) => void;
  onOpenPalette: () => void;
  /** The workspace menu behind the logo row. */
  workspaceMenu: MenuItem[];
}) {
  const status = useStatus();
  const schema = useSchema();
  const starred = useStarred();
  const pool = useOpenPool();
  const workspace = status.data
    ? (status.data.repo.split("/").filter(Boolean).pop() ?? status.data.repo)
    : "…";
  const me = status.data?.me ?? null;

  // Every count reads the same open pool the lists read.
  const statuses = schema.data?.workflow.statuses;
  const done = doneIds(statuses);
  const mine = mineQuery(statuses);
  const isMine = route.name === "issues" && route.q === mine && !route.inbox && !route.starred;
  const open = pool.data?.items ?? [];
  const inboxCount = pool.data ? open.filter((issue) => isInbox(issue, done)).length : null;
  const mineCount = pool.data && me ? open.filter((issue) => issue.assignees.includes(me)).length : null;
  const openCount = pool.data ? pool.data.total : null;

  // The split between the nav and the section, in pixels of nav height.
  // `null` means nobody has dragged it and the nav keeps its natural height,
  // so an untouched rail looks exactly as it always did.
  const shell = useRef<HTMLDivElement>(null);
  const [split, setSplit] = useState<number | null>(() => readSplit());
  const drag = useRef<{ from: number; at: number } | null>(null);

  const room = useCallback(() => shell.current?.getBoundingClientRect().height ?? 0, []);

  const apply = useCallback(
    (next: number | null) => {
      if (next === null) {
        setSplit(null);
        writeSplit(null);
        return;
      }
      const clamped = clampSplit(next, room());
      setSplit(clamped);
      writeSplit(clamped);
    },
    [room],
  );

  const onDrag = useCallback(
    (clientY: number, phase: "start" | "move" | "end") => {
      if (phase === "start") {
        const height =
          split ?? shell.current?.querySelector(".nav")?.getBoundingClientRect().height ?? 0;
        drag.current = { from: clientY, at: height };
        return;
      }
      const start = drag.current;
      if (start === null) return;
      apply(start.at + (clientY - start.from));
      if (phase === "end") drag.current = null;
    },
    [apply, split],
  );

  const onNudge = useCallback(
    (delta: number) => {
      const height =
        split ?? shell.current?.querySelector(".nav")?.getBoundingClientRect().height ?? 0;
      apply(height + delta);
    },
    [apply, split],
  );

  // A window that shrinks below the stored split has to give the section its
  // minimum back, or the drag from a taller screen follows you down.
  useLayoutEffect(() => {
    if (split === null) return;
    const element = shell.current;
    if (element === null) return;
    const observer = new ResizeObserver(() => {
      const fits = clampSplit(split, element.getBoundingClientRect().height);
      if (fits !== split) setSplit(fits);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [split]);

  const issuesActive =
    (route.name === "issues" && !route.inbox && !route.starred) ||
    route.name === "issue" ||
    route.name === "new-issue";

  return (
    <aside
      aria-label="Sidebar"
      aria-hidden={mode === "hidden"}
      className={cn("sidebar shrink-0", mode === "hidden" && "collapsed")}
    >
      <div className="sb-top">
        <MenuButton items={workspaceMenu}>
          <button type="button" className="ws" title="Workspace menu">
            <img className="logo-img" src={logo} alt="DIT" width={24} height={24} draggable={false} />
            <span className="name">{workspace}</span>
            {status.data ? (
              <span className="branch">
                <GitBranch className="i" style={{ width: 12, height: 12 }} aria-hidden />
                {status.data.branch}
              </span>
            ) : null}
          </button>
        </MenuButton>
        <button type="button" className="sb-search" onClick={onOpenPalette}>
          <Search className="i" aria-hidden />
          <span>Search or jump to…</span>
          <kbd>⌘K</kbd>
        </button>
      </div>

      <div className="sb-shell" ref={shell}>
      <nav
        className="nav"
        aria-label="Views"
        style={split === null ? undefined : { height: split, overflowY: "auto" }}
      >
          <NavLink label="Home" icon={House} shortcut="⌘1" active={route.name === "home"} onClick={() => onNavigate({ name: "home" })} />
          <NavLink label="Docs" icon={FileText} shortcut="⌘5" active={route.name === "docs"} onClick={() => onNavigate({ name: "docs", p: null })} />
          <NavLink
            label="Morse"
            icon={Radio}
            shortcut="⌘0"
            title="API scenarios in the repo: endpoints derived from the registered OpenAPI specs, and whether each scenario still matches (§20)"
            active={route.name === "morse"}
            onClick={() => onNavigate({ name: "morse" })}
          />

          <div className="sb-h nav-h">Work</div>
          <NavLink label="Board" icon={Columns3} shortcut="⌘3" active={route.name === "board"} onClick={() => onNavigate({ name: "board" })} />
          <NavLink
            label="Issues"
            icon={ListTodo}
            shortcut="⌘4"
            title="Every open issue"
            count={openCount}
            active={issuesActive && !isMine}
            onClick={() => onNavigate({ name: "issues", q: null })}
          />
          <NavQuery
            label="Inbox"
            title="Open issues with no owner or no @context"
            count={inboxCount}
            active={route.name === "issues" && route.inbox === true}
            onClick={() => onNavigate({ name: "issues", q: null, inbox: true })}
          />
          <NavQuery
            label="My issues"
            title={me ? "Open issues assigned to @me" : "No alias configured for @me — set one in Settings"}
            count={mineCount}
            active={isMine}
            onClick={() => onNavigate({ name: "issues", q: mine })}
          />
          <NavQuery
            label="Starred"
            title="Issues you starred — kept in this browser, never in the repo"
            count={starred.size}
            active={route.name === "issues" && route.starred === true}
            onClick={() => onNavigate({ name: "issues", q: null, starred: true })}
          />
          <NavLink
            label="Flow"
            icon={Waypoints}
            shortcut="⌘9"
            title="Orchestrations as diagrams: issues as nodes, blocked_by as arrows (ADR 0019)"
            active={route.name === "flow"}
            onClick={() => onNavigate({ name: "flow" })}
          />

          <div className="sb-h nav-h">Plan</div>
          <NavLink label="Timeline" icon={Clock} shortcut="⌘6" active={route.name === "timeline"} onClick={() => onNavigate({ name: "timeline" })} />
          <NavLink label="Roadmap" icon={Layers} shortcut="⌘7" active={route.name === "roadmap"} onClick={() => onNavigate({ name: "roadmap" })} />
          <NavLink label="Gantt" icon={ChartGantt} shortcut="⌘8" active={route.name === "gantt"} onClick={() => onNavigate({ name: "gantt" })} />
      </nav>

      {section ? (
        <>
          <Splitter onDrag={onDrag} onNudge={onNudge} onReset={() => apply(null)} />
          <section aria-label={`${section.title} section`} className="sb-section">
            {section.node}
          </section>
        </>
      ) : null}
      </div>

      <div className="sb-bottom nav">
        <a
          href="#/settings"
          onClick={(event) => {
            event.preventDefault();
            onNavigate({ name: "settings" });
          }}
          className={cn("row", route.name === "settings" && "on")}
          style={{ height: 30 }}
        >
          <Settings className="i" aria-hidden />
          <span className="lbl">Settings</span>
          <kbd>⌘,</kbd>
        </a>
      </div>
    </aside>
  );
}
