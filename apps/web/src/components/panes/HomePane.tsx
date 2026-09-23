// The Home sidebar section: the glanceable layer of the dashboard — what is
// waiting on other people, and the workspace activity feed. The waiting
// list reads the same open pool the Home view runs (identical cache key),
// so the pane and the view can never disagree; the feed is one windowed
// request over `field_events`. Both are derived (invariant 5): nothing here
// is stored.

import { useMemo } from "react";
import { GitCommitHorizontal } from "lucide-react";
import { useActivity, useIssues, useOpenPool, useSchema } from "../../lib/queries";
import { relativeTime, resolveIdValue } from "../../lib/format";
import type { IssueDto, StatusDto } from "../../lib/types";
import { PaneSection } from "../PaneSection";
import { AssigneeCircles, IssueHandle } from "../badges";
import { routeToHash } from "../../lib/router";

/** "Waiting" is workflow-defined, not a client concept: only statuses whose
 *  id or label says review, waiting or blocked count. */
const WAITING = /review|waiting|blocked/i;

/** The fields worth a line in the feed. Body edits and comments have their
 *  own surfaces; timestamps and the like are noise here. */
const FEED_FIELDS = new Set([
  "status",
  "priority",
  "assignees",
  "labels",
  "epic",
  "estimate",
  "due",
  "start",
  "title",
]);

const FEED_LENGTH = 6;

function WaitingOn({
  pool,
  statuses,
  onOpen,
}: {
  pool: readonly IssueDto[];
  statuses: readonly StatusDto[];
  onOpen: (id: string) => void;
}) {
  const ids = useMemo(() => new Set(statuses.map((s) => s.id)), [statuses]);
  const waiting = useMemo(() => pool.filter((issue) => ids.has(issue.status)), [pool, ids]);
  // The same list as a query a person could type, for the count link.
  const query = statuses.map((s) => `status = ${s.id}`).join(" OR ");

  return (
    <PaneSection
      id="home.waiting"
      title="Waiting on"
      count={
        statuses.length > 0 ? (
          <a className="mono" href={routeToHash({ name: "search", q: query })} title={query}>
            {waiting.length}
          </a>
        ) : null
      }
    >
      <div className="sb-body">
        {waiting.length === 0 ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            Nothing in review.
          </p>
        ) : (
          waiting.map((issue) => (
            <button key={issue.id} type="button" className="row" onClick={() => onOpen(issue.short_ref)}>
              <AssigneeCircles assignees={issue.assignees} />
              <span className="lbl">{issue.title}</span>
              <span className="cnt">{relativeTime(issue.updated)}</span>
            </button>
          ))
        )}
      </div>
    </PaneSection>
  );
}

function Activity({
  statuses,
  onOpen,
}: {
  statuses: readonly StatusDto[];
  onOpen: (id: string) => void;
}) {
  // One windowed request over the whole workspace, ordered by `seq` on the
  // server — the only order that is not self-contradictory (invariant 9).
  // Fetch more than the six shown so the field filter still has enough.
  const activity = useActivity({ limit: 60 });
  const labelOf = useMemo(() => new Map(statuses.map((s) => [s.id, s.label])), [statuses]);
  // `epic` events carry an issue id; the same pool the dashboard reads turns
  // it back into the epic's title.
  const everything = useIssues({ limit: 500 });
  const titles = useMemo(
    () => new Map((everything.data?.items ?? []).map((issue) => [issue.id, issue.title])),
    [everything.data],
  );
  const titleOf = (id: string) => titles.get(id);

  const events = useMemo(
    () => (activity.data?.events ?? []).filter((event) => FEED_FIELDS.has(event.field)).slice(0, FEED_LENGTH),
    [activity.data],
  );

  return (
    <PaneSection id="home.activity" title="Activity" count={<span className="mono">field_events</span>} fill>
      <div className="sb-body">
        {activity.isPending ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            Loading…
          </p>
        ) : activity.isError ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            {activity.error instanceof Error ? activity.error.message : "Could not load activity"}
          </p>
        ) : events.length === 0 ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            No field events yet.
          </p>
        ) : (
          events.map((event) => (
            <button
              key={`${event.issue_id}-${event.seq}-${event.field}`}
              type="button"
              className="row"
              style={{ height: "auto", padding: "5px 8px", alignItems: "flex-start" }}
              onClick={() => onOpen(event.short_ref)}
            >
              <span style={{ width: 16, display: "grid", placeItems: "center", marginTop: 2 }}>
                <GitCommitHorizontal className="i" aria-hidden />
              </span>
              <span className="lbl" style={{ whiteSpace: "normal", lineHeight: 1.35, fontSize: 12 }}>
                <b style={{ fontWeight: 500 }}>{event.author}</b> set {event.field} →{" "}
                <span className="mono">
                  {event.field === "status"
                    ? (labelOf.get(event.new_value ?? "") ?? event.new_value ?? "∅")
                    : event.new_value === null
                      ? "∅"
                      : resolveIdValue(event.field, event.new_value, titleOf)}
                </span>
                <br />
                <span style={{ color: "var(--muted)" }}>
                  <IssueHandle shortRef={event.short_ref} number={event.number} /> · {relativeTime(event.ts)}
                </span>
              </span>
            </button>
          ))
        )}
      </div>
    </PaneSection>
  );
}

export function HomePane({ onOpen }: { onOpen: (id: string) => void }) {
  const schema = useSchema();
  const pool = useOpenPool();
  const statuses = schema.data?.workflow.statuses ?? [];
  const waitingStatuses = useMemo(
    () => statuses.filter((s) => WAITING.test(`${s.id} ${s.label}`)),
    [statuses],
  );

  return (
    <>
      <WaitingOn pool={pool.data?.items ?? []} statuses={waitingStatuses} onOpen={onOpen} />
      <Activity statuses={statuses} onOpen={onOpen} />
    </>
  );
}
