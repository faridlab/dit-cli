// ⌘K palette: navigation, actions, and fuzzy issue quick-open. Issue
// matching happens on the server (it owns the index); the palette only
// fuzzy-matches the small static action list locally, so cmdk's built-in
// filter is switched off and every result the server returns stays visible.

import { useEffect, useMemo, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { Columns3, ListTodo, Plus, Search } from "lucide-react";
import { useIssues } from "../lib/queries";
import { useDebouncedValue } from "../lib/hooks";
import type { Route } from "../lib/router";
import { PriorityDot, TypeBadge } from "./badges";

const NAV_ITEMS: Array<{ label: string; route: Route; keywords: string }> = [
  { label: "Go to Board", route: { name: "board" }, keywords: "board kanban columns" },
  { label: "Go to Issues", route: { name: "issues" }, keywords: "issues list table" },
  { label: "Go to Search", route: { name: "search", q: "" }, keywords: "search dql query" },
];

// Subsequence match with a preference for contiguous runs — good enough for
// a four-item list and never worse than the server's issue matching.
function fuzzyMatch(needle: string, haystack: string): boolean {
  const n = needle.toLowerCase();
  const h = haystack.toLowerCase();
  if (n.length === 0) return true;
  let index = 0;
  for (const char of h) {
    if (char === n[index]) index += 1;
    if (index === n.length) return true;
  }
  return false;
}

export function CommandPalette({
  open,
  onOpenChange,
  onNavigate,
  onNewIssue,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
}) {
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search, 200);
  const trimmed = debouncedSearch.trim();

  // A fresh palette should never show the previous session's leftovers.
  useEffect(() => {
    if (!open) setSearch("");
  }, [open]);

  // Explicit title/body search: deterministic and fast on the server index.
  const results = useIssues(
    { q: `title ~ ${trimmed} OR body ~ ${trimmed}`, limit: 8 },
    open && trimmed.length > 0,
  );

  const navItems = useMemo(
    () =>
      NAV_ITEMS.filter(
        (item) =>
          fuzzyMatch(search, item.label) || item.keywords.split(" ").some((k) => fuzzyMatch(search, k)),
      ),
    [search],
  );

  const showNewIssue = fuzzyMatch(search, "new issue create");

  const pick = (action: () => void) => {
    onOpenChange(false);
    action();
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-black/60" />
        <DialogPrimitive.Content
          className="fixed left-1/2 top-28 z-50 w-[560px] max-w-[92vw] -translate-x-1/2 overflow-hidden rounded-lg border border-zinc-700 bg-zinc-900 shadow-2xl"
          onOpenAutoFocus={(event) => {
            // Let the command input take focus instead of the dialog itself.
            event.preventDefault();
          }}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <Command shouldFilter={false} className="flex flex-col" loop>
            <div className="flex items-center gap-2 border-b border-zinc-800 px-3">
              <Search className="size-4 shrink-0 text-zinc-500" aria-hidden />
              <Command.Input
                value={search}
                onValueChange={setSearch}
                placeholder="Search issues or type a command…"
                className="h-11 w-full bg-transparent text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none"
              />
              <kbd className="rounded border border-zinc-700 px-1 font-mono text-[10px] text-zinc-500">
                esc
              </kbd>
            </div>
            <Command.List className="max-h-80 overflow-y-auto p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-sm text-zinc-500">
                No matches.
              </Command.Empty>

              {navItems.length > 0 ? (
                <Command.Group
                  heading="Navigate"
                  className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-zinc-500"
                >
                  {navItems.map((item) => (
                    <Command.Item
                      key={item.label}
                      value={item.label}
                      onSelect={() => pick(() => onNavigate(item.route))}
                      className="flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-[13px] text-zinc-300 data-selected:bg-zinc-800 data-selected:text-zinc-100"
                    >
                      {item.route.name === "board" ? (
                        <Columns3 className="size-4 text-zinc-500" aria-hidden />
                      ) : item.route.name === "issues" ? (
                        <ListTodo className="size-4 text-zinc-500" aria-hidden />
                      ) : (
                        <Search className="size-4 text-zinc-500" aria-hidden />
                      )}
                      {item.label}
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}

              {showNewIssue ? (
                <Command.Group
                  heading="Actions"
                  className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-zinc-500"
                >
                  <Command.Item
                    value="new issue"
                    onSelect={() => pick(onNewIssue)}
                    className="flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-[13px] text-zinc-300 data-selected:bg-zinc-800 data-selected:text-zinc-100"
                  >
                    <Plus className="size-4 text-zinc-500" aria-hidden />
                    New issue
                  </Command.Item>
                </Command.Group>
              ) : null}

              {trimmed.length > 0 ? (
                <Command.Group
                  heading="Issues"
                  className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-zinc-500"
                >
                  {results.isFetching && (results.data?.items.length ?? 0) === 0 ? (
                    <div className="px-2 py-2 text-xs text-zinc-500">Searching…</div>
                  ) : null}
                  {results.isError ? (
                    <div className="px-2 py-2 text-xs text-red-400">
                      {results.error instanceof Error ? results.error.message : "Search failed"}
                    </div>
                  ) : null}
                  {results.data?.items.length === 0 && !results.isFetching ? (
                    <div className="px-2 py-2 text-xs text-zinc-500">
                      No issues match “{trimmed}”.
                    </div>
                  ) : null}
                  {(results.data?.items ?? []).map((issue) => (
                    <Command.Item
                      key={issue.id}
                      value={issue.id}
                      onSelect={() => pick(() => onNavigate({ name: "issue", id: issue.short_ref }))}
                      className="flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-[13px] text-zinc-300 data-selected:bg-zinc-800 data-selected:text-zinc-100"
                    >
                      <span className="font-mono text-xs tabular-nums text-zinc-500">
                        {issue.number !== null ? `#${issue.number}` : issue.short_ref}
                      </span>
                      <TypeBadge type={issue.type} />
                      <PriorityDot priority={issue.priority} />
                      <span className="truncate">{issue.title}</span>
                      <span className="ml-auto shrink-0 font-mono text-[10px] text-zinc-600">
                        {issue.status}
                      </span>
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}
            </Command.List>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
