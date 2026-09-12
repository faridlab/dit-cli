// The Home sidebar section: the glanceable layer of the dashboard — what is
// blocked on other people, and the workspace activity feed. Both read the
// same pool query the Home view runs (identical cache key), so the pane and
// the dashboard can never disagree, and both are derived (invariant 5):
// nothing here is stored.

import { useMemo } from "react";
import {
  useActivity,
  useIssues,
  useSchema,
  useStatus,
} from "../../lib/queries";
import { circleColor, initials, relativeTime } from "../../lib/format";
import type { IssueDto } from "../../lib/types";
import { cn } from "../../lib/cn";
import { SectionHeading } from "../chrome";
import { ErrorBox, Loading } from "../states";

// Must match HomeView's pool size: the two components share one fetch.
const POOL_LIMIT = 200;

function WaitingOn({
  pool,
  blockedStatusIds,
  me,
  onOpen,
}: {
  pool: IssueDto[];
  blockedStatusIds: ReadonlySet<string>;
  me: string | null;
  onOpen: (id: string) => void;
}) {
  // "Blocked" is workflow-defined, not a client concept: only statuses whose
  // id or label says blocked/waiting count. No such status, no section.
  const waiting = useMemo(() => {
    if (blockedStatusIds.size === 0) return [];
    return pool
      .filter(
        (issue) =>
          blockedStatusIds.has(issue.status) &&
          (me === null || !issue.assignees.includes(me)),
      )
      .slice(0, 3);
  }, [pool, blockedStatusIds, me]);

  if (blockedStatusIds.size === 0) return null;

  return (
    <section>
      <SectionHeading size="sm" className="mb-3">
        Waiting on
      </SectionHeading>
      <div className="flex flex-col gap-2.5">
        {waiting.map((issue) => {
          const first = issue.assignees[0];
          return (
            <button
              key={issue.id}
              type="button"
              onClick={() => onOpen(issue.id)}
              className="flex items-start gap-2.5 text-left"
            >
              <span
                className={cn(
                  "inline-flex size-[22px] shrink-0 items-center justify-center rounded-full font-mono text-[9px] leading-none text-white",
                  first ? circleColor(first) : "bg-dim",
                )}
              >
                {first ? initials(first) : "?"}
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13.5px] leading-snug text-ink-2">
                  {issue.title}
                </span>
                <span className="mt-0.5 block font-mono text-[11px] text-dim">
                  {issue.number !== null ? `#${issue.number}` : issue.short_ref} ·{" "}
                  {relativeTime(issue.updated)}
                </span>
              </span>
            </button>
          );
        })}
        {waiting.length === 0 ? (
          <p className="text-xs text-dim">Nothing blocked on someone else.</p>
        ) : null}
      </div>
    </section>
  );
}

function ActivitySection() {
  // One windowed request over the whole workspace, ordered by `seq` on the
  // server — the only order that is not self-contradictory (invariant 9).
  // The full stream, with filters and time travel, is the Timeline screen.
  const activity = useActivity({ limit: 15 });

  return (
    <section>
      <div className="mb-2.5 flex items-center gap-2">
        <SectionHeading size="sm">Activity</SectionHeading>
        <a
          href="#/timeline"
          className="ml-auto text-[11px] text-muted transition-colors hover:text-ink"
        >
          Timeline →
        </a>
      </div>
      {activity.isPending ? (
        <p className="text-xs text-faint">Loading…</p>
      ) : activity.isError ? (
        <p className="text-xs text-crit-text">
          {activity.error instanceof Error ? activity.error.message : "Could not load activity"}
        </p>
      ) : (activity.data?.events.length ?? 0) === 0 ? (
        <p className="text-xs text-faint">No field events yet.</p>
      ) : (
        <ol className="flex flex-col">
          {(activity.data?.events ?? []).map((event) => (
            <li
              key={`${event.issue_id}-${event.seq}-${event.field}`}
              className="flex flex-col gap-0.5 border-l border-edge py-2 pl-3"
            >
              <div className="flex items-baseline gap-2 text-[11px] text-muted">
                <span className="font-mono text-dim">
                  {event.number !== null ? `#${event.number}` : event.short_ref}
                </span>
                <span className="font-mono text-ink-2">{event.author}</span>
                <span className="ml-auto">{relativeTime(event.ts)}</span>
              </div>
              <p className="text-[13px] text-ink-2">{event.field}</p>
              <p className="font-mono text-[11px] text-muted">
                {event.old_value ?? "∅"} → {event.new_value ?? "∅"}
              </p>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

export function HomePane({ onOpen }: { onOpen: (id: string) => void }) {
  const status = useStatus();
  const schema = useSchema();
  const pool = useIssues({ limit: POOL_LIMIT });

  const me = status.data?.me ?? null;
  // "Blocked" is whatever the workspace's workflow calls blocked — matched
  // on the status's own id and label, never a hardcoded list.
  const blockedStatusIds = useMemo(() => {
    const set = new Set<string>();
    for (const s of schema.data?.workflow.statuses ?? []) {
      if (/block|wait/i.test(`${s.id} ${s.label}`)) set.add(s.id);
    }
    return set;
  }, [schema.data]);

  if (pool.isPending) {
    return <Loading label="Loading workspace…" className="p-4" />;
  }
  if (pool.isError) {
    return (
      <div className="p-2">
        <ErrorBox
          error={pool.error}
          onRetry={() => void pool.refetch()}
          title="Could not load the workspace"
        />
      </div>
    );
  }

  const items = pool.data?.items ?? [];

  return (
    <div className="flex flex-col gap-[22px] px-[18px] pb-7 pt-4">
      <WaitingOn pool={items} blockedStatusIds={blockedStatusIds} me={me} onOpen={onOpen} />
      <ActivitySection />
    </div>
  );
}
