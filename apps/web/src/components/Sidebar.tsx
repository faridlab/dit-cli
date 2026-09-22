// The workbench's left column: workspace identity, the search box, labeled
// navigation with live counts, the Plan group, then the active view's own
// section (columns, filters, the pages tree…), with "New issue" and Settings
// pinned to the bottom edge. Nav and section share one scroll region so a
// short window never lets the section run under the pinned row.
//
// ⌘B (or the header's toggle) collapses it to nothing; the header is the way
// back. The pointer never does either — a sweep across the screen must not
// undo a decision.

import type { ReactNode } from "react";
import {
  ChartGantt,
  Clock,
  Columns3,
  FileText,
  GitBranch,
  House,
  Inbox,
  Layers,
  ListTodo,
  Plus,
  Search,
  Settings,
  Star,
  UserRound,
  Waypoints,
} from "lucide-react";
import type { Route } from "../lib/router";
import { useOpenPool, useSchema, useStatus } from "../lib/queries";
import { doneIds, isInbox } from "../lib/lists";
import { mineQuery } from "../lib/dql";
import { useStarred } from "../lib/starred";
import { cn } from "../lib/cn";
import { MenuButton, type MenuItem } from "./chrome";
import logo from "../assets/dit-logo.png";

export type SidebarMode = "expanded" | "hidden";

/** The rail order is also the ⌘1..⌘8 shortcut order (⌘2 is Search, reached
 *  through the palette). AppShell's shortcut table and this list must agree. */
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

export function Sidebar({
  route,
  mode,
  section,
  onNavigate,
  onNewIssue,
  onOpenPalette,
  workspaceMenu,
}: {
  route: Route;
  mode: SidebarMode;
  /** The active view's own surface: filters, columns, the pages tree. */
  section: { title: string; node: ReactNode } | null;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
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

      <div className="sb-scroll">
        <nav className="nav" aria-label="Views">
          <NavLink label="Home" icon={House} shortcut="⌘1" active={route.name === "home"} onClick={() => onNavigate({ name: "home" })} />
          <NavLink
            label="Inbox"
            icon={Inbox}
            title="Open issues with no owner or no @context"
            count={inboxCount}
            active={route.name === "issues" && route.inbox === true}
            onClick={() => onNavigate({ name: "issues", q: null, inbox: true })}
          />
          <NavLink
            label="My issues"
            icon={UserRound}
            title={me ? "Open issues assigned to @me" : "No alias configured for @me — set one in Settings"}
            count={mineCount}
            active={isMine}
            onClick={() => onNavigate({ name: "issues", q: mine })}
          />
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
          <NavLink label="Docs" icon={FileText} shortcut="⌘5" active={route.name === "docs"} onClick={() => onNavigate({ name: "docs", p: null })} />
          <NavLink
            label="Starred"
            icon={Star}
            title="Issues you starred — kept in this browser, never in the repo"
            count={starred.size}
            active={route.name === "issues" && route.starred === true}
            onClick={() => onNavigate({ name: "issues", q: null, starred: true })}
          />
          <div className="sb-h" style={{ marginTop: 8, paddingBottom: 2 }}>
            Plan
          </div>
          <NavLink label="Timeline" icon={Clock} shortcut="⌘6" active={route.name === "timeline"} onClick={() => onNavigate({ name: "timeline" })} />
          <NavLink label="Roadmap" icon={Layers} shortcut="⌘7" active={route.name === "roadmap"} onClick={() => onNavigate({ name: "roadmap" })} />
          <NavLink label="Gantt" icon={ChartGantt} shortcut="⌘8" active={route.name === "gantt"} onClick={() => onNavigate({ name: "gantt" })} />
          <NavLink
            label="Flow"
            icon={Waypoints}
            shortcut="⌘9"
            title="Orchestrations as diagrams: issues as nodes, blocked_by as arrows (ADR 0019)"
            active={route.name === "flow"}
            onClick={() => onNavigate({ name: "flow" })}
          />
        </nav>

        {section ? (
          <section aria-label={`${section.title} section`} className="sb-section">
            {section.node}
          </section>
        ) : null}
      </div>

      <div className="sb-bottom nav">
        <button type="button" className="row" style={{ height: 30 }} onClick={onNewIssue}>
          <Plus className="i" aria-hidden />
          <span className="lbl">New issue</span>
          <kbd>C</kbd>
        </button>
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
