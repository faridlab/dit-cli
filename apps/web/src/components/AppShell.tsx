// Workbench layout: sidebar, main area (header + content + issue panel),
// status bar, plus the global overlays (palette, notes). Route state, the
// keyboard listeners and the shared view options live here so the views
// below stay purely about data.

import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Code, Columns3, PanelRight, Settings, Terminal } from "lucide-react";
import { toast } from "sonner";
import { useLiveEvents } from "../lib/events";
import { useDocTabs } from "../lib/doctabs";
import { invalidateWorkspaceData, useIssue, useStatus } from "../lib/queries";
import { parseHash, peekHost, peekOf, useNavigate, useRoute, withPeek, type Route } from "../lib/router";
import { stepPeek, usePeekList } from "../lib/peeklist";
import { useTheme } from "../lib/theme";
import { ViewOptionsProvider, useViewOptions } from "../lib/viewopts";
import { shortSha } from "../lib/format";
import { CommandPalette } from "./CommandPalette";
import { IssuePeek } from "./issue/IssuePeek";
import { Sidebar, SHORTCUT_VIEWS, type SidebarMode } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { Header, crumbsFor } from "./Header";
import { DisplayButton, FilterButton, HeaderHint, SortButton } from "./HeaderMenus";
import { NotesDrawer } from "./NotesDrawer";
import { Btn, type MenuItem } from "./chrome";
import { GanttOptionsProvider, GanttPane } from "./panes/GanttPane";
import { RoadmapOptionsProvider, RoadmapPane } from "./panes/RoadmapPane";
import { TimelinePane } from "./panes/TimelinePane";
import { BoardPane } from "./panes/BoardPane";
import { DocsPane } from "./panes/DocsPane";
import { FlowPane } from "./panes/FlowPane";
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
import { FlowView } from "../views/FlowView";

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  return /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
}

