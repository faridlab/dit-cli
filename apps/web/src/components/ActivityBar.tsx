// The first of the workbench's three regions: one icon per kind of place.
//
// The icons stand for what the product is made of — material (Home, Docs,
// Morse), the lenses over issues (Work, Flow, Plan), and Search — with
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
  Waypoints,
} from "lucide-react";
import { cn } from "../lib/cn";
import type { ActivityId } from "../lib/workbench";
import { MenuButton, type MenuItem } from "./chrome";
import logo from "../assets/dit-logo.png";

type Icon = typeof House;

/** The shortcut shown is the one AppShell's ⌘1…⌘0 table already binds, so
 *  nobody's muscle memory moves with the layout. */
export const ACTIVITIES: { id: ActivityId; label: string; icon: Icon; kbd: string; hint: string }[] = [
  { id: "home", label: "Home", icon: House, kbd: "⌘1", hint: "What changed and what waits on you" },
  { id: "docs", label: "Docs", icon: FileText, kbd: "⌘5", hint: "Pages in docs/, notes/, epics/, changelogs/" },
  { id: "morse", label: "Morse", icon: Radio, kbd: "⌘0", hint: "API scenarios pinned to their OpenAPI specs" },
  { id: "work", label: "Work", icon: Columns3, kbd: "⌘3", hint: "Board, issues and the saved lists" },
  { id: "flow", label: "Flow", icon: Waypoints, kbd: "⌘9", hint: "Orchestrations as diagrams" },
  { id: "plan", label: "Plan", icon: CalendarRange, kbd: "⌘6", hint: "Timeline, roadmap and Gantt" },
  { id: "search", label: "Search", icon: Search, kbd: "⌘2", hint: "Every issue, by words or DQL" },
];

export function ActivityBar({
  active,
  panelOpen,
  badges,
  onActivate,
  workspaceMenu,
  workspace,
}: {
  active: ActivityId;
  /** Whether the side panel is showing — the lit icon says so too. */
  panelOpen: boolean;
  badges: Partial<Record<ActivityId, number | null>>;
  onActivate: (id: ActivityId) => void;
  workspaceMenu: MenuItem[];
  workspace: string;
}) {
  const item = (a: { id: ActivityId; label: string; icon: Icon; kbd: string; hint: string }) => {
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
          <kbd>{a.kbd}</kbd>
          <span>{a.hint}</span>
        </span>
      </button>
    );
  };

  return (
    <nav className="ab" aria-label="Activities">
      <MenuButton items={workspaceMenu}>
        <button type="button" className="ab-logo" aria-label={`Workspace menu — ${workspace}`} title={workspace}>
          <img src={logo} alt="" width={22} height={22} draggable={false} />
        </button>
      </MenuButton>
      {ACTIVITIES.map(item)}
      <span className="ab-sp" />
      {item({ id: "settings", label: "Settings", icon: Settings, kbd: "⌘,", hint: "Layout, numbering, appearance, people" })}
    </nav>
  );
}
