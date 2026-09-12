// Workbench layout: sidebar, main area, status bar, plus the two global
// overlays. Route state, the ⌘K / ⌘B / ⌘1-⌘5 listeners, and the sidebar
// width live here so the views below stay purely about data.

import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useLiveEvents } from "../lib/events";
import { useDocTabs } from "../lib/doctabs";
import { invalidateWorkspaceData } from "../lib/queries";
import { peekHost, peekOf, useNavigate, useRoute, withPeek } from "../lib/router";
import { stepPeek, usePeekList } from "../lib/peeklist";
import { CommandPalette } from "./CommandPalette";
import { IssuePeek } from "./issue/IssuePeek";
import {
  SHORTCUT_VIEWS,
  Sidebar,
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  type SidebarMode,
} from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { BoardColumnsProvider, BoardPane } from "./panes/BoardPane";
import { GanttOptionsProvider, GanttPane } from "./panes/GanttPane";
import { RoadmapOptionsProvider, RoadmapPane } from "./panes/RoadmapPane";
import { TimelinePane } from "./panes/TimelinePane";
import { DocsPane } from "./panes/DocsPane";
import { HomePane } from "./panes/HomePane";
import { IssuesPane } from "./panes/IssuesPane";
import { SearchPane } from "./panes/SearchPane";
import { SettingsPane } from "./panes/SettingsPane";
import { BoardView } from "../views/BoardView";
import { DocsView } from "../views/DocsView";
import { HomeView } from "../views/HomeView";
import { IssueDetailView } from "../views/IssueDetailView";
import { IssuesView } from "../views/IssuesView";
import { NewIssueView } from "../views/NewIssueView";
import { SearchView } from "../views/SearchView";
import { SettingsView } from "../views/SettingsView";
import { GanttView } from "../views/GanttView";
import { RoadmapView } from "../views/RoadmapView";
import { TimelineView } from "../views/TimelineView";

const SIDEBAR_WIDTH_KEY = "dit.sidebar.width";

function loadSidebarWidth(): number {
  try {
    const raw = window.localStorage.getItem(SIDEBAR_WIDTH_KEY);
    const parsed = raw === null ? Number.NaN : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return SIDEBAR_DEFAULT_WIDTH;
    return Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, parsed));
  } catch {
    return SIDEBAR_DEFAULT_WIDTH;
  }
}

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
}