async function copyText(text: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${label} — ${text}`);
  } catch {
    toast(`${label}: ${text}`);
  }
}

/** The CLI spelling of the current screen, for the palette and the menus. */
function cliFor(route: Route): string {
  switch (route.name) {
    case "board":
      return "dit board";
    case "issues":
      return route.q ? `dit ls "${route.q}"` : "dit ls";
    case "search":
      return route.q ? `dit ls "${route.q}"` : "dit ls";
    case "issue":
      return `dit show ${route.id}`;
    case "docs":
      return route.p ? `dit doc show ${route.p}` : "dit doc ls";
    default:
      return "dit ui";
  }
}

export function AppShell() {
  return (
    <ViewOptionsProvider>
      <GanttOptionsProvider>
        <RoadmapOptionsProvider>
          <Shell />
        </RoadmapOptionsProvider>
      </GanttOptionsProvider>
    </ViewOptionsProvider>
  );
}

function Shell() {
  const queryClient = useQueryClient();
  // The watcher saw a new commit: everything it can have changed goes stale.
  const conn = useLiveEvents(() => {
    invalidateWorkspaceData(queryClient);
  });

  const route = useRoute();
  const navigate = useNavigate();
  const status = useStatus();
  const theme = useTheme();
  const options = useViewOptions();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [notesOpen, setNotesOpen] = useState(false);
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>("expanded");
  const toggleSidebar = useCallback(() => {
    setSidebarMode((mode) => (mode === "hidden" ? "expanded" : "hidden"));
  }, []);

  const workspace = status.data
    ? (status.data.repo.split("/").filter(Boolean).pop() ?? status.data.repo)
    : "…";

  // -- the issue panel ------------------------------------------------------
  // An issue opens beside the list, not instead of it: the route keeps the
  // list (and its filter) and gains `?issue=`. The full page is a deliberate
  // second step — ⌘↵ or the expand button — unless Settings say otherwise.
  const peekId = peekOf(route);
  const peekList = usePeekList();
  const peekIndex = peekId === null ? -1 : peekList.indexOf(peekId);

  const expandPeek = useCallback(
    (id: string) => navigate({ name: "issue", id, from: peekHost(route) }),
    [navigate, route],
  );
  const openIssue = useCallback(
    (id: string) => {
      if (options.openAs === "page") expandPeek(id);
      else navigate(withPeek(route, id));
    },
    [expandPeek, navigate, options.openAs, route],
  );
  const closePeek = useCallback(() => navigate(withPeek(route, null)), [navigate, route]);
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
  const selectDoc = useCallback((p: string | null) => navigate({ name: "docs", p }), [navigate]);
  // Filtering from the sidebar stays inside the list you are on.
  const filterIssues = useCallback(
    (q: string | null) =>
      navigate({
        name: "issues",
        q,
        starred: route.name === "issues" ? route.starred : undefined,
        inbox: route.name === "issues" ? route.inbox : undefined,
      }),
    [navigate, route],
  );
  const go = useCallback((hash: string) => navigate(parseHash(hash)), [navigate]);

  // -- keyboard -------------------------------------------------------------
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        if (event.key === "Escape") {
          // Innermost thing first. A menu or the palette that just dismissed
          // itself marks the event handled, and the panel behind it stays.
          if (event.defaultPrevented) return;
          if (notesOpen) {
            event.preventDefault();
            setNotesOpen(false);
            return;
          }
          if (isTypingTarget(event.target)) return;
          if (peekId !== null) {
            event.preventDefault();
            closePeek();
          }
          return;
        }
        // Bare-letter shortcuts, only while nobody is typing: `c` creates,
        // `/` opens the palette, J/K walk the list behind an open panel.
        if (event.altKey || event.shiftKey || isTypingTarget(event.target)) return;
        if (event.key === "c") {
          event.preventDefault();
          openNewIssue();
        } else if (event.key === "/") {
          event.preventDefault();
          setPaletteOpen(true);
        } else if ((event.key === "j" || event.key === "k") && peekList.length > 0) {
          event.preventDefault();
          if (peekId === null) {
            const first = peekList[0];
            if (first) navigate(withPeek(route, first));
          } else {
            stepPeekTo(event.key === "j" ? 1 : -1);
          }
        }
        return;
      }
      if (event.altKey) return;
      // ⌘↵ promotes the open panel to the full page — the one place the
      // page is reached without a click. The comment composer keeps it.
      if (event.key === "Enter" && peekId !== null && !isTypingTarget(event.target)) {
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
  }, [closePeek, expandPeek, navigate, notesOpen, openNewIssue, peekId, peekList, route, stepPeekTo, toggleSidebar]);

  // `dit serve` opens the browser without a fragment; default to Home so
  // the address bar always reflects where you are.
  useEffect(() => {
    if (window.location.hash === "") navigate({ name: "home" });
  }, [navigate]);

  // -- docs tabs -------------------------------------------------------------
  // Tab list, pins and per-path drafts live here (not in DocsView) so they
  // outlive navigation to another view; the URL's `p` is the active tab.
  const docsTabs = useDocTabs();
  const docsP = route.name === "docs" ? route.p : null;

  // `useDocTabs` hands back a fresh object every render, so this effect must
  // key on the path alone — otherwise it re-runs constantly and can re-add a
  // tab that a close or a rename removed a moment ago.
  const ensureTab = useRef(docsTabs.ensure);
  ensureTab.current = docsTabs.ensure;
  useEffect(() => {
    if (docsP !== null) ensureTab.current(docsP);
  }, [docsP]);

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
  const movedDoc = useCallback(
    (from: string, to: string) => {
      const wasActive = docsP === from;
      docsTabs.rekey(from, to);
      if (wasActive) selectDoc(to);
    },
    [docsTabs, docsP, selectDoc],
  );
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

  // -- the workspace menu ----------------------------------------------------
  const workspaceMenu = useMemo<MenuItem[]>(
    () => [
      { kind: "head", label: "Workspace" },
      {
        kind: "text",
        node: (
          <>
            <span className="mono">{status.data?.repo ?? "…"}</span>
            <br />
            {status.data ? `${status.data.branch} @ ${shortSha(status.data.head)}` : ""}
          </>
        ),
      },
      { label: "Open board", icon: <Columns3 className="i" aria-hidden />, run: () => navigate({ name: "board" }) },
      {
        label: "Copy CLI command for this view",
        icon: <Terminal className="i" aria-hidden />,
        run: () => void copyText(cliFor(route), "Command copied"),
      },
      { kind: "sep" },
      { label: "Settings", icon: <Settings className="i" aria-hidden />, kbd: "⌘,", run: () => navigate({ name: "settings" }) },
    ],
    [navigate, route, status.data],
  );

  // -- the sidebar section and the header actions, both a function of the route
  let section: { title: string; node: ReactNode } | null = null;
  let right: ReactNode = null;
  let extraCrumb: string | null = null;
  let back: { title: string; onClick: () => void } | null = null;

  if (route.name === "home") {
    section = { title: "Home", node: <HomePane onOpen={openIssue} /> };
  } else if (route.name === "search") {
    section = { title: "Search", node: <SearchPane q={route.q} onSearch={openSearch} /> };
  } else if (route.name === "board") {
    section = { title: "Board", node: <BoardPane /> };
    right = (
      <>
        <FilterButton />
        <DisplayButton />
      </>
    );
  } else if (route.name === "issues" || route.name === "issue" || route.name === "new-issue") {
    const q = route.name === "issues" ? route.q : null;
    section = { title: "Issues", node: <IssuesPane q={q} onFilter={filterIssues} /> };
    if (route.name === "issues") {
      right = (
        <>
          <FilterButton />
          <SortButton />
        </>
      );
    }
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
    extraCrumb = route.p ? (route.p.split("/").pop() ?? route.p) : null;
    const dirty = route.p ? docsTabs.isDirty(route.p) : false;
    right = route.p ? (
      <>
        <Btn
          primary={options.docSource}
          onClick={() => options.setDocSource(!options.docSource)}
          title="Toggle source mode (markdown as dit fmt writes it)"
        >
          <Code className="i" aria-hidden />
          Source
        </Btn>
        <HeaderHint>
          {dirty ? "unsaved · autosave pending" : `saved · ${status.data ? shortSha(status.data.head) : "…"}`}
        </HeaderHint>
      </>
    ) : null;
  } else if (route.name === "timeline") {
    section = { title: "Timeline", node: <TimelinePane seq={route.seq ?? null} /> };
    right = <HeaderHint>the history layer · read from git, never stored</HeaderHint>;
  } else if (route.name === "roadmap") {
    section = { title: "Roadmap", node: <RoadmapPane /> };
    right = <HeaderHint>epics and releases · spans without dates are derived from children</HeaderHint>;
  } else if (route.name === "gantt") {
    section = { title: "Gantt", node: <GanttPane /> };
    right = (
      <>
        <FilterButton />
        <HeaderHint>drag a bar to move · edges change start / due · ← → keys nudge</HeaderHint>
      </>
    );
  } else if (route.name === "flow") {
    section = { title: "Flow", node: <FlowPane onOpen={openIssue} /> };
    right = (
      <HeaderHint>
        click a node to trace it · f finds · 0 fits · [ ] walk the critical path
      </HeaderHint>
    );
  } else if (route.name === "settings") {
    section = { title: "Settings", node: <SettingsPane /> };
  }

  if (route.name === "issue") {
    const host = peekHost(route);
    back = {
      title: `Back to ${host} (keeps this issue open beside it)`,
      onClick: () => navigate(withPeek(route, route.id)),
    };
    right = (
      <Btn onClick={() => navigate(withPeek(route, route.id))} title="Collapse to the side panel">
        <PanelRight className="i" aria-hidden />
        Show as panel
      </Btn>
    );
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-app text-ink">
      <div className="flex min-h-0 flex-1">
        <Sidebar
          route={route}
          mode={sidebarMode}
          section={section}
          onNavigate={navigate}
          onNewIssue={openNewIssue}
          onOpenPalette={() => setPaletteOpen(true)}
          workspaceMenu={workspaceMenu}
        />
        {/* `.main` is relative so the issue panel can sit over the view
            without taking the list off screen. */}
        <main className="main flex-1">
          {route.name === "issue" ? (
            <IssueHeader
              id={route.id}
              crumbs={(handle) => crumbsFor(route, workspace, handle)}
              back={back}
              right={right}
              onToggleSidebar={toggleSidebar}
              onNewIssue={openNewIssue}
              onNotes={() => setNotesOpen((open) => !open)}
              notesOpen={notesOpen}
            />
          ) : (
            <Header
              crumbs={crumbsFor(route, workspace, extraCrumb)}
              back={back}
              right={right}
              onToggleSidebar={toggleSidebar}
              onNewIssue={openNewIssue}
              onNotes={() => setNotesOpen((open) => !open)}
              notesOpen={notesOpen}
            />
          )}
          <div className="content">
            {route.name === "home" ? <HomeView conn={conn} onOpen={openIssue} onSearch={openSearch} /> : null}
            {route.name === "board" ? <BoardView onOpen={openIssue} /> : null}
            {route.name === "issues" ? (
              <IssuesView q={route.q} starred={route.starred === true} inbox={route.inbox === true} onOpen={openIssue} />
            ) : null}
            {route.name === "docs" ? (
              <DocsView p={route.p} onSelect={selectDoc} tabs={docsTabs} onCloseTab={closeDocTab} />
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
            {route.name === "flow" ? <FlowView onOpen={openIssue} /> : null}
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
          </div>

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
      </div>
      <StatusBar
        conn={conn}
        workspaceMenu={workspaceMenu}
        onOpenSettings={() => navigate({ name: "settings" })}
        onNotes={() => setNotesOpen((open) => !open)}
      />
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onNavigate={navigate}
        onOpenIssue={openIssue}
        onNewIssue={openNewIssue}
        onOpenDoc={previewDoc}
        onToggleSidebar={toggleSidebar}
        sidebarHidden={sidebarMode === "hidden"}
        onNotes={() => setNotesOpen(true)}
        cli={cliFor(route)}
      />
      <NotesDrawer
        open={notesOpen}
        onClose={() => setNotesOpen(false)}
        onOpenPalette={() => setPaletteOpen(true)}
        onSwitchTheme={() => theme.setPreference(theme.resolved === "dark" ? "light" : "dark")}
        onGo={go}
      />
    </div>
  );
}

/** The full page's header needs the issue's handle for its last crumb. */
function IssueHeader({
  id,
  crumbs,
  ...rest
}: Omit<React.ComponentProps<typeof Header>, "crumbs"> & {
  id: string;
  crumbs: (handle: string) => string[];
}) {
  const issue = useIssue(id);
  const handle = issue.data ? (issue.data.number !== null ? `#${issue.data.number}` : issue.data.short_ref) : id;
  return <Header crumbs={crumbs(handle)} {...rest} />;
}

