// Home — the daily driver. Everything here is composed from the endpoints
// the other views already use (there is no dashboard endpoint, and none is
// needed): quick capture, next actions grouped by context, an epic rollup,
// and inbox triage. Waiting-on and the activity feed live in the Home side
// pane. All of it is derived (invariant 5): nothing on this screen is
// stored.

import { FormEvent, useMemo, useState } from "react";
import { Plus } from "lucide-react";
import type { ConnectionState } from "../lib/events";
import {
  useCreateIssue,
  useIssues,
  usePatchIssue,
  useSchema,
  useStatus,
} from "../lib/queries";
import {
  contextOf,
  dueText,
  dueTone,
  energyOf,
} from "../lib/format";
import type { IssueDto } from "../lib/types";
import { cn } from "../lib/cn";
import { Kbd, SectionHeading } from "../components/chrome";
import { IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";

// Real DQL, not the mock's: `~` is full-text over title/body, so set
// membership uses `=`. The queries the section headers display are the
// queries actually run.
// Urgent-first is ASC here: priority is the text p0..p4, so p0 sorts before
// p4 and DESC would float the least urgent to the top.
const NEXT_QUERY = "label = next AND assignee = @me ORDER BY priority ASC";
const INBOX_QUERY = "label = inbox ORDER BY created DESC";

// The pool powers everything that is grouped client-side (epics, open
// count). 200 issues keeps Home bounded on big workspaces; the counts are
// labeled from it, not pretended to be exhaustive. The side pane runs the
// same pool query — one fetch, two surfaces.
const POOL_LIMIT = 200;

// Handle | type | priority | title | due; energy and epic join from 1100px.
// Header-free grid, so the group label above carries the alignment.
const NEXT_GRID =
  "grid items-center gap-3 grid-cols-[56px_20px_12px_minmax(0,1fr)_76px] min-[1100px]:grid-cols-[60px_20px_12px_minmax(0,1fr)_104px_108px_76px]";

function SyncPill({ conn, dirty }: { conn: ConnectionState; dirty: boolean }) {
  const okay = conn === "live" && !dirty;
  const label =
    conn === "live"
      ? dirty
        ? "Uncommitted changes"
        : "In sync"
      : conn === "off"
        ? "Offline"
        : "Reconnecting…";
  return (
    <span
      className={cn(
        "flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11px]",
        okay
          ? "border-done-line bg-done-bg text-done-text"
          : "border-warn-line bg-warn-bg text-warn-text",
      )}
    >
      <span className={cn("size-1.5 rounded-full", okay ? "bg-done-text" : "bg-warn-text")} />
      {label}
    </span>
  );
}

function CaptureForm({ defaultStatus }: { defaultStatus: string | undefined }) {
  const [text, setText] = useState("");
  const create = useCreateIssue();

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const title = text.trim();
    if (title.length === 0) return;
    setText("");
    // One line in, one issue out. It lands in the inbox for triage with
    // nothing else pre-decided — type, priority and context come later.
    create.mutate({
      title,
      type: "task",
      ...(defaultStatus ? { status: defaultStatus } : {}),
      labels: ["inbox"],
      body: "",
    });
  };

  return (
    <form
      onSubmit={submit}
      className="flex items-center gap-2.5 rounded-lg border border-ctl bg-card px-3.5 py-3"
    >
      <Plus className="size-[18px] shrink-0 text-zinc-500" aria-hidden />
      <input
        value={text}
        onChange={(event) => setText(event.target.value)}
        placeholder="Capture anything — one line becomes an issue in the inbox"
        aria-label="Quick capture"
        className="w-full flex-1 border-none bg-transparent text-[15px] text-zinc-200 placeholder:text-zinc-600 focus:outline-none"
      />
      <Kbd>⏎</Kbd>
    </form>
  );
}

