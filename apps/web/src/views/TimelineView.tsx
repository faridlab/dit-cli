// The history layer, made visible: every field change in the workspace, in
// the order it happened, with a scrubber that answers "what did this look
// like then?".
//
// Two things about this screen are only possible because the data lives in
// git. The feed is derived from commits, so it says the same thing whether
// the change came from the UI, the CLI or someone's text editor. And the
// as-of view is computed from `field_events` on demand rather than restored
// from a snapshot, because there is no snapshot — nothing here is stored
// (invariant 5).

import { useMemo } from "react";
import { GitCommitVertical, RotateCcw } from "lucide-react";
import { useActivity, useActivitySummary } from "../lib/queries";
import { fullTimestamp } from "../lib/format";
import { useRegisterPeekList } from "../lib/peeklist";
import { DAY_MS, toDay } from "../lib/schedule";
import type { ActivityEventDto } from "../lib/types";
import { ErrorBox, Loading } from "../components/states";
import { SectionHeading } from "../components/chrome";
import { cn } from "../lib/cn";

const FEED_LIMIT = 150;
const HISTOGRAM_DAYS = 56;

/** Events for one day, in the order the server returned them. */
interface DayGroup {
  day: string;
  events: ActivityEventDto[];
}

function groupByDay(events: readonly ActivityEventDto[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const event of events) {
    const day = event.ts.slice(0, 10);
    const open = groups[groups.length - 1];
    if (open && open.day === day) open.events.push(event);
    else groups.push({ day, events: [event] });
  }
  return groups;
}

function dayLabel(day: string, now: number): string {
  const today = toDay(now);
  if (day === today) return "Today";
  if (day === toDay(now - DAY_MS)) return "Yesterday";
  return new Date(`${day}T00:00:00Z`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: day.slice(0, 4) === today.slice(0, 4) ? undefined : "numeric",
    timeZone: "UTC",
  });
}

/** A field value as a person reads it. Statuses and priorities keep their
 *  own shape; everything else is shown verbatim, because inventing a
 *  presentation for an unknown field is how a UI starts lying. */
function Value({ value }: { value: string | null }) {
  if (value === null || value.length === 0) {
    return <span className="text-faint">—</span>;
  }
  return <span className="font-mono text-[11.5px] text-ink-2">{value}</span>;
}

function Counts({ counts, label }: { counts: { todo: number; doing: number; done: number }; label: string }) {
  const total = counts.todo + counts.doing + counts.done || 1;
  const bar = (n: number, tone: string) =>
    n > 0 ? <i className={cn("block h-full", tone)} style={{ width: `${(n / total) * 100}%` }} /> : null;
  return (
    <div className="grid grid-cols-[54px_minmax(0,1fr)_86px] items-center gap-3">
      <span className="font-mono text-[11px] text-muted">{label}</span>
      <span className="flex h-2.5 gap-[2px] overflow-hidden rounded-[3px] bg-sunken">
        {bar(counts.todo, "bg-todo-text/50")}
        {bar(counts.doing, "bg-doing-text/70")}
        {bar(counts.done, "bg-done-text/70")}
      </span>
      <span className="text-right font-mono text-[11px] tabular-nums text-muted">
        {counts.todo}·{counts.doing}·{counts.done}
      </span>
    </div>
  );
}

