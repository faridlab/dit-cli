// All issues in one dense table. Sorting and virtualization live in the
// shared IssueTable; this view owns fetching, the filter chips (which compose
// real DQL — the same language the search box speaks), and bulk status edits.

import { useMemo, useState } from "react";
import { RefreshCw } from "lucide-react";
import { ContextChip } from "../components/chrome";
import { IssueTable } from "../components/IssueTable";
import { Empty, ErrorBox, Loading } from "../components/states";
import { useBulkPatchIssue, useIssues, useSchema, useStatus } from "../lib/queries";
import { contextOf } from "../lib/format";
import type { FieldPatch } from "../lib/types";

// The table is virtualized, so a large page size costs DOM rows only for
// what is on screen. Enough for a v0.1 workspace; paging comes later.
const PAGE_SIZE = 500;

export function IssuesView({ onOpen }: { onOpen: (id: string) => void }) {
  const schema = useSchema();
  const status = useStatus();
  const bulk = useBulkPatchIssue();

  const [mine, setMine] = useState(false);
  const [contexts, setContexts] = useState<ReadonlySet<string>>(new Set());
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());

  const me = status.data?.me ?? null;

  // The chips are not a private filter language: they compose the exact DQL
  // a power user would type, so what the list shows is always inspectable.
  const q = useMemo(() => {
    const parts: string[] = [];
    if (mine && me !== null) parts.push("assignee = @me");
    for (const context of contexts) parts.push(`label = context:${context}`);
    return parts.length > 0 ? parts.join(" AND ") : undefined;
  }, [mine, me, contexts]);

  const issues = useIssues({ q, limit: PAGE_SIZE });

  // Contexts come from the labels actually present in the workspace — the
  // workflow defines statuses, the issues themselves define contexts.
  const availableContexts = useMemo(() => {
    const set = new Set<string>();
    for (const issue of issues.data?.items ?? []) {
      const context = contextOf(issue.labels);
      if (context !== null) set.add(context);
    }
    return [...set].sort();
  }, [issues.data]);

  const toggleContext = (context: string) => {
    setContexts((previous) => {
      const next = new Set(previous);
      if (next.has(context)) next.delete(context);
      else next.add(context);
      return next;
    });
  };

  const toggleSelect = (id: string) => {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  // Bulk targets are workflow-defined, so the buttons say what the workspace
  // calls the status — and disappear when the workflow lacks a category.
  const doingStatus = schema.data?.workflow.statuses.find((s) => s.category === "doing");
  const doneStatus = schema.data?.workflow.statuses.find((s) => s.category === "done");

  const applyBulk = (statusId: string) => {
    const edits = [...selected].map((id) => ({
      id,
      set: { status: statusId } satisfies FieldPatch,
    }));
    bulk.mutate(edits, { onSuccess: () => setSelected(new Set()) });
  };

  const actionClass =
    "rounded-md border border-ctl px-2.5 py-1 text-xs text-zinc-300 hover:border-zinc-400 hover:text-zinc-100 disabled:opacity-50";

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex flex-wrap items-center gap-2.5 border-b border-edge px-5 py-3">
        <h1 className="shrink-0 text-lg font-semibold text-zinc-100">Issues</h1>
        <span className="shrink-0 whitespace-nowrap font-mono text-[11px] text-dim">
          {issues.data ? issues.data.items.length : 0} shown
          {issues.data && issues.data.total > issues.data.items.length
            ? ` of ${issues.data.total}`
            : ""}
        </span>
        <span className="ml-auto flex flex-wrap items-center gap-2">
          <ContextChip
            on={mine && me !== null}
            onClick={() => setMine((value) => !value)}
            title={me === null ? "No git identity configured for @me" : "assignee = @me"}
          >
            @me
          </ContextChip>
          {availableContexts.map((context) => (
            <ContextChip
              key={context}
              on={contexts.has(context)}
              onClick={() => toggleContext(context)}
            >
              {context}
            </ContextChip>
          ))}
          <button
            type="button"
            onClick={() => void issues.refetch()}
            title="Refresh"
            className="rounded p-1 text-zinc-500 hover:bg-card hover:text-zinc-300"
          >
            <RefreshCw
              className={issues.isFetching ? "size-4 animate-spin" : "size-4"}
              aria-hidden
            />
          </button>
        </span>
      </header>

      {selected.size > 0 ? (
        <div className="flex items-center gap-3 border-b border-edge bg-card px-5 py-2.5">
          <span className="text-[13px] text-zinc-200">{selected.size} selected</span>
          <span className="flex gap-2">
            {doingStatus ? (
              <button
                type="button"
                disabled={bulk.isPending}
                onClick={() => applyBulk(doingStatus.id)}
                className={actionClass}
              >
                Status → {doingStatus.label}
              </button>
            ) : null}
            {doneStatus ? (
              <button
                type="button"
                disabled={bulk.isPending}
                onClick={() => applyBulk(doneStatus.id)}
                className={actionClass}
              >
                Status → {doneStatus.label}
              </button>
            ) : null}
            <button
              type="button"
              disabled={bulk.isPending}
              onClick={() => setSelected(new Set())}
              className={actionClass}
            >
              Clear
            </button>
          </span>
          <span className="ml-auto text-[11.5px] text-dim">
            one patch per issue — one commit per file
          </span>
        </div>
      ) : null}

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
          title={q ? "No issues match these filters." : "No issues yet"}
          hint={q ? "Drop a chip, or clear them all." : "Create the first one with ⌘K → New issue."}
          className="flex-1 justify-center"
        />
      ) : null}
      {issues.data && issues.data.items.length > 0 ? (
        <IssueTable
          issues={issues.data.items}
          statuses={schema.data?.workflow.statuses ?? []}
          onOpen={onOpen}
          selectable
          selected={selected}
          onToggleSelect={toggleSelect}
        />
      ) : null}
    </div>
  );
}
