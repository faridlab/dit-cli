// The workbench's leftmost rail (the VS Code "activity bar"): one icon per
// view, plus create and settings pinned to the bottom edge. The rail never
// collapses — it is the app's spatial memory; what changes per view is the
// pane beside it, not the rail.

import { Columns3, FileText, House, ListTodo, Plus, Search, Settings } from "lucide-react";
import * as Tooltip from "@radix-ui/react-tooltip";
import type { Route } from "../lib/router";
import { useStatus } from "../lib/queries";
import { cn } from "../lib/cn";
import { Kbd } from "./chrome";

interface ActivityItem {
  label: string;
  shortcut: string;
  route: Route;
  icon: typeof House;
  match: (route: Route) => boolean;
}

// The rail order is also the ⌘1..⌘5 shortcut order.
const ITEMS: ActivityItem[] = [
  { label: "Home", shortcut: "⌘1", route: { name: "home" }, icon: House, match: (r) => r.name === "home" },
  {
    label: "Search",
    shortcut: "⌘2",
    route: { name: "search", q: "" },
    icon: Search,
    match: (r) => r.name === "search",
  },
  {
    label: "Board",
    shortcut: "⌘3",
    route: { name: "board" },
    icon: Columns3,
    match: (r) => r.name === "board",
  },
  {
    label: "Issues",
    shortcut: "⌘4",
    route: { name: "issues", q: null },
    icon: ListTodo,
    match: (r) => r.name === "issues",
  },
  {
    label: "Docs",
    shortcut: "⌘5",
    route: { name: "docs", p: null },
    icon: FileText,
    match: (r) => r.name === "docs",
  },
];

function RailButton({
  label,
  shortcut,
  active,
  onClick,
  children,
}: {
  label: string;
  shortcut?: string;
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>
        <button
          type="button"
          onClick={onClick}
          aria-label={label}
          aria-current={active ? "page" : undefined}
          className={cn(
            "relative flex size-12 shrink-0 items-center justify-center transition-colors",
            active ? "text-zinc-100" : "text-zinc-500 hover:text-zinc-200",
          )}
        >
          {active ? (
            <span className="absolute inset-y-[9px] left-0 w-[2px] rounded-r bg-accent" aria-hidden />
          ) : null}
          {children}
        </button>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content
          side="right"
          sideOffset={6}
          className="z-50 flex items-center gap-2 rounded-md border border-ctl bg-card px-2.5 py-1.5 text-xs text-zinc-200 shadow-xl"
        >
          {label}
          {shortcut ? <Kbd>{shortcut}</Kbd> : null}
          <Tooltip.Arrow className="fill-ctl" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}

export function ActivityBar({
  route,
  onNavigate,
  onNewIssue,
}: {
  route: Route;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
}) {
  const status = useStatus();
  const workspace = status.data
    ? (status.data.repo.split("/").filter(Boolean).pop() ?? status.data.repo)
    : null;

  return (
    <Tooltip.Provider delayDuration={350} skipDelayDuration={150}>
      <nav className="flex w-12 shrink-0 flex-col items-stretch border-r border-edge bg-panel" aria-label="Views">
        {/* The workspace identity doubles as a home affordance — the badge is
            where the eye lands first, so that is where "start over" lives. */}
        <RailButton
          label={workspace ? `${workspace} — Home` : "Home"}
          shortcut="⌘1"
          active={route.name === "home"}
          onClick={() => onNavigate({ name: "home" })}
        >
          <span className="rounded bg-sky-800 px-1.5 py-0.5 font-mono text-[11px] font-bold text-sky-200">
            DIT
          </span>
        </RailButton>

        <div className="mx-auto my-1 h-px w-6 bg-edge" aria-hidden />

        {ITEMS.map((item) => (
          <RailButton
            key={item.label}
            label={item.label}
            shortcut={item.shortcut}
            active={item.match(route)}
            onClick={() => onNavigate(item.route)}
          >
            <item.icon className="size-[19px]" aria-hidden />
          </RailButton>
        ))}

        <div className="mt-auto flex flex-col">
          <RailButton label="New issue (⌘K → New issue)" active={false} onClick={onNewIssue}>
            <Plus className="size-[19px]" aria-hidden />
          </RailButton>
          <RailButton
            label="Settings"
            shortcut="⌘,"
            active={route.name === "settings"}
            onClick={() => onNavigate({ name: "settings" })}
          >
            <Settings className="size-[19px]" aria-hidden />
          </RailButton>
        </div>
      </nav>
    </Tooltip.Provider>
  );
}
