// Left rail: the primary views plus the two global affordances (new issue,
// command palette). Keyboard-first tools earn a persistent, predictable nav —
// no hamburger, no collapsing. The header carries the workspace identity
// (badge, name, branch) so every view answers "where am I" without asking.

import { Columns3, FileText, House, ListTodo, Plus, Search, Settings } from "lucide-react";
import type { Route } from "../lib/router";
import { useStatus } from "../lib/queries";
import { cn } from "../lib/cn";
import { Kbd } from "./chrome";

interface NavItem {
  label: string;
  route: Route;
  icon: typeof Columns3;
  match: (route: Route) => boolean;
}

const ITEMS: NavItem[] = [
  { label: "Home", route: { name: "home" }, icon: House, match: (r) => r.name === "home" },
  { label: "Board", route: { name: "board" }, icon: Columns3, match: (r) => r.name === "board" },
  { label: "Issues", route: { name: "issues" }, icon: ListTodo, match: (r) => r.name === "issues" },
  {
    label: "Docs",
    route: { name: "docs", p: null },
    icon: FileText,
    match: (r) => r.name === "docs",
  },
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
  const status = useStatus();
  const workspace = status.data ? status.data.repo.split("/").filter(Boolean).pop() : null;

  return (
    <nav className="flex w-56 shrink-0 flex-col border-r border-edge bg-panel">
      <div className="flex h-[42px] items-center gap-2 border-b border-edge px-3">
        <span className="rounded bg-sky-800 px-1.5 py-0.5 font-mono text-[11px] font-bold text-sky-200">
          DIT
        </span>
        <span className="truncate text-xs text-zinc-400" title={status.data?.repo ?? undefined}>
          {workspace ?? "…"}
        </span>
        {status.data ? (
          <span className="ml-auto shrink-0 font-mono text-[10px] text-dim">
            {status.data.branch}
          </span>
        ) : null}
      </div>

      <div className="flex flex-col p-2">
        {ITEMS.map((item) => {
          const active = item.match(route);
          const Icon = item.icon;
          return (
            <button
              key={item.label}
              type="button"
              onClick={() => onNavigate(item.route)}
              className={cn(
                "flex items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-sm",
                active
                  ? "bg-edge text-zinc-100"
                  : "text-zinc-400 hover:bg-card hover:text-zinc-200",
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
          className="flex items-center gap-2 rounded-md border border-ctl px-2.5 py-1.5 text-[13px] text-zinc-300 transition-colors hover:border-zinc-500 hover:text-zinc-100"
        >
          <Plus className="size-4" aria-hidden />
          New issue
        </button>
        <button
          type="button"
          onClick={onOpenPalette}
          className="flex items-center justify-between rounded-md px-2.5 py-1.5 text-[13px] text-zinc-500 transition-colors hover:bg-card hover:text-zinc-300"
        >
          <span className="flex items-center gap-2">
            <Search className="size-4" aria-hidden />
            Jump to…
          </span>
          <Kbd>⌘K</Kbd>
        </button>
      </div>
    </nav>
  );
}
