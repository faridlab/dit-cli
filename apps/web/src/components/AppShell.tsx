// Workbench layout (the VS Code shape): activity bar, side pane, main
// area, status bar, plus the two global overlays. Route state, the
// ⌘K / ⌘B / ⌘1-⌘5 listeners, and the pane expand/slim state machine live
// here so the views below stay purely about data.

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Columns3, FileText, House, ListTodo, Search, Settings } from "lucide-react";
import { useLiveEvents } from "../lib/events";
import { useDocTabs } from "../lib/doctabs";
import { invalidateWorkspaceData } from "../lib/queries";
import { useNavigate, useRoute, type Route } from "../lib/router";
import { ActivityBar } from "./ActivityBar";
import { CommandPalette } from "./CommandPalette";
import { PaneFrame, PANE_MAX_WIDTH, PANE_MIN_WIDTH, type PaneMode } from "./PaneFrame";
import { StatusBar } from "./StatusBar";
import { BoardColumnsProvider, BoardPane } from "./panes/BoardPane";
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

// The rail order (Home, Search, Board, Issues, Docs) is also the ⌘1..⌘5
// shortcut order; the activity bar and this table must agree.
const SHORTCUT_VIEWS: Route[] = [
  { name: "home" },
  { name: "search", q: "" },
  { name: "board" },
  { name: "issues", q: null },
  { name: "docs", p: null },
];

const PANE_WIDTH_KEY = "dit.pane.width";
const DEFAULT_PANE_WIDTH = 268;

function loadPaneWidth(): number {
  try {
    const raw = window.localStorage.getItem(PANE_WIDTH_KEY);
    const parsed = raw === null ? Number.NaN : Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) return DEFAULT_PANE_WIDTH;
    return Math.min(PANE_MAX_WIDTH, Math.max(PANE_MIN_WIDTH, parsed));
  } catch {
    return DEFAULT_PANE_WIDTH;
  }
}

// How long the pointer (or focus) must stay in the main area before the
// pane gives up its width. Long enough that a sweep across the screen does
// not collapse it, short enough to feel like it notices you working.
const SLIM_DELAY_MS = 450;