export function AppShell() {
  const queryClient = useQueryClient();
  // The watcher saw a new commit: everything it can have changed goes stale.
  const conn = useLiveEvents(() => {
    invalidateWorkspaceData(queryClient);
  });

  const route = useRoute();
  const navigate = useNavigate();
  const [paletteOpen, setPaletteOpen] = useState(false);

  // -- the sidebar --------------------------------------------------------
  // ⌘B hides it and ⌘B brings it back; the pointer never does either, so
  // the shortcut is a decision, not something a sweep across the screen
  // undoes. The width is remembered per browser.
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>("expanded");
  const [sidebarWidth, setSidebarWidth] = useState(loadSidebarWidth);

  const toggleSidebar = useCallback(() => {
    setSidebarMode((mode) => (mode === "hidden" ? "expanded" : "hidden"));
  }, []);

  const onResizeEnd = useCallback((width: number) => {
    setSidebarWidth(width);
    try {
      window.localStorage.setItem(SIDEBAR_WIDTH_KEY, String(width));
    } catch {
      // A blocked or full localStorage only loses the remembered width.
    }
  }, []);

  // -- the issue panel ------------------------------------------------------
  // An issue opens beside the list, not instead of it: the route keeps the
  // list (and its filter) and gains `?issue=`. The full page is a deliberate
  // second step — ⌘↵ or the expand button.
  const peekId = peekOf(route);
  // Subscribing to the on-screen order is what keeps the panel's next/prev
  // buttons honest when a filter changes underneath it.
  const peekList = usePeekList();
  const peekIndex = peekId === null ? -1 : peekList.indexOf(peekId);

  const openIssue = useCallback(
    (id: string) => navigate(withPeek(route, id)),
    [navigate, route],
  );
  const closePeek = useCallback(() => navigate(withPeek(route, null)), [navigate, route]);
  const expandPeek = useCallback(
    (id: string) => navigate({ name: "issue", id, from: peekHost(route) }),
    [navigate, route],
  );
  const stepPeekTo = useCallback(
    (delta: -1 | 1) => {
      const next = stepPeek(peekId, delta);
      if (next !== null && next !== peekId) navigate(withPeek(route, next));
    },
    [navigate, peekId, route],
  );
  // New issues are a page, not a dialog: the composer looks like the detail
  // view it becomes, editor ready, nothing committed until it is created.
  const openNewIssue = useCallback(() => navigate({ name: "new-issue" }), [navigate]);
  const openSearch = useCallback((q: string) => navigate({ name: "search", q }), [navigate]);
  const selectDoc = useCallback(
    (p: string | null) => navigate({ name: "docs", p }),
    [navigate],
  );
  // Filtering from the sidebar stays inside the list you are on: the
  // shortlist narrowed by a context is still the shortlist.
  const filterIssues = useCallback(
    (q: string | null) =>
      navigate({
        name: "issues",
        q,
        starred: route.name === "issues" ? route.starred : undefined,
      }),
    [navigate, route],
  );

  // -- keyboard -------------------------------------------------------------
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        // Bare-letter shortcuts, only while nobody is typing: `c` creates,
        // and J/K walk the list whenever a panel is open over it. Escape
        // closes the panel from anywhere it is not busy abandoning an edit.
        if (event.altKey || event.shiftKey || isTypingTarget(event.target)) return;
        if (event.key === "c") {
          event.preventDefault();
          openNewIssue();
        } else if (event.key === "Escape" && peekId !== null) {
          event.preventDefault();
          closePeek();
        } else if ((event.key === "j" || event.key === "k") && peekId !== null) {
          event.preventDefault();
          stepPeekTo(event.key === "j" ? 1 : -1);
        }
        return;
      }
      if (event.altKey) return;
      // ⌘↵ promotes the open panel to the full page — the one place the
      // page is reached without a click.
      if (event.key === "Enter" && peekId !== null) {
        event.preventDefault();
        expandPeek(peekId);
        return;
      }
      const key = event.key.toLowerCase();
      if (key === "k" || key === "p") {
        // ⌘K is the palette; ⌘P rides along as the quick-open reflex every
        // editor teaches — same palette, pages among the results.
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (key === "b") {
        event.preventDefault();
        toggleSidebar();
      } else if (key === ",") {
        event.preventDefault();
        navigate({ name: "settings" });
      } else if (key >= "1" && key <= "8") {
        const target = SHORTCUT_VIEWS[Number(key) - 1];
        if (target) {
          event.preventDefault();
          navigate(target);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [closePeek, expandPeek, navigate, openNewIssue, peekId, stepPeekTo, toggleSidebar]);

  // `dit serve` opens the browser without a fragment; default to Home so
  // the address bar always reflects where you are.
  useEffect(() => {
    if (window.location.hash === "") navigate({ name: "home" });
  }, [navigate]);

  // -- docs tabs -------------------------------------------------------------
  // Tab list, pins and per-path drafts live here (not in DocsView) so they
  // outlive navigation to another view; the URL's `p` is the active tab.
  const docsTabs = useDocTabs();
  // The union narrowed once, so every callback below can read it plainly.
  const docsP = route.name === "docs" ? route.p : null;

  // A deep link or reload names an active page no tab remembers — give it
  // one. Idempotent, so re-running on any render is harmless.
  useEffect(() => {
    if (docsP !== null) docsTabs.ensure(docsP);
  }, [docsP, docsTabs]);

  const previewDoc = useCallback(
    (path: string) => {
      docsTabs.preview(path);
      selectDoc(path);
    },
    [docsTabs, selectDoc],
  );

  const pinDoc = useCallback(
    (path: string) => {
      docsTabs.pin(path);
      selectDoc(path);
    },
    [docsTabs, selectDoc],
  );

  // A rename or drag-move landed: the tab, its pin and its draft follow the
  // new path; if the moved page was active, the URL follows too.
  const movedDoc = useCallback(
    (from: string, to: string) => {
      const wasActive = docsP === from;
      docsTabs.rekey(from, to);
      if (wasActive) selectDoc(to);
    },
    [docsTabs, docsP, selectDoc],
  );

  /** Close a tab; when it was active, fall through to its right-hand
   *  neighbor (leftmost when it was last). `force` skips the unsaved-changes
   *  question — used after the page itself was deleted. */
  const closeDocTab = useCallback(
    (path: string, opts?: { force?: boolean }) => {
      if (!opts?.force && docsTabs.isDirty(path)) {
        const confirmed = window.confirm(
          `${path} has unsaved changes.\n\nClose the tab anyway? The changes are not recoverable.`,
        );
        if (!confirmed) return;
      }
      const index = docsTabs.paths.indexOf(path);
      const remaining = docsTabs.paths.filter((tab) => tab !== path);
      docsTabs.close(path);
      if (docsP === path) {
        selectDoc(remaining[Math.min(Math.max(index, 0), remaining.length - 1)] ?? null);
      }
    },
    [docsTabs, docsP, selectDoc],
  );

  // The sidebar's lower section is a pure function of the route: each view
  // names its own secondary surface. The section fetches its own data
  // (shared TanStack cache keys keep it in agreement with the main view),
  // so no state is drilled down from here.
  let section: { title: string; node: ReactNode } | null = null;
  if (route.name === "home") {
    section = { title: "Home", node: <HomePane onOpen={openIssue} /> };
  } else if (route.name === "search") {
    section = { title: "Search", node: <SearchPane q={route.q} onSearch={openSearch} /> };
  } else if (route.name === "board") {
    section = { title: "Board", node: <BoardPane /> };
  } else if (route.name === "issues" || route.name === "issue" || route.name === "new-issue") {
    // An open issue keeps the filters: the list is one back away. The
    // composer does too — it is one back away from the same list.
    const q = route.name === "issues" ? route.q : null;
    section = { title: "Issues", node: <IssuesPane q={q} onFilter={filterIssues} /> };
  } else if (route.name === "docs") {
    section = {
      title: "Docs",
      node: (
        <DocsPane
          p={route.p}
          onSelect={previewDoc}
          onOpen={pinDoc}
          onMoved={movedDoc}
          onDeleted={(path) => closeDocTab(path, { force: true })}
          isDirty={docsTabs.isDirty}
        />
      ),
    };
  } else if (route.name === "timeline") {
    section = { title: "Timeline", node: <TimelinePane seq={route.seq ?? null} /> };
  } else if (route.name === "roadmap") {
    section = { title: "Roadmap", node: <RoadmapPane /> };
  } else if (route.name === "gantt") {
    section = { title: "Gantt", node: <GanttPane /> };
  } else if (route.name === "settings") {
    section = { title: "Settings", node: <SettingsPane /> };
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-app text-ink">
      <div className="flex min-h-0 flex-1">
        {/* The provider spans both sidebar and main: hidden board columns
            are view state the two surfaces share, not a property of either. */}
        <BoardColumnsProvider>
         <GanttOptionsProvider>
          <RoadmapOptionsProvider>
          <Sidebar
            route={route}
            mode={sidebarMode}
            width={sidebarWidth}
            section={section}
            onNavigate={navigate}
            onNewIssue={openNewIssue}
            onOpenPalette={() => setPaletteOpen(true)}
            onToggle={toggleSidebar}
            onResizeStart={() => undefined}
            onResize={setSidebarWidth}
            onResizeEnd={onResizeEnd}
          />
          {/* `relative` so the issue panel can sit over the view without
              taking the list off screen. */}
          <main className="relative flex min-w-0 flex-1 flex-col">
            {route.name === "home" ? (
              <HomeView conn={conn} onOpen={openIssue} onSearch={openSearch} />
            ) : null}
            {route.name === "board" ? <BoardView onOpen={openIssue} /> : null}
            {route.name === "issues" ? (
              <IssuesView q={route.q} starred={route.starred === true} onOpen={openIssue} />
            ) : null}
            {route.name === "docs" ? (
              <DocsView
                p={route.p}
                onSelect={selectDoc}
                tabs={docsTabs}
                onCloseTab={closeDocTab}
              />
            ) : null}
            {route.name === "search" ? <SearchView q={route.q} onOpen={openIssue} /> : null}
            {route.name === "timeline" ? (
              <TimelineView
                seq={route.seq ?? null}
                onOpen={openIssue}
                onSeek={(seq) => navigate({ name: "timeline", seq, issue: route.issue })}
              />
            ) : null}
            {route.name === "roadmap" ? <RoadmapView onOpen={openIssue} /> : null}
            {route.name === "gantt" ? <GanttView onOpen={openIssue} /> : null}
            {route.name === "issue" ? (
              <IssueDetailView
                id={route.id}
                from={peekHost(route)}
                onCollapse={() => navigate(withPeek(route, route.id))}
              />
            ) : null}
            {/* A created issue opens as the page the composer just became —
                the panel is for triaging a list, not for filling one in. */}
            {route.name === "new-issue" ? (
              <NewIssueView onCreated={(id) => navigate({ name: "issue", id, from: "issues" })} />
            ) : null}
            {route.name === "settings" ? <SettingsView /> : null}

            {peekId !== null ? (
              <IssuePeek
                key={peekId}
                id={peekId}
                route={route}
                onClose={closePeek}
                onExpand={() => expandPeek(peekId)}
                onStep={stepPeekTo}
                canStepBack={peekIndex > 0}
                canStepForward={peekIndex >= 0 && peekIndex < peekList.length - 1}
              />
            ) : null}
          </main>
          </RoadmapOptionsProvider>
         </GanttOptionsProvider>
        </BoardColumnsProvider>
      </div>
      <StatusBar conn={conn} />
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onNavigate={navigate}
        onOpenIssue={openIssue}
        onNewIssue={openNewIssue}
        onOpenDoc={previewDoc}
      />
    </div>
  );
}
