// Left rail: the three primary views plus the two global affordances
// (new issue, command palette). Keyboard-first tools earn a persistent,
// predictable nav — no hamburger, no collapsing.

import { Columns3, ListTodo, Plus, Search, Settings } from "lucide-react";
import type { Route } from "../lib/router";
import { cn } from "../lib/cn";

interface NavItem {
  label: string;
  route: Route;
  icon: typeof Columns3;
  match: (route: Route) => boolean;
}

const ITEMS: NavItem[] = [
  { label: "Board", route: { name: "board" }, icon: Columns3, match: (r) => r.name === "board" },
  { label: "Issues", route: { name: "issues" }, icon: ListTodo, match: (r) => r.name === "issues" },
  {
    label: "Search",
    route: { name: "search", q: "" },
    icon: Search,
    match: (r) => r.name === "search",
  },
  {
    label: "Settings",
    route: { name: "settings" },
    icon: Settings,
    match: (r) => r.name === "settings",
  },
];

export function Sidebar({
  route,
  onNavigate,
  onNewIssue,
  onOpenPalette,
}: {
  route: Route;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
  onOpenPalette: () => void;
}) {
  return (
    <nav className="flex w-48 shrink-0 flex-col border-r border-zinc-800 bg-zinc-950">
      <div className="flex h-10 items-center gap-2 border-b border-zinc-800 px-3">
        <span className="rounded bg-sky-800 px-1.5 py-0.5 font-mono text-[11px] font-bold text-sky-200">
          DIT
        </span>
        <span className="text-[11px] text-zinc-500">Done in Git</span>
      </div>

      <div className="flex flex-col gap-px p-2">
        {ITEMS.map((item) => {
          const active = item.match(route);
          const Icon = item.icon;
          return (
            <button
              key={item.label}
              type="button"
              onClick={() => onNavigate(item.route)}
              className={cn(
                "flex items-center gap-2 rounded px-2 py-1.5 text-left text-[13px]",
                active
                  ? "bg-zinc-800 text-zinc-100"
                  : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-200",
              )}
            >
              <Icon className="size-4" aria-hidden />
              {item.label}
            </button>
          );
        })}
      </div>

      <div className="mt-auto flex flex-col gap-2 p-2">
        <button
          type="button"
          onClick={onNewIssue}
          className="flex items-center gap-2 rounded border border-zinc-700 px-2 py-1.5 text-[13px] text-zinc-300 hover:border-zinc-500 hover:text-zinc-100"
        >
          <Plus className="size-4" aria-hidden />
          New issue
        </button>
        <button
          type="button"
          onClick={onOpenPalette}
          className="flex items-center justify-between rounded px-2 py-1.5 text-[13px] text-zinc-500 hover:bg-zinc-900 hover:text-zinc-300"
        >
          <span className="flex items-center gap-2">
            <Search className="size-4" aria-hidden />
            Jump to…
          </span>
          <kbd className="rounded border border-zinc-700 bg-zinc-900 px-1 font-mono text-[10px] text-zinc-500">
            ⌘K
          </kbd>
        </button>
      </div>
    </nav>
  );
}
