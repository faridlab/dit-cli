// The first of the workbench's three regions: one icon per kind of place.
//
// The icons stand for what the product is made of — material (Home, Docs,
// Morse), the lenses over issues (Work, Flow, Plan), the code map (Code),
// and Search — with
// Settings pinned to the bottom edge, the way VS Code keeps its gear. Names
// and counts are not lost to the icons: they live one column over, in the
// side panel's sub-menu. Here each icon carries its name as a tooltip, with
// the shortcut that reaches it.
//
// Clicking the icon that is already lit folds the side panel away and back,
// as in VS Code; the screen stays where it is.

import {
  Columns3,
  FileText,
  House,
  CalendarRange,
  Radio,
  Search,
  Settings,
  Network,
  Waypoints,
} from "lucide-react";
import { cn } from "../lib/cn";
import type { ActivityId } from "../lib/workbench";
import logo from "../assets/dit-logo.png";

type Icon = typeof House;

type Activity = { id: ActivityId; label: string; icon: Icon; kbd?: string; hint: string };

/** The shortcut shown is the one AppShell's ⌘1…⌘0 table already binds, so
 *  nobody's muscle memory moves with the layout. That table is full, so a
 *  newer place (Code) has no key yet rather than stealing one. */
export const ACTIVITIES: Activity[] = [
  { id: "home", label: "Home", icon: House, kbd: "⌘1", hint: "What changed and what waits on you" },
  { id: "docs", label: "Docs", icon: FileText, kbd: "⌘5", hint: "Pages in docs/, notes/, epics/, changelogs/" },
  { id: "morse", label: "Morse", icon: Radio, kbd: "⌘0", hint: "API scenarios pinned to their OpenAPI specs" },
  { id: "work", label: "Work", icon: Columns3, kbd: "⌘3", hint: "Board, issues and the saved lists" },
  { id: "flow", label: "Flow", icon: Waypoints, kbd: "⌘9", hint: "Orchestrations as diagrams" },
  { id: "code", label: "Code", icon: Network, hint: "The code map: who imports what" },
  { id: "plan", label: "Plan", icon: CalendarRange, kbd: "⌘6", hint: "Timeline, roadmap and Gantt" },
  { id: "search", label: "Search", icon: Search, kbd: "⌘2", hint: "Every issue, by words or DQL" },
];

export function ActivityBar({
  active,
  panelOpen,
  badges,
  onActivate,
  only,
}: {
  active: ActivityId;
  /** Whether the side panel is showing — the lit icon says so too. */
  panelOpen: boolean;
  badges: Partial<Record<ActivityId, number | null>>;
  onActivate: (id: ActivityId) => void;
  /** The activities to offer, when not all of them (code-only mode). */
  only?: readonly ActivityId[];
}) {
  const offered = (id: ActivityId) => only === undefined || only.includes(id);
  const item = (a: Activity) => {
    const Icon = a.icon;
    const badge = badges[a.id];
    const on = active === a.id;
    return (
      <button
        key={a.id}
        type="button"
        className={cn("ab-i", on && "on", on && !panelOpen && "folded")}
        aria-label={a.label}
        aria-current={on ? "page" : undefined}
        onClick={() => onActivate(a.id)}
      >
        <Icon className="i" aria-hidden />
        {badge ? <span className="ab-badge">{badge > 999 ? "999+" : badge}</span> : null}
        <span className="ab-tip" role="tooltip">
          <b>{a.label}</b>
          {a.kbd ? <kbd>{a.kbd}</kbd> : null}
          <span>{a.hint}</span>
        </span>
      </button>
    );
  };

  return (
    <nav className="ab" aria-label="Activities">
      {/* The workspace menu lives at the top of the side panel, named and
          marked as a menu; the logo only goes home. */}
      <button
        type="button"
        className="ab-logo"
        aria-label="Home"
        title="Home"
        onClick={() => onActivate(only?.[0] ?? "home")}
      >
        <img src={logo} alt="" width={22} height={22} draggable={false} />
      </button>
      {ACTIVITIES.filter((a) => offered(a.id)).map(item)}
      <span className="ab-sp" />
      {offered("settings")
        ? item({ id: "settings", label: "Settings", icon: Settings, kbd: "⌘,", hint: "Layout, numbering, appearance, people" })
        : null}
    </nav>
  );
}
