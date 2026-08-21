// The Issues side pane: the list's filters. Each toggle composes the exact
// DQL a power user would type (the same language the search box speaks) and
// puts it in the URL, so a filtered list is a shareable, reloadable thing
// rather than private view state. The pane re-parses its own canonical
// output, so chips and URL can never drift apart.

import { useMemo } from "react";
import { useIssues, useStatus } from "../../lib/queries";
import { contextOf } from "../../lib/format";
import { cn } from "../../lib/cn";
import { CheckSquare, SectionHeading } from "../chrome";

const MINE_FRAGMENT = "assignee = @me";
const CONTEXT_RE = /label = context:([a-z0-9-]+)/g;

function composeQuery(mine: boolean, contexts: ReadonlySet<string>): string | null {
  const parts: string[] = [];
  if (mine) parts.push(MINE_FRAGMENT);
  for (const context of [...contexts].sort()) parts.push(`label = context:${context}`);
  return parts.length > 0 ? parts.join(" AND ") : null;
}

function FilterRow({
  label,
  on,
  onClick,
  title,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  title?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className={cn(
        "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left font-mono text-xs transition-colors hover:bg-card",
        on ? "bg-white/[0.03] text-zinc-100" : "text-zinc-400",
      )}
    >
      <CheckSquare on={on} />
      <span className="truncate">{label}</span>
    </button>
  );
}

export function IssuesPane({
  q,
  onFilter,
}: {
  q: string | null;
  onFilter: (q: string | null) => void;
}) {
  const status = useStatus();
  const me = status.data?.me ?? null;

  const mine = q !== null && q.includes(MINE_FRAGMENT);
  const contexts = useMemo(
    () =>
      new Set(
        [...(q ?? "").matchAll(CONTEXT_RE)].map((match) => match[1] ?? ""),
      ),
    [q],
  );

  // Contexts come from the labels present in the workspace, not from the
  // filtered list, so picking one filter never hides the others.
  const unfiltered = useIssues({ limit: 500 });
  const availableContexts = useMemo(() => {
    const set = new Set<string>();
    for (const issue of unfiltered.data?.items ?? []) {
      const context = contextOf(issue.labels);
      if (context !== null) set.add(context);
    }
    return [...set].sort();
  }, [unfiltered.data]);

  const toggleMine = () => onFilter(composeQuery(!mine, contexts));
  const toggleContext = (context: string) => {
    const next = new Set(contexts);
    if (next.has(context)) next.delete(context);
    else next.add(context);
    onFilter(composeQuery(mine, next));
  };

  return (
    <div className="flex flex-col gap-4 p-3">
      <section>
        <div className="flex items-center gap-2 px-1 pb-2">
          <SectionHeading size="sm">Filters</SectionHeading>
          {q !== null ? (
            <button
              type="button"
              onClick={() => onFilter(null)}
              className="ml-auto text-[11px] text-zinc-500 transition-colors hover:text-zinc-200"
            >
              Clear all
            </button>
          ) : null}
        </div>
        <FilterRow
          label="@me"
          on={mine && me !== null}
          onClick={toggleMine}
          title={me === null ? "No git identity configured for @me" : MINE_FRAGMENT}
        />
        {availableContexts.map((context) => (
          <FilterRow
            key={context}
            label={context}
            on={contexts.has(context)}
            onClick={() => toggleContext(context)}
            title={`label = context:${context}`}
          />
        ))}
      </section>

      {/* The live query, verbatim: what the list shows is always
          inspectable, never a private filter language. */}
      {q !== null ? (
        <p
          className="rounded-md border border-edge bg-card px-2.5 py-2 font-mono text-[10.5px] leading-relaxed text-zinc-500"
          title="The exact query the list runs"
        >
          {q}
        </p>
      ) : null}
    </div>
  );
}