export function AppShell() {
  const queryClient = useQueryClient();
  // The watcher saw a new commit: everything it can have changed goes stale.
  const conn = useLiveEvents(() => {
    invalidateWorkspaceData(queryClient);
  });

  const route = useRoute();
  const navigate = useNavigate();
  const [paletteOpen, setPaletteOpen] = useState(false);

  // -- the pane state machine ---------------------------------------------
  // expanded -> slim: automatically, shortly after the pointer or focus
  //   lands in the main area — the content you are working in gets the
  //   width, the pane stays a labeled strip rather than vanishing.
  // slim -> expanded: hover, focus or click on the strip.
  // expanded <-> hidden: ⌘B. A hidden pane stays hidden; only ⌘B restores
  //   it, so the shortcut is a decision, not something the pointer undoes.
  const [paneMode, setPaneMode] = useState<PaneMode>("expanded");
  const [paneWidth, setPaneWidth] = useState(loadPaneWidth);
  // True while the pointer is over the pane or its resize handle — the
  // auto-slim timer must never fire against a pane in use. Kept in a ref
  // (not state) so reading it does not re-render on every pointer move.
  const paneHover = useRef(false);
  const slimTimer = useRef<number | null>(null);

  const cancelSlim = useCallback(() => {
    if (slimTimer.current !== null) {
      window.clearTimeout(slimTimer.current);
      slimTimer.current = null;
    }
  }, []);

  const paneInteract = useCallback(() => {
    paneHover.current = true;
    cancelSlim();
    setPaneMode((mode) => (mode === "slim" ? "expanded" : mode));
  }, [cancelSlim]);

  const paneLeave = useCallback(() => {
    paneHover.current = false;
  }, []);

  const scheduleSlim = useCallback(() => {
    if (paneHover.current) return;
    cancelSlim();
    slimTimer.current = window.setTimeout(() => {
      slimTimer.current = null;
      setPaneMode((mode) => (mode === "expanded" ? "slim" : mode));
    }, SLIM_DELAY_MS);
  }, [cancelSlim]);

  const togglePane = useCallback(() => {
    cancelSlim();
    setPaneMode((mode) => (mode === "hidden" ? "expanded" : "hidden"));
  }, [cancelSlim]);

  const persistPaneWidth = useCallback((width: number) => {
    try {
      window.localStorage.setItem(PANE_WIDTH_KEY, String(width));
    } catch {
      // A blocked or full localStorage only loses the remembered width.
    }
  }, []);

  // Dragging the resize handle moves the pointer across the main area,
  // which would otherwise slim the pane mid-drag; the handle claims the
  // hover flag for the length of the gesture.
  const onResizeStart = useCallback(() => {
    paneHover.current = true;
    cancelSlim();
  }, [cancelSlim]);

  const onResizeEnd = useCallback(
    (width: number) => {
      paneHover.current = false;
      setPaneWidth(width);
      persistPaneWidth(width);
    },
    [persistPaneWidth],
  );

  useEffect(() => cancelSlim, [cancelSlim]);

  // -- keyboard -------------------------------------------------------------
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "k" || key === "p") {
        // ⌘K is the palette; ⌘P rides along as the quick-open reflex every
        // editor teaches — same palette, pages among the results.
        event.preventDefault();
        setPaletteOpen((open) => !open);
      } else if (key === "b") {
        event.preventDefault();
        togglePane();
      } else if (key === ",") {
        event.preventDefault();
        navigate({ name: "settings" });
      } else if (key >= "1" && key <= "5") {
        const target = SHORTCUT_VIEWS[Number(key) - 1];
        if (target) {
          event.preventDefault();
          navigate(target);
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [navigate, togglePane]);

  // `dit serve` opens the browser without a fragment; default to Home so
  // the address bar always reflects where you are.
  useEffect(() => {
    if (window.location.hash === "") navigate({ name: "home" });
  }, [navigate]);

  const openIssue = useCallback((id: string) => navigate({ name: "issue", id }), [navigate]);
  // New issues are a page, not a dialog: the composer looks like the detail
  // view it becomes, editor ready, nothing committed until it is created.
  const openNewIssue = useCallback(() => navigate({ name: "new-issue" }), [navigate]);
  const openSearch = useCallback((q: string) => navigate({ name: "search", q }), [navigate]);
  const selectDoc = useCallback(
    (p: string | null) => navigate({ name: "docs", p }),
    [navigate],
  );
  const filterIssues = useCallback(
    (q: string | null) => navigate({ name: "issues", q }),
    [navigate],
  );

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

  // The pane is a pure function of the route: each view names its own
  // secondary surface. The pane fetches its own data (shared TanStack cache
  // keys keep it in agreement with the main view), so no state is drilled
  // down from here.
  let pane: { title: string; icon: typeof House; node: ReactNode } | null = null;
  if (route.name === "home") {
    pane = { title: "Home", icon: House, node: <HomePane onOpen={openIssue} /> };
  } else if (route.name === "search") {
    pane = {
      title: "Search",
      icon: Search,
      node: <SearchPane q={route.q} onSearch={openSearch} />,
    };
  } else if (route.name === "board") {
    pane = { title: "Board", icon: Columns3, node: <BoardPane /> };
  } else if (route.name === "issues" || route.name === "issue" || route.name === "new-issue") {
    // An open issue keeps the filters pane: the list is one back away. The
    // composer does too — it is one back away from the same list.
    const q = route.name === "issues" ? route.q : null;
    pane = {
      title: "Issues",
      icon: ListTodo,
      node: <IssuesPane q={q} onFilter={filterIssues} />,
    };
  } else if (route.name === "docs") {
    pane = {
      title: "Docs",
      icon: FileText,
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
  } else if (route.name === "settings") {
    pane = { title: "Settings", icon: Settings, node: <SettingsPane /> };
  }

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-app text-zinc-200">
      <div className="flex min-h-0 flex-1">
        <ActivityBar route={route} onNavigate={navigate} onNewIssue={openNewIssue} />
        {/* The provider spans both pane and main: hidden board columns are
            view state the two surfaces share, not a property of either. */}
        <BoardColumnsProvider>
          {pane ? (
            <PaneFrame
              mode={paneMode}
              width={paneWidth}
              title={pane.title}
              icon={pane.icon}
              onInteract={paneInteract}
              onLeave={paneLeave}
              onCollapse={togglePane}
              onResizeStart={onResizeStart}
              onResize={setPaneWidth}
              onResizeEnd={onResizeEnd}
            >
              {pane.node}
            </PaneFrame>
          ) : null}
          <main
            onPointerEnter={scheduleSlim}
            onFocusCapture={scheduleSlim}
            className="flex min-w-0 flex-1 flex-col"
          >
            {route.name === "home" ? (
              <HomeView conn={conn} onOpen={openIssue} onSearch={openSearch} />
            ) : null}
            {route.name === "board" ? <BoardView onOpen={openIssue} /> : null}
            {route.name === "issues" ? (
              <IssuesView q={route.q} onOpen={openIssue} />
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
            {route.name === "issue" ? <IssueDetailView id={route.id} /> : null}
            {route.name === "new-issue" ? <NewIssueView onCreated={openIssue} /> : null}
            {route.name === "settings" ? <SettingsView /> : null}
          </main>
        </BoardColumnsProvider>
      </div>
      <StatusBar conn={conn} />
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onNavigate={navigate}
        onNewIssue={openNewIssue}
        onOpenDoc={previewDoc}
      />
    </div>
  );
}
