// DQL search results. The query box and the examples live in the Search
// side pane (and in the URL); this view's whole job is: send the text, show
// what comes back, and show the server's parse error verbatim. Those
// messages are written to be read by humans; paraphrasing them would only
// lose information.

import { IssueTable } from "../components/IssueTable";
import { Empty, ErrorBox, Loading } from "../components/states";
import { useIssues, useSchema } from "../lib/queries";
import { ApiError } from "../lib/api";

const RESULT_LIMIT = 200;

export function SearchView({ q, onOpen }: { q: string; onOpen: (id: string) => void }) {
  const schema = useSchema();

  const trimmed = q.trim();
  // No query text -> no request. The palette and the issues view use the
  // same endpoint with their own keys, so nothing collides.
  const results = useIssues({ q: trimmed, limit: RESULT_LIMIT }, trimmed.length > 0);

  // A 400 is the parser rejecting the text, not the request failing — the
  // warn tone keeps it a coaching moment, not a red alert.
  const parseError: string | null =
    results.error instanceof ApiError && results.error.status === 400 && trimmed.length > 0
      ? results.error.message
      : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <h1 className="shrink-0 text-lg font-semibold text-zinc-100">Search</h1>
        {trimmed.length > 0 ? (
          <span
            className="max-w-[520px] truncate rounded-[3px] border border-edge bg-card px-2 py-0.5 font-mono text-[11px] text-zinc-400"
            title={trimmed}
          >
            {trimmed}
          </span>
        ) : null}
        {results.data ? (
          <span className="ml-auto shrink-0 whitespace-nowrap font-mono text-[11px] text-dim">
            {/* The list is capped for responsiveness; the count is not, so a
                capped result must say so rather than quietly drop matches. */}
            {results.data.total} {results.data.total === 1 ? "issue" : "issues"}
            {results.data.total > results.data.items.length
              ? ` · showing the first ${results.data.items.length}`
              : ""}
          </span>
        ) : null}
      </header>

      {trimmed.length === 0 ? (
        <Empty
          title="Type a DQL query in the side pane"
          hint="…or pick an example there. Fields: title, status, type, priority, assignee, label, updated. Operators: =, !=, ~, IN, AND, OR, ORDER BY, LIMIT."
          className="flex-1 justify-center"
        />
      ) : null}

      {parseError ? (
        <ErrorBox tone="warn" error={new Error(parseError)} title="The query could not be parsed" />
      ) : null}

      {trimmed.length > 0 && results.isPending ? (
        <Loading label="Searching…" className="flex-1" />
      ) : null}

      {trimmed.length > 0 && results.isError && !parseError ? (
        <ErrorBox
          error={results.error}
          title="Search failed"
          onRetry={() => void results.refetch()}
        />
      ) : null}

      {results.data && results.data.items.length === 0 ? (
        <Empty
          title="No issues match this query"
          hint="Loosen the filter, or check the field names against the examples in the side pane."
          className="flex-1 justify-center"
        />
      ) : null}

      {results.data && results.data.items.length > 0 ? (
        // Results live in a card rather than bleeding to the gutters —
        // search is a query tool, the card frames the answer.
        <div className="min-h-0 flex-1 px-4 pb-4 pt-3">
          <div className="flex h-full min-h-0 flex-col overflow-hidden rounded-[10px] border border-edge bg-panel">
            <IssueTable
              issues={results.data.items}
              statuses={schema.data?.workflow.statuses ?? []}
              onOpen={onOpen}
              columns="compact"
            />
          </div>
        </div>
      ) : null}
    </div>
  );
}
