// ⌘K palette: navigation, actions, fuzzy issue quick-open, and doc-page
// quick-open. Issue matching happens on the server (it owns the index); the
// palette only fuzzy-matches the small static action list and the page
// paths locally, so cmdk's built-in filter is switched off and every result
// the server returns stays visible.

import { useEffect, useMemo, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { Columns3, FileText, House, ListTodo, Plus, Search } from "lucide-react";
import { useDocs, useIssues } from "../lib/queries";
import { useDebouncedValue } from "../lib/hooks";
import type { Route } from "../lib/router";
import { PriorityDot, TypeBadge } from "./badges";
import { Kbd } from "./chrome";

const NAV_ITEMS: Array<{ label: string; route: Route; keywords: string }> = [
  { label: "Go to Home", route: { name: "home" }, keywords: "home dashboard inbox triage" },
  { label: "Go to Board", route: { name: "board" }, keywords: "board kanban columns" },
  { label: "Go to Issues", route: { name: "issues", q: null }, keywords: "issues list table" },
  { label: "Go to Docs", route: { name: "docs", p: null }, keywords: "docs pages wiki markdown" },
  { label: "Go to Search", route: { name: "search", q: "" }, keywords: "search dql query" },
];

// cmdk group chrome, shared by every group below.
const GROUP_CLASS =
  "[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-zinc-500";
const ITEM_CLASS =
  "flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-[13px] text-zinc-300 data-selected:bg-edge data-selected:text-zinc-100";

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
  onOpenDoc,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (route: Route) => void;
  onNewIssue: () => void;
  /** Opens a doc page in the editor as a preview tab. */
  onOpenDoc: (path: string) => void;
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

  // Pages match locally: the listing is small and already cached by the
  // explorer, and a path subsequence ("dfla" → docs/flows/auth.md) is a
  // better fit than any server round trip.
  const docs = useDocs(open);
  const pages = useMemo(() => {
    if (trimmed.length === 0) return [];
    return (docs.data ?? []).filter((entry) => fuzzyMatch(trimmed, entry.path)).slice(0, 8);
  }, [docs.data, trimmed]);

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
          className="fixed left-1/2 top-28 z-50 w-[560px] max-w-[92vw] -translate-x-1/2 overflow-hidden rounded-lg border border-ctl bg-card shadow-[0_25px_50px_-12px_rgba(0,0,0,0.8)]"
          onOpenAutoFocus={(event) => {
            // Let the command input take focus instead of the dialog itself.
            event.preventDefault();
          }}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <Command shouldFilter={false} className="flex flex-col" loop>
            <div className="flex items-center gap-2 border-b border-edge px-3">
              <Search className="size-4 shrink-0 text-zinc-500" aria-hidden />
              <Command.Input
                value={search}
                onValueChange={setSearch}
                placeholder="Search issues, pages or type a command…"
                className="h-11 w-full bg-transparent text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none"
              />
              <Kbd>esc</Kbd>
            </div>
            <Command.List className="max-h-80 overflow-y-auto p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-sm text-zinc-500">
                No matches.
              </Command.Empty>

              {navItems.length > 0 ? (
                <Command.Group heading="Navigate" className={GROUP_CLASS}>
                  {navItems.map((item) => (
                    <Command.Item
                      key={item.label}
                      value={item.label}
                      onSelect={() => pick(() => onNavigate(item.route))}
                      className={ITEM_CLASS}
                    >
                      {item.route.name === "home" ? (
                        <House className="size-4 text-zinc-500" aria-hidden />
                      ) : item.route.name === "board" ? (
                        <Columns3 className="size-4 text-zinc-500" aria-hidden />
                      ) : item.route.name === "issues" ? (
                        <ListTodo className="size-4 text-zinc-500" aria-hidden />
                      ) : item.route.name === "docs" ? (
                        <FileText className="size-4 text-zinc-500" aria-hidden />
                      ) : (
                        <Search className="size-4 text-zinc-500" aria-hidden />
                      )}
                      {item.label}
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}

              {showNewIssue ? (
                <Command.Group heading="Actions" className={GROUP_CLASS}>
                  <Command.Item
                    value="new issue"
                    onSelect={() => pick(onNewIssue)}
                    className={ITEM_CLASS}
                  >
                    <Plus className="size-4 text-zinc-500" aria-hidden />
                    New issue
                  </Command.Item>
                </Command.Group>
              ) : null}

              {trimmed.length > 0 ? (
                <Command.Group heading="Issues" className={GROUP_CLASS}>
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
                      className={ITEM_CLASS}
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

              {pages.length > 0 ? (
                <Command.Group heading="Pages" className={GROUP_CLASS}>
                  {pages.map((page) => (
                    <Command.Item
                      key={page.path}
                      value={`page:${page.path}`}
                      onSelect={() => pick(() => onOpenDoc(page.path))}
                      className={ITEM_CLASS}
                    >
                      <FileText className="size-4 shrink-0 text-zinc-500" aria-hidden />
                      <span className="truncate font-mono text-xs">{page.path}</span>
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
