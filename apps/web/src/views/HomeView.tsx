// Home — the daily driver. Everything on it is composed from the endpoints
// the other views already use (there is no dashboard endpoint, and none is
// needed): quick capture, next actions grouped by context, an epic rollup,
// and inbox triage. Waiting-on and the activity feed live in the Home side
// pane (HomePane). All of it is derived (invariant 5): nothing on this
// screen is stored.
//
// The markup follows the approved design's class recipes (styles.css,
// "Workbench recipes": .home .capture .group .glbl .irow .epics .epic .bar
// .inbox .tri) so the pixels match the design by construction.

import { useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { Plus } from "lucide-react";
import type { ConnectionState } from "../lib/events";
import { useCreateIssue, useIssues, useOpenPool, usePatchIssue, useSchema, useStatus } from "../lib/queries";
import { contextOf, dueInfo } from "../lib/format";
import type { IssueDto, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";
import { Kbd, Sp } from "../components/chrome";
import { AssigneeCircles, Chip, IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";
import { mineQuery, openQuery } from "../lib/dql";
import {
  INBOX_DEFINITION,
  NEXT_DEFINITION,
  POOL_LIMIT,
  byPriority,
  contextsOf,
  doneIds,
  isInbox,
  isNext,
} from "../lib/lists";
import { routeToHash, useRoute } from "../lib/router";
import { useRegisterPeekList } from "../lib/peeklist";

/** The real query behind "Next actions", runnable on the Search page. The
 *  open-status part is spelled out from the workflow because DQL has no
 *  `status != done` category test. */
function nextSearchQuery(statuses: readonly StatusDto[] | undefined): string {
  const open = openQuery(statuses);
  return [open, 'assignee = @me AND label ~ "context:"'].filter(Boolean).join(" AND ");
}

/** One list row: handle · type · priority · title (+ epic) · due · @context · owners.
 *  Every cell is always rendered — the row is a 7-column grid and a missing
 *  cell would shift the ones after it. */
function IssueRow({
  issue,
  epicTitle,
  selected,
  onOpen,
}: {
  issue: IssueDto;
  epicTitle: string | null;
  selected: boolean;
  onOpen: (id: string) => void;
}) {
  const due = dueInfo(issue.due);
  return (
    <button type="button" className={cn("irow", selected && "sel")} onClick={() => onOpen(issue.short_ref)}>
      <IssueHandle shortRef={issue.short_ref} number={issue.number} />
      <TypeBadge type={issue.type} />
      <PriorityDot priority={issue.priority} />
      <span className="t">
        {issue.title}
        {epicTitle ? <small>{epicTitle}</small> : null}
      </span>
      <span className={cn("due", due?.cls)} title={issue.due ?? undefined}>
        {due?.text ?? ""}
      </span>
      <span className="labels" style={{ display: "flex", gap: 4 }}>
        {issue.labels
          .filter((label) => label.startsWith("context:"))
          .map((label) => (
            <Chip key={label} label={label} />
          ))}
      </span>
      <AssigneeCircles assignees={issue.assignees} />
    </button>
  );
}

function Capture({
  firstStatus,
  onOpen,
}: {
  firstStatus: StatusDto | undefined;
  onOpen: (id: string) => void;
}) {
  const [text, setText] = useState("");
  const create = useCreateIssue();

  const submit = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    const title = text.trim();
    if (title.length === 0 || create.isPending) return;
    event.preventDefault();
    const andOpen = event.shiftKey;
    setText("");
    // One line in, one issue out: a task in the workflow's first status with
    // nothing else pre-decided. Without an owner or a @context it shows up in
    // the inbox below, which is where it gets refined.
    create.mutate(
      {
        title,
        type: "task",
        ...(firstStatus ? { status: firstStatus.id } : {}),
        labels: [],
        body: "",
      },
      {
        onSuccess: (created) => {
          if (andOpen) onOpen(created.short_ref);
        },
      },
    );
  };

  return (
    <div className="capture">
      <Plus className="i" aria-hidden />
      <input
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={submit}
        placeholder={`Capture an issue — title, then Enter. Lands in ${firstStatus?.label ?? "Backlog"}, refine it later.`}
        aria-label="Quick capture"
      />
      <span className="hint">
        <Kbd>↵</Kbd> create <Kbd>⇧↵</Kbd> create and open
      </span>
    </div>
  );
}

function EpicCard({
  epic,
  kids,
  done,
  doing,
  onOpen,
}: {
  epic: IssueDto;
  /** Every issue whose `epic` points at this one. */
  kids: readonly IssueDto[];
  done: ReadonlySet<string>;
  doing: ReadonlySet<string>;
  onOpen: (id: string) => void;
}) {
  const total = kids.length;
  const finished = kids.filter((child) => done.has(child.status)).length;
  const inFlight = kids.filter((child) => doing.has(child.status)).length;
  const pct = (count: number) => (total > 0 ? `${(count / total) * 100}%` : "0%");
  return (
    <button type="button" className="epic" onClick={() => onOpen(epic.short_ref)}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <IssueHandle shortRef={epic.short_ref} number={epic.number} />
        <TypeBadge type="story" />
        <span className="sp" style={{ flex: 1 }} />
        <AssigneeCircles assignees={epic.assignees} />
      </div>
      <div className="t">{epic.title}</div>
      <div className="bar">
        <i className="d" style={{ width: pct(finished) }} />
        <i className="g" style={{ width: pct(inFlight) }} />
      </div>
      <div className="m">
        <span>
          {finished} of {total} done
        </span>
        <span>{inFlight} in flight</span>
      </div>
    </button>
  );
}

/** An inbox row with the triage buttons that appear on hover. The row is a
 *  div acting as a button because the triage controls inside it are real
 *  buttons, and a button cannot contain buttons. */
function InboxRow({
  issue,
  me,
  context,
  doneStatus,
  selected,
  onOpen,
}: {
  issue: IssueDto;
  me: string | null;
  context: string;
  doneStatus: StatusDto | undefined;
  selected: boolean;
  onOpen: (id: string) => void;
}) {
  const patch = usePatchIssue(issue.id);
  const stop = (event: React.SyntheticEvent) => event.stopPropagation();
  return (
    <div
      role="button"
      tabIndex={0}
      className={cn("irow", selected && "sel")}
      onClick={() => onOpen(issue.short_ref)}
      onKeyDown={(event) => {
        if (event.target !== event.currentTarget) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onOpen(issue.short_ref);
        }
      }}
    >
      <IssueHandle shortRef={issue.short_ref} number={issue.number} />
      <TypeBadge type={issue.type} />
      <PriorityDot priority={issue.priority} />
      <span className="t">{issue.title}</span>
      <span className="tri" onClick={stop} onKeyDown={stop}>
        {me ? (
          <button
            type="button"
            disabled={patch.isPending}
            title={`Assign to ${me}`}
            onClick={() => patch.mutate({ assignees: [...new Set([...issue.assignees, me])] })}
          >
            Take
          </button>
        ) : null}
        <button
          type="button"
          disabled={patch.isPending}
          title={`Add label context:${context}`}
          onClick={() => patch.mutate({ labels: [...issue.labels, `context:${context}`] })}
        >
          @{context}
        </button>
        {doneStatus ? (
          <button
            type="button"
            disabled={patch.isPending}
            title={`Set status ${doneStatus.label}`}
            onClick={() => patch.mutate({ status: doneStatus.id })}
          >
            Done
          </button>
        ) : null}
      </span>
    </div>
  );
}

function Section({ className, children }: { className?: string; children: ReactNode }) {
  return <section className={cn("group", className)}>{children}</section>;
}

export function HomeView(props: {
  /** Kept for the shell's call site; the sync state is drawn by the status bar. */
  conn: ConnectionState;
  onOpen: (id: string) => void;
  onSearch: (q: string) => void;
}) {
  const { onOpen } = props;
  const status = useStatus();
  const schema = useSchema();
  const open = useOpenPool();
  // The full pool including done work, so the epic progress bars count
  // finished children. Same bound as the sidebar's lists.
  const pool = useIssues({ limit: POOL_LIMIT });
  const route = useRoute();

  const me = status.data?.me ?? null;
  const statuses = schema.data?.workflow.statuses;
  const firstStatus = statuses?.[0];
  const doneStatus = statuses?.find((s) => s.category === "done");
  const done = useMemo(() => doneIds(statuses), [statuses]);
  const doing = useMemo(
    () => new Set((statuses ?? []).filter((s) => s.category === "doing").map((s) => s.id)),
    [statuses],
  );

  const openItems = open.data?.items ?? [];
  const poolItems = pool.data?.items ?? [];

  const next = useMemo(
    () => openItems.filter((issue) => isNext(issue, done, me)).sort(byPriority),
    [openItems, done, me],
  );
  const groups = useMemo(() => {
    const map = new Map<string, IssueDto[]>();
    for (const issue of next) {
      const context = contextOf(issue.labels) ?? "";
      const bucket = map.get(context);
      if (bucket) bucket.push(issue);
      else map.set(context, [issue]);
    }
    return [...map.entries()];
  }, [next]);
  const inbox = useMemo(() => openItems.filter((issue) => isInbox(issue, done)), [openItems, done]);

  // Epic titles come from whichever pool has the issue: the open pool for
  // live epics, the full pool for ones already closed.
  const titleById = useMemo(() => {
    const map = new Map<string, string>();
    for (const issue of [...poolItems, ...openItems]) map.set(issue.id, issue.title);
    return map;
  }, [poolItems, openItems]);
  const epics = useMemo(() => poolItems.filter((issue) => issue.type === "story" && issue.epic === null), [poolItems]);
  const childrenOf = useMemo(() => {
    const map = new Map<string, IssueDto[]>();
    for (const issue of poolItems) {
      if (issue.epic === null) continue;
      const bucket = map.get(issue.epic);
      if (bucket) bucket.push(issue);
      else map.set(issue.epic, [issue]);
    }
    return map;
  }, [poolItems]);

  // The triage shortcut offers the context the workspace already uses most
  // (first alphabetically); "computer" only when there is none yet.
  const triageContext = useMemo(() => contextsOf(openItems)[0] ?? "computer", [openItems]);

  // Down the page: next actions first, then the inbox — the order the issue
  // panel walks with J/K. The epic cards are entry points, not a queue.
  useRegisterPeekList(
    useMemo(() => [...next, ...inbox].map((issue) => issue.short_ref), [next, inbox]),
  );

  const selected = "issue" in route ? (route.issue ?? null) : null;
  const isSelected = (issue: IssueDto) => selected !== null && (selected === issue.short_ref || selected === issue.id);

  if (open.isPending) {
    return <Loading label="Loading workspace…" className="flex-1 items-center justify-center" />;
  }
  if (open.isError) {
    return <ErrorBox error={open.error} onRetry={() => void open.refetch()} />;
  }

  return (
    // `w-full`: the shell's content area is a flex column, and the recipe's
    // `margin: 0 auto` would otherwise shrink the column to its content
    // instead of centering it inside the 1080px cap.
    <div className="home w-full">
      <Capture firstStatus={firstStatus} onOpen={onOpen} />

      <Section>
        <div className="sec-h">
          Next actions{" "}
          <a className="dql" href={routeToHash({ name: "search", q: nextSearchQuery(statuses) })} title="Run this query">
            {NEXT_DEFINITION}
          </a>
          <Sp />
          <a
            href={routeToHash({ name: "issues", q: mineQuery(statuses) })}
            style={{ color: "var(--accent-ink)", textTransform: "none", letterSpacing: 0, fontWeight: 500 }}
          >
            All my issues →
          </a>
        </div>
        {groups.length === 0 ? (
          <p className="empty">Nothing assigned to you with a context. Take something from the inbox.</p>
        ) : (
          groups.map(([context, issues]) => (
            <div key={context} className="contents">
              <div className="glbl">
                <Chip label={`context:${context}`} />
                <b>{issues.length}</b> {issues.length === 1 ? "issue" : "issues"}
              </div>
              {issues.map((issue) => (
                <IssueRow
                  key={issue.id}
                  issue={issue}
                  epicTitle={issue.epic ? (titleById.get(issue.epic) ?? null) : null}
                  selected={isSelected(issue)}
                  onOpen={onOpen}
                />
              ))}
            </div>
          ))
        )}
      </Section>

      <Section>
        <div className="sec-h">
          Epics <span className="dql">grouped from the first {POOL_LIMIT} issues · never stored</span>
        </div>
        {pool.isError ? (
          <ErrorBox error={pool.error} title="Could not load the issue pool" tone="warn" />
        ) : epics.length === 0 ? (
          <p className="empty">No epics yet — a story without an epic of its own becomes one.</p>
        ) : (
          <div className="epics">
            {epics.map((epic) => (
              <EpicCard
                key={epic.id}
                epic={epic}
                kids={childrenOf.get(epic.id) ?? []}
                done={done}
                doing={doing}
                onOpen={onOpen}
              />
            ))}
          </div>
        )}
      </Section>

      <Section className="inbox">
        <div className="sec-h">
          Inbox <span className="dql">{INBOX_DEFINITION}</span>
          <Sp />
          <span className="mono" style={{ textTransform: "none", letterSpacing: 0, fontWeight: 400 }}>
            {inbox.length}
          </span>
        </div>
        {inbox.length === 0 ? (
          <p className="empty">Inbox zero.</p>
        ) : (
          inbox.map((issue) => (
            <InboxRow
              key={issue.id}
              issue={issue}
              me={me}
              context={triageContext}
              doneStatus={doneStatus}
              selected={isSelected(issue)}
              onOpen={onOpen}
            />
          ))
        )}
      </Section>
    </div>
  );
}
