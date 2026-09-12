// ⌘K palette: navigation, actions, fuzzy issue quick-open, and doc-page
// quick-open. Issue matching happens on the server (it owns the index); the
// palette only fuzzy-matches the small static action list and the page
// paths locally, so cmdk's built-in filter is switched off and every result
// the server returns stays visible.

import { useEffect, useMemo, useRef, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import { Columns3, FileText, House, ListTodo, Moon, Plus, Search, Star, Sun, Terminal } from "lucide-react";
import { useDocs, useIssues } from "../lib/queries";
import { useDebouncedValue } from "../lib/hooks";
import { looksLikeDql } from "../lib/dql";
import { snippet } from "../lib/snippet";
import type { Route } from "../lib/router";
import { useTheme, type ThemePreference } from "../lib/theme";
import { cn } from "../lib/cn";
import { PriorityDot, TypeBadge } from "./badges";
import { Kbd } from "./chrome";

const NAV_ITEMS: Array<{ label: string; route: Route; keywords: string }> = [
  { label: "Go to Home", route: { name: "home" }, keywords: "home dashboard inbox triage" },
  { label: "Go to Board", route: { name: "board" }, keywords: "board kanban columns" },
  { label: "Go to Issues", route: { name: "issues", q: null }, keywords: "issues list table" },
  { label: "Go to Docs", route: { name: "docs", p: null }, keywords: "docs pages wiki markdown" },
  { label: "Go to Search", route: { name: "search", q: "" }, keywords: "search dql query" },
  {
    label: "Go to Starred",
    route: { name: "issues", q: null, starred: true },
    keywords: "starred favourites favorites shortlist bookmarks",
  },
];

// cmdk group chrome, shared by every group below.
const GROUP_CLASS =
  "[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-muted";
const ITEM_CLASS =
  "flex cursor-default items-center gap-2 rounded px-2 py-1.5 text-[13px] text-ink-2 data-selected:bg-edge data-selected:text-ink";

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

/** The matching line of body text, with the match marked. Renders nothing
 *  when the words are not in the body — the title already said everything
 *  there is to say. */
function Snippet({ body, query }: { body: string; query: string }) {
  const segments = useMemo(() => snippet(body, query), [body, query]);
  if (segments.length === 0) return null;
  return (
    <span className="mt-0.5 block truncate text-[11.5px] text-muted">
      {segments.map((segment, index) =>
        segment.match ? (
          <mark key={index} className="rounded-[2px] bg-accent-soft text-context">
            {segment.text}
          </mark>
        ) : (
          <span key={index}>{segment.text}</span>
        ),
      )}
    </span>
  );
}

export function CommandPalette({
  open,
  onOpenChange,
  onNavigate,
  onOpenIssue,
  onNewIssue,
  onOpenDoc,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (route: Route) => void;
  /** Opens an issue in the side panel, over whatever list is behind it. */
  onOpenIssue: (shortRef: string) => void;
  onNewIssue: () => void;
  /** Opens a doc page in the editor as a preview tab. */
  onOpenDoc: (path: string) => void;
}) {
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
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
  const isQuery = looksLikeDql(trimmed);

  // Theme lives in the palette too: the settings page is the durable home,
  // this is the reflex — "dark" or "light" typed into ⌘K flips it.
  const theme = useTheme();
  const nextTheme: ThemePreference = theme.resolved === "dark" ? "light" : "dark";
  const showTheme = fuzzyMatch(search, "theme dark light appearance");

  const pick = (action: () => void) => {
    onOpenChange(false);
    action();
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-[var(--dit-scrim)]" />
        <DialogPrimitive.Content
          className="fixed left-1/2 top-28 z-50 w-[560px] max-w-[92vw] -translate-x-1/2 overflow-hidden rounded-lg border border-ctl bg-card shadow-[var(--dit-shadow-lg)]"
          onOpenAutoFocus={(event) => {
            // Radix would focus the dialog container, which leaves the first
            // keystroke going nowhere. Take focus for the search box itself —
            // the palette exists to be typed into.
            event.preventDefault();
            inputRef.current?.focus();
          }}
        >
          <DialogPrimitive.Title className="sr-only">Command palette</DialogPrimitive.Title>
          <Command shouldFilter={false} className="flex flex-col" loop>
            <div className="flex items-center gap-2 border-b border-edge px-3">
              <Search className="size-4 shrink-0 text-muted" aria-hidden />
              <Command.Input
                ref={inputRef}
                value={search}
                onValueChange={setSearch}
                placeholder="Search issues, pages or type a command…"
                className="h-11 w-full bg-transparent text-sm text-ink placeholder:text-faint focus:outline-none"
              />
              <Kbd>esc</Kbd>
            </div>
            <Command.List className="max-h-80 overflow-y-auto p-1.5">
              <Command.Empty className="px-3 py-6 text-center text-sm text-muted">
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
                        <House className="size-4 text-muted" aria-hidden />
                      ) : item.route.name === "board" ? (
                        <Columns3 className="size-4 text-muted" aria-hidden />
                      ) : item.route.name === "issues" && item.route.starred !== true ? (
                        <ListTodo className="size-4 text-muted" aria-hidden />
                      ) : item.route.name === "docs" ? (
                        <FileText className="size-4 text-muted" aria-hidden />
                      ) : item.route.name === "issues" ? (
                        <Star className="size-4 text-muted" aria-hidden />
                      ) : (
                        <Search className="size-4 text-muted" aria-hidden />
                      )}
                      {item.label}
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}

              {showNewIssue || showTheme ? (
                <Command.Group heading="Actions" className={GROUP_CLASS}>
                  {showNewIssue ? (
                    <Command.Item
                      value="new issue"
                      onSelect={() => pick(onNewIssue)}
                      className={ITEM_CLASS}
                    >
                      <Plus className="size-4 text-muted" aria-hidden />
                      New issue
                    </Command.Item>
                  ) : null}
                  {showTheme ? (
                    <Command.Item
                      value="switch theme"
                      onSelect={() => pick(() => theme.setPreference(nextTheme))}
                      className={ITEM_CLASS}
                    >
                      {nextTheme === "dark" ? (
                        <Moon className="size-4 text-muted" aria-hidden />
                      ) : (
                        <Sun className="size-4 text-muted" aria-hidden />
                      )}
                      Switch to {nextTheme} theme
                    </Command.Item>
                  ) : null}
                </Command.Group>
              ) : null}

              {/* Typing a query and typing words are different intents, and
                  the palette can tell them apart — so it offers to run the
                  query instead of searching for its text. */}
              {trimmed.length > 0 ? (
                <Command.Group heading="Query" className={GROUP_CLASS}>
                  <Command.Item
                    value="run query"
                    onSelect={() => pick(() => onNavigate({ name: "search", q: trimmed }))}
                    className={ITEM_CLASS}
                  >
                    {isQuery ? (
                      <Terminal className="size-4 shrink-0 text-muted" aria-hidden />
                    ) : (
                      <Search className="size-4 shrink-0 text-muted" aria-hidden />
                    )}
                    <span className="min-w-0 truncate">
                      {isQuery ? "Run " : "Search everything for "}
                      <span className={isQuery ? "font-mono text-ink" : "text-ink"}>{trimmed}</span>
                    </span>
                    <Kbd className="ml-auto shrink-0">⏎</Kbd>
                  </Command.Item>
                </Command.Group>
              ) : null}

              {trimmed.length > 0 ? (
                <Command.Group heading="Issues" className={GROUP_CLASS}>
                  {results.isFetching && (results.data?.items.length ?? 0) === 0 ? (
                    <div className="px-2 py-2 text-xs text-muted">Searching…</div>
                  ) : null}
                  {results.isError ? (
                    <div className="px-2 py-2 text-xs text-crit-text">
                      {results.error instanceof Error ? results.error.message : "Search failed"}
                    </div>
                  ) : null}
                  {results.data?.items.length === 0 && !results.isFetching ? (
                    <div className="px-2 py-2 text-xs text-muted">
                      No issues match “{trimmed}”.
                    </div>
                  ) : null}
                  {(results.data?.items ?? []).map((issue) => (
                    <Command.Item
                      key={issue.id}
                      value={issue.id}
                      onSelect={() => pick(() => onOpenIssue(issue.short_ref))}
                      className={cn(ITEM_CLASS, "items-start")}
                    >
                      <span className="mt-px font-mono text-xs tabular-nums text-muted">
                        {issue.number !== null ? `#${issue.number}` : issue.short_ref}
                      </span>
                      <span className="mt-px">
                        <TypeBadge type={issue.type} />
                      </span>
                      <span className="mt-1.5">
                        <PriorityDot priority={issue.priority} />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate">{issue.title}</span>
                        {/* Why this one matched: a title alone often does not
                            say, because the words were paragraphs down. */}
                        <Snippet body={issue.body} query={trimmed} />
                      </span>
                      <span className="mt-px shrink-0 font-mono text-[10px] text-faint">
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
                      <FileText className="size-4 shrink-0 text-muted" aria-hidden />
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