function SectionHint({ dql, action }: { dql?: string; action?: React.ReactNode }) {
  return (
    <>
      {dql ? <span className="font-mono text-[11px] text-dim">{dql}</span> : null}
      {action ? <span className="ml-auto">{action}</span> : null}
    </>
  );
}

function NextActions({
  issues,
  epicTitle,
  onOpen,
  onSearch,
}: {
  issues: IssueDto[];
  epicTitle: (id: string | null) => string;
  onOpen: (id: string) => void;
  onSearch: (q: string) => void;
}) {
  const groups = useMemo(() => {
    const map = new Map<string, IssueDto[]>();
    for (const issue of issues) {
      const key = contextOf(issue.labels) ?? "no context";
      const bucket = map.get(key);
      if (bucket) bucket.push(issue);
      else map.set(key, [issue]);
    }
    return [...map.entries()];
  }, [issues]);

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline gap-2.5">
        <SectionHeading>Next actions</SectionHeading>
        <SectionHint
          dql={`${NEXT_QUERY.split(" ORDER BY")[0]} · grouped by context`}
          action={
            <button
              type="button"
              onClick={() => onSearch(NEXT_QUERY)}
              className="text-xs text-zinc-500 hover:text-zinc-200"
            >
              Open in Search →
            </button>
          }
        />
      </div>
      {groups.map(([context, items]) => (
        <div key={context} className="flex flex-col gap-0.5">
          <div className="flex items-center gap-2 px-1 pb-1 pt-2">
            <span className="font-mono text-xs text-context">{context}</span>
            <span className="font-mono text-[11px] text-dim">{items.length}</span>
            <span className="h-px flex-1 bg-edge" />
          </div>
          {items.map((issue) => (
            <button
              key={issue.id}
              type="button"
              onClick={() => onOpen(issue.id)}
              className={cn(NEXT_GRID, "h-11 rounded-md px-2 text-left hover:bg-card")}
            >
              <IssueHandle shortRef={issue.short_ref} number={issue.number} />
              <TypeBadge type={issue.type} />
              <PriorityDot priority={issue.priority} />
              <span className="truncate text-[14.5px] text-zinc-200">{issue.title}</span>
              <span className="hidden truncate font-mono text-[11px] text-zinc-400 min-[1100px]:block">
                {energyOf(issue.labels)}
              </span>
              <span className="hidden truncate text-xs text-zinc-500 min-[1100px]:block">
                {epicTitle(issue.epic)}
              </span>
              <span className={cn("text-right text-xs", dueTone(issue.due))}>
                {dueText(issue.due)}
              </span>
            </button>
          ))}
        </div>
      ))}
      {groups.length === 0 ? (
        <p className="rounded-lg border border-dashed border-edge py-[22px] text-center text-[13px] text-dim">
          Nothing queued as next. Triage the inbox below.
        </p>
      ) : null}
    </section>
  );
}

interface EpicRollupRow {
  id: string;
  name: string;
  done: number;
  open: number;
  total: number;
  due: string | null;
}

