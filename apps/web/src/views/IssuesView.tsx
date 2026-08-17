// All issues in one dense table, newest activity first. Sorting and
// virtualization live in the shared IssueTable; this view owns fetching,
// the count line, and the three states every view owes the user.

import { RefreshCw } from "lucide-react";
import { IssueTable } from "../components/IssueTable";
import { Empty, ErrorBox, Loading } from "../components/states";
import { useIssues, useSchema } from "../lib/queries";

// The table is virtualized, so a large page size costs DOM rows only for
// what is on screen. Enough for a v0.1 workspace; paging comes later.
const PAGE_SIZE = 500;

export function IssuesView({ onOpen }: { onOpen: (id: string) => void }) {
  const issues = useIssues({ limit: PAGE_SIZE });
  const schema = useSchema();

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-zinc-800 px-3 py-2">
        <h1 className="text-sm font-semibold text-zinc-200">Issues</h1>
        {issues.data ? (
          <span className="font-mono text-[11px] text-zinc-500 tabular-nums">
            {issues.data.total} total
            {issues.data.total > issues.data.items.length
              ? ` — showing first ${issues.data.items.length}`
              : ""}
          </span>
        ) : null}
        <button
          type="button"
          onClick={() => void issues.refetch()}
          title="Refresh"
          className="ml-auto rounded p-1 text-zinc-500 hover:bg-zinc-900 hover:text-zinc-300"
        >
          <RefreshCw className={issues.isFetching ? "size-4 animate-spin" : "size-4"} aria-hidden />
        </button>
      </header>

      {issues.isPending ? <Loading label="Loading issues…" className="flex-1" /> : null}
      {issues.isError ? (
        <ErrorBox
          error={issues.error}
          title="Could not load issues"
          onRetry={() => void issues.refetch()}
        />
      ) : null}
      {issues.data && issues.data.items.length === 0 ? (
        <Empty
          title="No issues yet"
          hint="Create the first one with ⌘K → New issue."
          className="flex-1 justify-center"
        />
      ) : null}
      {issues.data && issues.data.items.length > 0 ? (
        <IssueTable
          issues={issues.data.items}
          statuses={schema.data?.workflow.statuses ?? []}
          onOpen={onOpen}
        />
      ) : null}
    </div>
  );
}