export function TimelineView({
  seq,
  onOpen,
  onSeek,
}: {
  /** Where the scrubber stands, as a position in the commit graph. */
  seq: number | null;
  onOpen: (id: string) => void;
  onSeek: (seq: number | null) => void;
}) {
  const feed = useActivity({ limit: FEED_LIMIT });
  const summary = useActivitySummary({ seq, days: HISTOGRAM_DAYS });
  const now = Date.now();

  const events = useMemo(() => feed.data?.events ?? [], [feed.data]);
  // J/K walk the issues the feed mentions, in the order they appear.
  useRegisterPeekList(
    useMemo(() => [...new Set(events.map((event) => event.short_ref))], [events]),
  );
  const groups = useMemo(() => groupByDay(events), [events]);

  const days = summary.data?.days ?? [];
  const peak = Math.max(1, ...days.map((day) => day.count));
  const travelling = summary.data ? summary.data.seq < summary.data.max_seq : false;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <h1 className="shrink-0 text-lg font-semibold text-ink">Timeline</h1>
        <span className="font-mono text-[11px] text-dim">
          read from git · nothing on this screen is stored
        </span>
        {travelling ? (
          <button
            type="button"
            onClick={() => onSeek(null)}
            className="ml-auto flex items-center gap-1.5 rounded-md border border-accent px-2.5 py-1 text-[12px] text-context transition-colors hover:bg-accent-soft"
          >
            <RotateCcw className="size-3.5" aria-hidden />
            Back to now
          </button>
        ) : null}
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1000px] flex-col gap-6 px-6 pb-16 pt-5">
          {summary.isError ? (
            <ErrorBox
              error={summary.error}
              title="Could not read the history"
              onRetry={() => void summary.refetch()}
            />
          ) : null}

          {/* The scrubber: one bar per day, click to stand there. */}
          <section className="flex flex-col gap-2">
            <SectionHeading size="sm">
              Activity · last {HISTOGRAM_DAYS} days
            </SectionHeading>
            <div className="flex h-14 items-end gap-[3px]">
              {days.length === 0 && !summary.isPending ? (
                <p className="text-xs text-faint">No changes recorded yet.</p>
              ) : null}
              {days.map((day) => {
                // Stand at the last event on or before that day: a date maps
                // to a position in history only through the events on it, and
                // the feed is a window — days older than the oldest loaded
                // change cannot be resolved here, so they say so.
                const last = events.find((event) => event.ts.slice(0, 10) <= day.day);
                const reachable = last !== undefined;
                const standingHere = last !== undefined && seq === last.seq;
                return (
                  <button
                    key={day.day}
                    type="button"
                    disabled={!reachable}
                    aria-pressed={standingHere}
                    title={
                      reachable
                        ? `${day.day} · ${day.count} ${day.count === 1 ? "change" : "changes"} · click to stand here`
                        : `${day.day} · ${day.count} ${day.count === 1 ? "change" : "changes"} · older than the ${events.length} changes loaded below`
                    }
                    onClick={() => last && onSeek(last.seq)}
                    className="group flex h-full flex-1 items-end disabled:cursor-default"
                  >
                    <span
                      className={cn(
                        "w-full rounded-[2px] transition-colors",
                        standingHere
                          ? "bg-accent"
                          : reachable
                            ? "bg-edge group-hover:bg-accent"
                            : "bg-edge/50",
                      )}
                      style={{ height: `${Math.max(6, (day.count / peak) * 100)}%` }}
                    />
                  </button>
                );
              })}
            </div>
          </section>

          {/* Then, now, and the difference. */}
          <section className="grid gap-3 min-[900px]:grid-cols-2">
            <div className="flex flex-col gap-2.5 rounded-lg border border-edge p-4">
              <SectionHeading size="sm">
                Board {travelling ? "then and now" : "now"}
              </SectionHeading>
              {summary.data ? (
                <>
                  {travelling ? (
                    <Counts counts={summary.data.at_cutoff} label={`seq ${summary.data.seq}`} />
                  ) : null}
                  <Counts counts={summary.data.now} label="now" />
                  <p className="font-mono text-[11px] leading-relaxed text-faint">
                    to do · in flight · done, recomputed from field_events
                  </p>
                </>
              ) : (
                <Loading label="Reading history…" />
              )}
            </div>

            <div className="flex flex-col gap-2.5 rounded-lg border border-edge p-4">
              <SectionHeading size="sm">
                {travelling ? "Since that point" : "Since the beginning"}
              </SectionHeading>
              {summary.data ? (
                <div className="flex flex-wrap gap-2">
                  {[
                    ["finished", summary.data.since.finished, "bg-done-bg text-done-text"],
                    ["created", summary.data.since.created, "bg-doing-bg text-doing-text"],
                    ["reprioritized", summary.data.since.reprioritized, "bg-warn-bg text-warn-text"],
                    ["issues touched", summary.data.since.touched, "bg-sunken text-ink-2"],
                  ].map(([label, value, tone]) => (
                    <span
                      key={String(label)}
                      className={cn(
                        "flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[12px]",
                        String(tone),
                      )}
                    >
                      <b className="font-mono font-semibold">{String(value)}</b>
                      {String(label)}
                    </span>
                  ))}
                </div>
              ) : null}
              <p className="font-mono text-[11px] leading-relaxed text-faint">
                a semantic diff: what changed, not which bytes
              </p>
            </div>
          </section>

          {/* The feed itself. */}
          <section className="flex flex-col gap-4">
            {feed.isPending ? <Loading label="Loading activity…" /> : null}
            {feed.isError ? (
              <ErrorBox
                error={feed.error}
                title="Could not load the activity feed"
                onRetry={() => void feed.refetch()}
              />
            ) : null}
            {feed.data && events.length === 0 ? (
              <p className="py-8 text-center text-sm text-muted">
                No field changes recorded yet. They appear here as soon as anything is committed.
              </p>
            ) : null}

            {groups.map((group) => (
              <div key={group.day} className="flex flex-col gap-0.5">
                <div className="sticky top-0 z-10 flex items-baseline gap-2 bg-app py-1.5">
                  <h2 className="text-[12px] font-semibold text-ink">
                    {dayLabel(group.day, now)}
                  </h2>
                  <span className="font-mono text-[11px] text-faint">
                    {group.events.length} {group.events.length === 1 ? "change" : "changes"}
                  </span>
                </div>
                {group.events.map((event) => (
                  <div
                    key={`${event.seq}`}
                    className={cn(
                      "grid grid-cols-[20px_46px_minmax(0,1fr)] items-baseline gap-3 rounded-md px-2 py-1.5 text-[12.5px] min-[820px]:grid-cols-[20px_46px_120px_minmax(0,1fr)_220px]",
                      seq !== null && event.seq === seq && "bg-accent-soft",
                    )}
                  >
                    <GitCommitVertical className="size-4 self-center text-faint" aria-hidden />
                    <span
                      className="font-mono text-[11px] text-faint"
                      title={`${fullTimestamp(event.ts)}\ncommit ${event.commit_sha}\nseq ${event.seq}`}
                    >
                      {event.ts.slice(11, 16)}
                    </span>
                    <span className="hidden truncate font-mono text-ink-2 min-[820px]:block">
                      {event.author}
                    </span>
                    <span className="flex flex-wrap items-baseline gap-1.5 text-muted">
                      <span className="text-ink-2">{event.field}</span>
                      <Value value={event.old_value} />
                      <span className="text-faint" aria-label="changed to">
                        →
                      </span>
                      <Value value={event.new_value} />
                    </span>
                    <button
                      type="button"
                      onClick={() => onOpen(event.short_ref)}
                      className="hidden min-w-0 items-baseline gap-2 text-left min-[820px]:flex"
                    >
                      <span className="font-mono text-[11px] tabular-nums text-muted">
                        {event.number !== null ? `#${event.number}` : event.short_ref}
                      </span>
                      <span className="truncate text-ink-2 hover:text-ink hover:underline">
                        {event.title || "(deleted)"}
                      </span>
                    </button>
                  </div>
                ))}
              </div>
            ))}

            {feed.data?.next_before_seq !== null && feed.data !== undefined ? (
              <p className="pt-2 text-center font-mono text-[11px] text-faint">
                showing the last {events.length} changes · older ones are in the repo's history
              </p>
            ) : null}
          </section>
        </div>
      </div>
    </div>
  );
}

export { dayLabel, groupByDay };
