// ⌘K palette: issues with a matching snippet, pages, a query runner,
// navigation and actions — one box, grouped, with a scope footer. Issue
// matching happens on the server (it owns the index); the palette only
// fuzzy-matches the small static lists locally, so cmdk's built-in filter
// is switched off and every result the server returns stays visible.

import { useEffect, useMemo, useRef, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { Command } from "cmdk";
import {
  ChartGantt,
  Clock,
  Columns3,
  FileText,
  House,
  Inbox,
  Info,
  Layers,
  ListTodo,
  Moon,
  PanelLeft,
  Plus,
  Search,
  Settings,
  Star,
  Sun,
  Terminal,
  UserRound,
} from "lucide-react";
import { toast } from "sonner";
import { useDocs, useIssues, useSchema, useStatus } from "../lib/queries";
import { useDebouncedValue } from "../lib/hooks";
import { looksLikeDql, mineQuery } from "../lib/dql";
import { snippet } from "../lib/snippet";
import { relativeTime } from "../lib/format";
import type { Route } from "../lib/router";
import { useTheme } from "../lib/theme";
import { useViewOptions } from "../lib/viewopts";
import { TypeBadge } from "./badges";

type Icon = typeof House;

const NAV: Array<{ label: string; icon: Icon; route: Route; kbd?: string; keywords: string }> = [
  { label: "Home", icon: House, route: { name: "home" }, kbd: "⌘1", keywords: "home dashboard" },
  { label: "Board", icon: Columns3, route: { name: "board" }, kbd: "⌘3", keywords: "board kanban columns" },
  { label: "Issues", icon: ListTodo, route: { name: "issues", q: null }, kbd: "⌘4", keywords: "issues list table" },
  { label: "Docs", icon: FileText, route: { name: "docs", p: null }, kbd: "⌘5", keywords: "docs pages wiki markdown" },
  { label: "Inbox", icon: Inbox, route: { name: "issues", q: null, inbox: true }, keywords: "inbox triage untriaged" },
  { label: "Timeline", icon: Clock, route: { name: "timeline" }, kbd: "⌘6", keywords: "timeline history events" },
  { label: "Roadmap", icon: Layers, route: { name: "roadmap" }, kbd: "⌘7", keywords: "roadmap epics releases quarters" },
  { label: "Gantt", icon: ChartGantt, route: { name: "gantt" }, kbd: "⌘8", keywords: "gantt schedule dates" },
  { label: "Starred", icon: Star, route: { name: "issues", q: null, starred: true }, keywords: "starred favourites shortlist" },
  { label: "Settings", icon: Settings, route: { name: "settings" }, kbd: "⌘,", keywords: "settings preferences theme" },
];

// Subsequence match with a preference for contiguous runs — good enough for
// a dozen-item list and never worse than the server's issue matching.
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

/** `text` with every case-insensitive occurrence of `query` marked. */
function Marked({ text, query }: { text: string; query: string }) {
  if (query.length === 0) return <>{text}</>;
  const parts: React.ReactNode[] = [];
  const lower = text.toLowerCase();
  const q = query.toLowerCase();
  let from = 0;
  for (;;) {
    const at = lower.indexOf(q, from);
    if (at < 0) break;
    parts.push(text.slice(from, at), <mark key={at}>{text.slice(at, at + q.length)}</mark>);
    from = at + q.length;
  }
  parts.push(text.slice(from));
  return <>{parts}</>;
}

/** The matching line of body text, with the match marked. */
function Snippet({ body, query }: { body: string; query: string }) {
  const segments = useMemo(() => snippet(body, query), [body, query]);
  if (segments.length === 0) return null;
  return (
    <div className="sn">
      {segments.map((segment, index) =>
        segment.match ? <mark key={index}>{segment.text}</mark> : <span key={index}>{segment.text}</span>,
      )}
    </div>
  );
}

/** Quote user text for a DQL `~` match. */
function dqlText(text: string): string {
  return `"${text.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

async function copyText(text: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${label} — ${text}`);
  } catch {
    toast(`${label}: ${text}`);
  }
}

