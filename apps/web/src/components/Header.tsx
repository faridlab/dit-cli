// The 48px bar above every view: sidebar toggle, breadcrumbs that say where
// you are, the view's own actions (Filter, Display, Sort, Source…), the one
// primary action (New issue), then theme and notes. Views do not draw their
// own title bars — this is the only header, so every screen starts the
// same way.

import type { ReactNode } from "react";
import { ArrowLeft, ChevronRight, Info, Moon, PanelLeft, Plus, Sun } from "lucide-react";
import type { Route } from "../lib/router";
import { useTheme } from "../lib/theme";
import { Btn, IBtn, Kbd } from "./chrome";

function titleCase(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1);
}

/** Breadcrumb parts for a route: the workspace, then the view (and the
 *  Plan group for the three plan views), with the last part bold. */
export function crumbsFor(route: Route, workspace: string, extra?: string | null): string[] {
  switch (route.name) {
    case "home":
      return [workspace, "Home"];
    case "board":
      return [workspace, "Board"];
    case "issues":
      return [workspace, route.inbox ? "Inbox" : route.starred ? "Starred" : "Issues"];
    case "docs":
      return extra ? [workspace, "Docs", extra] : [workspace, "Docs"];
    case "search":
      return [workspace, "Search"];
    case "timeline":
      return [workspace, "Plan", "Timeline"];
    case "roadmap":
      return [workspace, "Plan", "Roadmap"];
    case "gantt":
      return [workspace, "Plan", "Gantt"];
    case "workflow":
      return [workspace, "Workflow"];
    case "settings":
      return [workspace, "Settings"];
    case "new-issue":
      return [workspace, "Issues", "New issue"];
    case "issue":
      return [workspace, titleCase(route.from ?? "issues"), extra ?? "—"];
  }
}

export function Crumbs({ parts }: { parts: string[] }) {
  return (
    <div className="crumbs">
      {parts.map((part, index) => (
        <span key={`${index}-${part}`} className="flex items-center gap-1">
          {index > 0 ? <ChevronRight className="i" aria-hidden /> : null}
          {index === parts.length - 1 ? <b>{part}</b> : <span>{part}</span>}
        </span>
      ))}
    </div>
  );
}

export function Header({
  crumbs,
  back,
  right,
  onToggleSidebar,
  onNewIssue,
  onNotes,
  notesOpen,
}: {
  crumbs: string[];
  /** Where the back arrow goes, when the view has one (the full issue page). */
  back?: { title: string; onClick: () => void } | null;
  /** The view's own actions, right of the breadcrumbs. */
  right?: ReactNode;
  onToggleSidebar: () => void;
  onNewIssue: () => void;
  onNotes: () => void;
  notesOpen: boolean;
}) {
  const theme = useTheme();
  const dark = theme.resolved === "dark";
  return (
    <header className="hdr">
      <IBtn onClick={onToggleSidebar} title="Toggle sidebar (⌘B)" aria-label="Toggle sidebar">
        <PanelLeft className="i" aria-hidden />
      </IBtn>
      {back ? (
        <IBtn onClick={back.onClick} title={back.title} aria-label={back.title}>
          <ArrowLeft className="i" aria-hidden />
        </IBtn>
      ) : null}
      <Crumbs parts={crumbs} />
      <span className="sp" />
      {right}
      <Btn primary onClick={onNewIssue} title="New issue (C)">
        <Plus className="i" aria-hidden />
        New issue
        <Kbd>C</Kbd>
      </Btn>
      <span className="mx-1 h-[18px] w-px bg-edge" aria-hidden />
      <IBtn
        onClick={() => theme.setPreference(dark ? "light" : "dark")}
        title={`Theme: ${theme.preference} — click for ${dark ? "light" : "dark"}`}
        aria-label="Switch theme"
      >
        {dark ? <Moon className="i" aria-hidden /> : <Sun className="i" aria-hidden />}
      </IBtn>
      <IBtn on={notesOpen} onClick={onNotes} title="Notes: how this workbench works, and the keys" aria-label="Notes">
        <Info className="i" aria-hidden />
      </IBtn>
    </header>
  );
}
