// DQL search. The query language belongs to the server, so this view's only
// intelligence is: send the text, show what comes back, and — importantly —
// show the server's parse error verbatim. Those messages are written to be
// read by humans; paraphrasing them would only lose information.

import { type FormEvent, useEffect, useState } from "react";
import { Search } from "lucide-react";
import { IssueTable } from "../components/IssueTable";
import { Empty, ErrorBox, Loading } from "../components/states";
import { useIssues, useSchema } from "../lib/queries";
import { ApiError } from "../lib/api";

// Chips double as documentation: they are the fastest way to learn a query
// language — steal a working example, edit it.
const EXAMPLES: Array<{ label: string; dql: string }> = [
  { label: "My open work", dql: "status != done AND assignees ~ @me" },
  { label: "Recent in auth/api", dql: "label IN (auth, api) AND updated > -7d" },
  { label: "Hot bugs", dql: "type = bug AND priority IN (urgent, high)" },
  { label: "Title contains “login”", dql: "title ~ login ORDER BY updated DESC LIMIT 20" },
];

const RESULT_LIMIT = 200;

export function SearchView({
  q,
  onSearch,
  onOpen,
}: {
  q: string;
  onSearch: (q: string) => void;
  onOpen: (id: string) => void;
}) {
  const [input, setInput] = useState(q);
  const schema = useSchema();

  // Following a palette jump or chip click the route changes; keep the text
  // box in sync without fighting the typist (only when not focused).
  useEffect(() => {
    setInput(q);
  }, [q]);

  const trimmed = q.trim();
  // No query text -> no request. The palette and the issues view use the
  // same endpoint with their own keys, so nothing collides.
  const results = useIssues({ q: trimmed, limit: RESULT_LIMIT }, trimmed.length > 0);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    onSearch(input);
  };

  const dqlError =
    results.error instanceof ApiError && results.error.status === 400 && trimmed.length > 0
      ? results.error.message
      : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <form onSubmit={submit} className="border-b border-zinc-800 px-3 py-2">
        <div className="flex items-center gap-2">
          <Search className="size-4 shrink-0 text-zinc-500" aria-hidden />
          <input
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder='Try: status != done AND assignee = @me'
            aria-label="DQL query"
            spellCheck={false}
            className="h-8 flex-1 rounded border border-zinc-700 bg-zinc-950 px-2 font-mono text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none"
          />
          <button
            type="submit"
            className="h-8 rounded bg-sky-700 px-3 text-xs font-medium text-white hover:bg-sky-600"
          >
            Search
          </button>
        </div>
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          {EXAMPLES.map((example) => (
            <button
              key={example.dql}
              type="button"
              onClick={() => onSearch(example.dql)}
              title={example.dql}
              className="rounded border border-zinc-800 bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-400 hover:border-zinc-600 hover:text-zinc-200"
            >
              {example.label}
            </button>
          ))}
        </div>
      </form>

      {trimmed.length === 0 ? (
        <Empty
          title="Type a DQL query or pick an example"
          hint="Fields: title, status, type, priority, assignee, label, updated. Operators: =, !=, ~, IN, AND, OR, ORDER BY, LIMIT."
          className="flex-1 justify-center"
        />
      ) : null}

      {dqlError ? (
        <div
          role="alert"
          className="m-3 rounded-md border border-amber-900/70 bg-amber-950/30 p-3 text-sm"
        >
          <p className="font-medium text-amber-300">The query could not be parsed</p>
          <p className="mt-1 font-mono text-xs text-amber-200/80">{dqlError}</p>
        </div>
      ) : null}

      {trimmed.length > 0 && results.isPending ? (
        <Loading label="Searching…" className="flex-1" />
      ) : null}

      {trimmed.length > 0 && results.isError && !dqlError ? (
        <ErrorBox
          error={results.error}
          title="Search failed"
          onRetry={() => void results.refetch()}
        />
      ) : null}

      {results.data ? (
        <>
          <div className="flex items-center gap-2 px-3 py-1.5 text-[11px] text-zinc-500">
            <span className="font-mono tabular-nums">
              {results.data.total} {results.data.total === 1 ? "issue" : "issues"}
            </span>
            {/* The list is capped for responsiveness; the count is not, so a
                capped result must say so rather than quietly drop matches. */}
            {results.data.total > results.data.items.length ? (
              <span className="text-zinc-600">
                showing the first {results.data.items.length} — narrow the query to see the rest
              </span>
            ) : null}
            <span className="truncate font-mono text-zinc-600">q: {trimmed}</span>
          </div>
          {results.data.items.length === 0 ? (
            <Empty
              title="No issues match this query"
              hint="Loosen the filter, or check the field names against the examples above."
              className="flex-1 justify-center"
            />
          ) : (
            <IssueTable
              issues={results.data.items}
              statuses={schema.data?.workflow.statuses ?? []}
              onOpen={onOpen}
            />
          )}
        </>
      ) : null}
    </div>
  );
}