function EpicRollup({
  pool,
  isDone,
  onSearch,
}: {
  pool: IssueDto[];
  isDone: (statusId: string) => boolean;
  onSearch: (q: string) => void;
}) {
  const byId = useMemo(() => new Map(pool.map((issue) => [issue.id, issue])), [pool]);
  const epics = useMemo(() => {
    const map = new Map<string, IssueDto[]>();
    for (const issue of pool) {
      if (issue.epic === null) continue;
      const bucket = map.get(issue.epic);
      if (bucket) bucket.push(issue);
      else map.set(issue.epic, [issue]);
    }
    const rows: EpicRollupRow[] = [];
    for (const [id, children] of map) {
      const done = children.filter((child) => isDone(child.status)).length;
      rows.push({
        id,
        name: byId.get(id)?.title ?? `epic ${id.slice(0, 8)}`,
        done,
        open: children.length - done,
        total: children.length,
        due: byId.get(id)?.due ?? null,
      });
    }
    // Most-finished first: the rollup answers "what is closest to done".
    return rows.sort((a, b) => b.done / Math.max(b.total, 1) - a.done / Math.max(a.total, 1));
  }, [pool, byId, isDone]);

  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline gap-2.5">
        <SectionHeading>Epics</SectionHeading>
        <SectionHint dql="rollup from child issues" />
      </div>
      <div className="grid grid-cols-1 gap-3 min-[860px]:grid-cols-2 min-[1280px]:grid-cols-3">
        {epics.map((epic) => {
          const pct = Math.round((epic.done / Math.max(epic.total, 1)) * 100);
          return (
            <button
              key={epic.id}
              type="button"
              // The drill-in is plain DQL over the epic id, so the card and
              // the search box agree on what "this epic" means.
              onClick={() => onSearch(`epic = "${epic.id}"`)}
              className="rounded-lg border border-edge bg-card p-4 text-left transition-colors hover:border-dim"
            >
              <div className="flex items-center gap-2">
                <span className="truncate text-[15px] font-semibold text-zinc-100">{epic.name}</span>
                <span className="rounded-full border border-doing-line bg-doing-bg px-2 py-px font-mono text-[10px] text-doing-text">
                  {epic.open} open
                </span>
              </div>
              <div className="mt-3.5 h-1 overflow-hidden rounded-full bg-edge">
                <span
                  className="block h-full rounded-full bg-accent"
                  style={{ width: `${pct}%` }}
                />
              </div>
              <div className="mt-2.5 font-mono text-[11px] text-zinc-500">
                {epic.done}/{epic.total} done · {epic.open} open
              </div>
              <div className="mt-2.5 flex justify-between text-xs text-dim">
                <span>due</span>
                <span className={cn("font-mono", dueTone(epic.due))}>{dueText(epic.due)}</span>
              </div>
            </button>
          );
        })}
      </div>
      {epics.length === 0 ? (
        <p className="rounded-lg border border-dashed border-edge py-[22px] text-center text-[13px] text-dim">
          No epics yet — set an issue&apos;s epic field and its children roll up here.
        </p>
      ) : null}
    </section>
  );
}

function InboxRow({
  issue,
  me,
  doneStatus,
}: {
  issue: IssueDto;
  me: string | null;
  doneStatus: string | undefined;
}) {
  const patch = usePatchIssue(issue.id);

  // The labels patch replaces the whole array, so every triage action sends
  // the full target set: inbox always comes off, the destination goes on.
  // Next additionally claims the issue for me — the query that feeds this
  // screen is `assignee = @me`.
  const retag = (add: string[], status?: string) => {
    const labels = issue.labels
      .filter((label) => label !== "inbox" && label !== "next" && label !== "someday")
      .concat(add);
    patch.mutate({
      labels,
      ...(status ? { status } : {}),
      ...(add.includes("next") && me ? { assignees: [me] } : {}),
    });
  };

  const actionClass =
    "rounded-md border border-ctl px-2.5 py-1 text-xs text-zinc-300 hover:border-zinc-400 hover:text-zinc-100 disabled:opacity-50";

  return (
    <div className="flex items-center gap-3 rounded-lg border border-edge bg-card/50 px-3.5 py-3 transition-colors hover:border-ctl">
      <IssueHandle shortRef={issue.short_ref} number={issue.number} />
      <span className="min-w-0 flex-1 truncate text-[14.5px] text-zinc-200" title={issue.title}>
        {issue.title}
      </span>
      <span className="flex gap-1.5">
        <button
          type="button"
          disabled={patch.isPending}
          onClick={() => retag(["next"])}
          className={actionClass}
        >
          Next
        </button>
        <button
          type="button"
          disabled={patch.isPending}
          onClick={() => retag(["someday"])}
          className={actionClass}
        >
          Someday
        </button>
        {doneStatus ? (
          <button
            type="button"
            disabled={patch.isPending}
            onClick={() => retag([], doneStatus)}
            className={actionClass}
          >
            Done
          </button>
        ) : null}
      </span>
    </div>
  );
}

