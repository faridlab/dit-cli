// Layout skeleton: sidebar, view area, status bar, plus the two global
// overlays (command palette, new-issue dialog). Route state and the ⌘K
// listener live here so the views below stay purely about data.

import { useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useLiveEvents } from "../lib/events";
import { invalidateWorkspaceData } from "../lib/queries";
import { useNavigate, useRoute } from "../lib/router";
import { CommandPalette } from "./CommandPalette";
import { NewIssueDialog } from "./NewIssueDialog";
import { Sidebar } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { BoardView } from "../views/BoardView";
import { HomeView } from "../views/HomeView";
import { IssueDetailView } from "../views/IssueDetailView";
import { IssuesView } from "../views/IssuesView";
import { SearchView } from "../views/SearchView";
import { SettingsView } from "../views/SettingsView";

export function AppShell() {
  const queryClient = useQueryClient();
  // The watcher saw a new commit: everything it can have changed goes stale.
  const conn = useLiveEvents(() => {
    invalidateWorkspaceData(queryClient);
  });

  const route = useRoute();
  const navigate = useNavigate();
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [newIssueOpen, setNewIssueOpen] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // `dit serve` opens the browser without a fragment; default to Home so
  // the address bar always reflects where you are.
  useEffect(() => {
    if (window.location.hash === "") navigate({ name: "home" });
  }, [navigate]);

  const openIssue = useCallback((id: string) => navigate({ name: "issue", id }), [navigate]);
  const openNewIssue = useCallback(() => setNewIssueOpen(true), []);
  const openSearch = useCallback((q: string) => navigate({ name: "search", q }), [navigate]);

  return (
    <div className="flex h-dvh flex-col overflow-hidden bg-app text-zinc-200">
      <div className="flex min-h-0 flex-1">
        <Sidebar
          route={route}
          onNavigate={navigate}
          onNewIssue={openNewIssue}
          onOpenPalette={() => setPaletteOpen(true)}
        />
        <main className="flex min-w-0 flex-1 flex-col">
          {route.name === "home" ? (
            <HomeView conn={conn} onOpen={openIssue} onSearch={openSearch} />
          ) : null}
          {route.name === "board" ? <BoardView onOpen={openIssue} /> : null}
          {route.name === "issues" ? <IssuesView onOpen={openIssue} /> : null}
          {route.name === "search" ? (
            <SearchView
              q={route.q}
              onSearch={(q) => navigate({ name: "search", q })}
              onOpen={openIssue}
            />
          ) : null}
          {route.name === "issue" ? <IssueDetailView id={route.id} /> : null}
          {route.name === "settings" ? <SettingsView /> : null}
        </main>
      </div>
      <StatusBar conn={conn} />
      <CommandPalette
        open={paletteOpen}
        onOpenChange={setPaletteOpen}
        onNavigate={navigate}
        onNewIssue={openNewIssue}
      />
      <NewIssueDialog
        open={newIssueOpen}
        onOpenChange={setNewIssueOpen}
        onCreated={(issue) => openIssue(issue.short_ref)}
      />
    </div>
  );
}