export function CommandPalette({
  open,
  onOpenChange,
  onNavigate,
  onOpenIssue,
  onNewIssue,
  onOpenDoc,
  onToggleSidebar,
  sidebarHidden,
  onNotes,
  cli,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onNavigate: (route: Route) => void;
  onOpenIssue: (shortRef: string) => void;
  onNewIssue: () => void;
  onOpenDoc: (path: string) => void;
  onToggleSidebar: () => void;
  sidebarHidden: boolean;
  onNotes: () => void;
  /** The CLI spelling of the current screen. */
  cli: string;
}) {
  const [search, setSearch] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const debouncedSearch = useDebouncedValue(search, 200);
  const trimmed = debouncedSearch.trim();

  useEffect(() => {
    if (!open) setSearch("");
  }, [open]);

  const status = useStatus();
  const schema = useSchema();
  const statuses = schema.data?.workflow.statuses ?? [];
  const statusLabel = (id: string) => statuses.find((s) => s.id === id)?.label ?? id;
  const workspace = status.data ? (status.data.repo.split("/").filter(Boolean).pop() ?? status.data.repo) : "…";

  // Full text over title and body on the server index; a handle (`#12`)
  // or a short ref matches by exact field so typing one jumps straight to it.
  const handle = /^#?(\d+)$/.exec(trimmed);
  const searchQuery = handle
    ? `number = ${handle[1]}`
    : /^[0-9A-HJKMNP-TV-Z]{7}$/i.test(trimmed)
      ? `short_ref = ${trimmed.toUpperCase()}`
      : `body ~ ${dqlText(trimmed)}`;
  const results = useIssues({ q: searchQuery, limit: 6 }, open && trimmed.length > 0);
  const all = useIssues({ limit: 1 }, open);

  const docs = useDocs(open);
  const pages = useMemo(() => {
    if (trimmed.length === 0) return [];
    return (docs.data ?? []).filter((entry) => fuzzyMatch(trimmed, entry.path)).slice(0, 6);
  }, [docs.data, trimmed]);

  const nav = useMemo(
    () =>
      NAV.filter(
        (item) =>
          fuzzyMatch(search, item.label) || item.keywords.split(" ").some((word) => fuzzyMatch(search, word)),
      ),
    [search],
  );
  const mine = mineQuery(statuses);

  const theme = useTheme();
  const dark = theme.resolved === "dark";
  const { board, setGroupBy } = useViewOptions();
  const nextGroup = board.groupBy === "status" ? "assignee" : "status";

  const actions: Array<{ label: string; icon: Icon; kbd?: string; run: () => void }> = [
    { label: "New issue", icon: Plus, kbd: "C", run: onNewIssue },
    { label: sidebarHidden ? "Show sidebar" : "Hide sidebar", icon: PanelLeft, kbd: "⌘B", run: onToggleSidebar },
    {
      label: dark ? "Switch to light theme" : "Switch to dark theme",
      icon: dark ? Sun : Moon,
      run: () => theme.setPreference(dark ? "light" : "dark"),
    },
    {
      label: `Board: group by ${nextGroup}`,
      icon: Columns3,
      run: () => {
        setGroupBy(nextGroup);
        onNavigate({ name: "board" });
      },
    },
    { label: "Copy CLI command for this view", icon: Terminal, run: () => void copyText(cli, "Command copied") },
    { label: "Notes", icon: Info, run: onNotes },
  ];
  const shownActions = actions.filter((action) => fuzzyMatch(search, action.label));
  const isQuery = looksLikeDql(trimmed);

  const pick = (action: () => void) => {
    onOpenChange(false);
    action();
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="overlay open" />
        <DialogPrimitive.Content
          className="pal fixed left-1/2 top-[12vh] z-[51] -translate-x-1/2"
          aria-label="Search and commands"
          onOpenAutoFocus={(event) => {
            // Radix would focus the dialog container, which leaves the first
            // keystroke going nowhere. The palette exists to be typed into.
            event.preventDefault();
            inputRef.current?.focus();
          }}
        >
          <DialogPrimitive.Title className="sr-only">Search and commands</DialogPrimitive.Title>
          <Command shouldFilter={false} className="flex min-h-0 flex-col" loop>
            <div className="pal-in">
              <Search className="i" style={{ color: "var(--muted)" }} aria-hidden />
              <Command.Input
                ref={inputRef}
                value={search}
                onValueChange={setSearch}
                placeholder="Search issues and pages, type DQL, or run a command…"
                autoComplete="off"
              />
              <span className="esc">esc</span>
            </div>
            <Command.List className="pal-list">
              <Command.Empty className="empty" style={{ padding: 16 }}>
                Nothing matches.
              </Command.Empty>

              {trimmed.length > 0 ? (
                <Command.Group heading="Issues">
                  {results.isFetching && (results.data?.items.length ?? 0) === 0 ? (
                    <div className="empty" style={{ padding: "6px 10px" }}>
                      Searching…
                    </div>
                  ) : null}
                  {results.isError ? (
                    <div className="empty" style={{ padding: "6px 10px", color: "var(--crit)" }}>
                      {results.error instanceof Error ? results.error.message : "Search failed"}
                    </div>
                  ) : null}
                  {results.data?.items.length === 0 && !results.isFetching ? (
                    <div className="empty" style={{ padding: "6px 10px" }}>
                      No issues match “{trimmed}”.
                    </div>
                  ) : null}
                  {(results.data?.items ?? []).map((issue) => (
                    <Command.Item
                      key={issue.id}
                      value={`issue:${issue.id}`}
                      onSelect={() => pick(() => onOpenIssue(issue.short_ref))}
                      className="pi"
                    >
                      <TypeBadge type={issue.type} />
                      <div className="min-w-0">
                        <div className="t">
                          {issue.number !== null ? `#${issue.number}` : issue.short_ref}{" "}
                          <Marked text={issue.title} query={trimmed} />
                        </div>
                        <Snippet body={issue.body} query={trimmed} />
                      </div>
                      <span className="meta">
                        {statusLabel(issue.status)} · {relativeTime(issue.updated)}
                      </span>
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}

              {pages.length > 0 ? (
                <Command.Group heading="Pages">
                  {pages.map((page) => (
                    <Command.Item
                      key={page.path}
                      value={`page:${page.path}`}
                      onSelect={() => pick(() => onOpenDoc(page.path))}
                      className="pi"
                    >
                      <FileText className="i" aria-hidden />
                      <div className="min-w-0">
                        <div className="t">
                          <Marked text={page.path.split("/").pop() ?? page.path} query={trimmed} />
                        </div>
                        <div className="sn mono">{page.path}</div>
                      </div>
                      <span className="meta">page</span>
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}

              {trimmed.length > 0 ? (
                <Command.Group heading="Query">
                  <Command.Item
                    value="query:run"
                    onSelect={() => pick(() => onNavigate({ name: "search", q: trimmed }))}
                    className="pi"
                  >
                    <Search className="i" aria-hidden />
                    <div className="min-w-0">
                      <div className="t">
                        {isQuery ? (
                          <>
                            Run DQL <span className="mono">{trimmed}</span>
                          </>
                        ) : (
                          <>Search everything for “{trimmed}”</>
                        )}
                      </div>
                      <div className="sn">
                        {isQuery
                          ? "Same language the sidebar filters compose"
                          : "Full text over titles, bodies, comments and pages"}
                      </div>
                    </div>
                    <span className="meta">↵</span>
                  </Command.Item>
                </Command.Group>
              ) : null}

              {nav.length > 0 ? (
                <Command.Group heading="Navigate">
                  {nav.map((item) => (
                    <Command.Item
                      key={item.label}
                      value={`nav:${item.label}`}
                      onSelect={() => pick(() => onNavigate(item.route))}
                      className="pi"
                    >
                      <item.icon className="i" aria-hidden />
                      <div className="t">{item.label}</div>
                      <span className="meta">{item.kbd ?? ""}</span>
                    </Command.Item>
                  ))}
                  {fuzzyMatch(search, "my issues") || fuzzyMatch(search, "mine") ? (
                    <Command.Item
                      value="nav:mine"
                      onSelect={() => pick(() => onNavigate({ name: "issues", q: mine }))}
                      className="pi"
                    >
                      <UserRound className="i" aria-hidden />
                      <div className="t">My issues</div>
                      <span className="meta" />
                    </Command.Item>
                  ) : null}
                </Command.Group>
              ) : null}

              {shownActions.length > 0 ? (
                <Command.Group heading="Actions">
                  {shownActions.map((action) => (
                    <Command.Item
                      key={action.label}
                      value={`action:${action.label}`}
                      onSelect={() => pick(action.run)}
                      className="pi"
                    >
                      <action.icon className="i" aria-hidden />
                      <div className="t">{action.label}</div>
                      <span className="meta">{action.kbd ?? ""}</span>
                    </Command.Item>
                  ))}
                </Command.Group>
              ) : null}
            </Command.List>
            <div className="pal-f">
              <span>
                Scope: {workspace}
                {all.data ? ` · ${all.data.total} issues` : ""}
                {docs.data ? ` · ${docs.data.length} pages` : ""}
              </span>
              <span className="sp" />
              <span className="k">
                <kbd>↑↓</kbd> navigate
              </span>
              <span className="k">
                <kbd>↵</kbd> open
              </span>
              <span className="k">
                <kbd>esc</kbd> close
              </span>
            </div>
          </Command>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