function InboxSection({
  issues,
  me,
  doneStatus,
}: {
  issues: IssueDto[];
  me: string | null;
  doneStatus: string | undefined;
}) {
  return (
    <section className="flex flex-col gap-2.5">
      <div className="flex items-baseline gap-2.5">
        <SectionHeading>Inbox</SectionHeading>
        <SectionHint dql="label = inbox · triage without leaving Home" />
      </div>
      <div className="flex flex-col gap-2">
        {issues.map((issue) => (
          <InboxRow key={issue.id} issue={issue} me={me} doneStatus={doneStatus} />
        ))}
        {issues.length === 0 ? (
          <p className="rounded-lg border border-dashed border-edge py-[22px] text-center text-[13px] text-dim">
            Inbox clear.
          </p>
        ) : null}
      </div>
    </section>
  );
}

export function HomeView({
  conn,
  onOpen,
  onSearch,
}: {
  conn: ConnectionState;
  onOpen: (id: string) => void;
  onSearch: (q: string) => void;
}) {
  const status = useStatus();
  const schema = useSchema();
  const pool = useIssues({ limit: POOL_LIMIT });
  const next = useIssues({ q: NEXT_QUERY, limit: 100 });
  const inbox = useIssues({ q: INBOX_QUERY, limit: 50 });

  const me = status.data?.me ?? null;
  const firstStatus = schema.data?.workflow.statuses[0]?.id;
  const doneStatus = schema.data?.workflow.statuses.find((s) => s.category === "done")?.id;

  const statusCategory = useMemo(() => {
    const map = new Map<string, string>();
    for (const s of schema.data?.workflow.statuses ?? []) map.set(s.id, s.category);
    return map;
  }, [schema.data]);
  const isDone = (statusId: string) => statusCategory.get(statusId) === "done";

  const items = pool.data?.items ?? [];
  const byId = useMemo(() => new Map(items.map((issue) => [issue.id, issue])), [items]);
  const epicTitle = (id: string | null) => (id ? (byId.get(id)?.title ?? null) : null) ?? "—";
  const openCount = items.filter((issue) => !isDone(issue.status)).length;

  if (pool.isPending) {
    return <Loading label="Loading workspace…" className="flex-1 items-center justify-center" />;
  }
  if (pool.isError) {
    return <ErrorBox error={pool.error} onRetry={() => void pool.refetch()} />;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3.5 border-b border-edge px-[22px] py-3.5">
        <h1 className="text-xl font-semibold tracking-[-0.01em] text-zinc-100">Home</h1>
        <SyncPill conn={conn} dirty={status.data?.dirty ?? false} />
        <span className="ml-auto font-mono text-[11px] text-dim">
          {openCount} open · {next.data?.total ?? 0} next · {inbox.data?.total ?? 0} in inbox
        </span>
      </header>

      <div className="flex min-w-0 flex-1 flex-col gap-[26px] overflow-y-auto px-[22px] pb-10 pt-[22px]">
        <CaptureForm defaultStatus={firstStatus} />

        {next.isError ? (
          <ErrorBox error={next.error} title="Could not load next actions" tone="warn" />
        ) : (
          <NextActions
            issues={next.data?.items ?? []}
            epicTitle={epicTitle}
            onOpen={onOpen}
            onSearch={onSearch}
          />
        )}

        <EpicRollup pool={items} isDone={isDone} onSearch={onSearch} />

        {inbox.isError ? (
          <ErrorBox error={inbox.error} title="Could not load the inbox" tone="warn" />
        ) : (
          <InboxSection
            issues={inbox.data?.items ?? []}
            me={me}
            doneStatus={doneStatus}
          />
        )}
      </div>
    </div>
  );
}
