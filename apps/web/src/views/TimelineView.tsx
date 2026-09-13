// The history layer, made visible: every field change and comment in the
// workspace, in the order it happened, with a scrubber that answers "what
// did the board look like then?".
//
// Two things about this screen are only possible because the data lives in
// git. The feed is derived from commits, so it says the same thing whether
// the change came from the UI, the CLI or someone's text editor. And the
// as-of view is computed from `field_events` on demand rather than restored
// from a snapshot, because there is no snapshot — nothing here is stored
// (invariant 5).
//
// A point in history is a `seq`, never a date: a date maps to one only
// through an author's clock. The density strip, the date input and the
// release picker all resolve to "the newest event on or before that day"
// among the events loaded here, and then the URL carries the seq.

import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useQueries } from "@tanstack/react-query";
import type { LucideIcon } from "lucide-react";
import {
  Calendar,
  GitCommitHorizontal,
  Hash,
  Layers,
  MessageSquare,
  Pencil,
  Plus,
  Tag,
  User,
  X,
  Zap,
} from "lucide-react";
import * as api from "../lib/api";
import {
  queryKeys,
  useActivity,
  useActivitySummary,
  useIssues,
  useReleases,
  useSchema,
  useWorkspaceComments,
} from "../lib/queries";
import {
  fillDays,
  filterBucket,
  seqForDay,
  workspaceTimeline,
  type TimelineEvent,
  type TimelineKind,
} from "../lib/activity";
import { useViewOptions } from "../lib/viewopts";
import { useRegisterPeekList } from "../lib/peeklist";
import { DAY_MS, toDay } from "../lib/schedule";
import type { CategoryCountsDto, IssueType, Priority, StatusCategory } from "../lib/types";
import { resolveIdValue } from "../lib/format";
import { Avatar, IssueHandle, PriorityDot, StatusPill, TypeBadge } from "../components/badges";
import { Btn, HeadingNote, SectionHeading } from "../components/chrome";
import { ErrorBox, Loading } from "../components/states";
import { TIMELINE_FEED_LIMIT } from "../components/panes/TimelinePane";
import { cn } from "../lib/cn";

const DENSITY_DAYS = 56;
const RANGE_DAYS = { "7d": 7, "30d": 30, "90d": 90, all: Number.POSITIVE_INFINITY } as const;
const MON = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const ISSUE_TYPES: readonly string[] = ["task", "bug", "story", "spike", "chore"];
const CATEGORIES: readonly StatusCategory[] = ["todo", "doing", "done"];

const KIND_ICON: Record<TimelineKind, LucideIcon> = {
  status: GitCommitHorizontal,
  priority: Zap,
  assignees: User,
  labels: Tag,
  epic: Layers,
  estimate: Hash,
  due: Calendar,
  start: Calendar,
  title: Pencil,
  other: Pencil,
  comment: MessageSquare,
  created: Plus,
};

/** "Sep 12" for a `YYYY-MM-DD` day or an RFC3339 stamp, in UTC like the files. */
function fmtShort(dayOrIso: string): string {
  const d = new Date(dayOrIso.length === 10 ? `${dayOrIso}T00:00:00Z` : dayOrIso);
  return `${MON[d.getUTCMonth()]} ${d.getUTCDate()}`;
}

function plural(n: number, word: string): string {
  return `${n} ${n === 1 ? word : `${word}s`}`;
}

/** The three-tone bar of a board: to do · in flight · done. */
function Stack({ counts }: { counts: CategoryCountsDto }) {
  const total = counts.todo + counts.doing + counts.done || 1;
  return (
    <span className="stack">
      {CATEGORIES.map((cat) => (
        <i key={cat} className={cat} style={{ width: `${(counts[cat] / total) * 100}%` }} title={`${cat}: ${counts[cat]}`} />
      ))}
    </span>
  );
}

function CompareRow({ label, counts }: { label: string; counts: CategoryCountsDto }) {
  return (
    <div className="cmp">
      <span className="cl">{label}</span>
      <Stack counts={counts} />
      <span className="cn mono">
        {counts.todo}·{counts.doing}·{counts.done}
      </span>
    </div>
  );
}

interface DayGroup {
  day: string;
  items: TimelineEvent[];
}

function groupByDay(rows: readonly TimelineEvent[]): DayGroup[] {
  const groups: DayGroup[] = [];
  for (const row of rows) {
    const day = row.ts.slice(0, 10);
    const open = groups[groups.length - 1];
    if (open && open.day === day) open.items.push(row);
    else groups.push({ day, items: [row] });
  }
  return groups;
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
  const { timeline } = useViewOptions();
  const now = Date.now();
  const today = toDay(now);

  // The newest page, plus any older pages the reader asked for. Each cursor
  // is its own query, so paging never overwrites the page behind it.
  const feed = useActivity({ limit: TIMELINE_FEED_LIMIT });
  const [cursors, setCursors] = useState<number[]>([]);
  const olderPages = useQueries({
    queries: cursors.map((beforeSeq) => ({
      queryKey: queryKeys.activity({ beforeSeq, limit: TIMELINE_FEED_LIMIT }),
      queryFn: () => api.getActivity({ beforeSeq, limit: TIMELINE_FEED_LIMIT }),
      staleTime: 15_000,
    })),
  });
  const events = useMemo(() => {
    const out = [...(feed.data?.events ?? [])];
    for (const page of olderPages) if (page.data) out.push(...page.data.events);
    return out;
  }, [feed.data, olderPages]);
  const lastPage = olderPages.length > 0 ? olderPages[olderPages.length - 1]?.data : feed.data;
  const nextBeforeSeq = lastPage?.next_before_seq ?? null;
  const loadingOlder = olderPages.some((page) => page.isPending);

  const comments = useWorkspaceComments(TIMELINE_FEED_LIMIT);
  const schema = useSchema();
  // `epic` events carry an issue id; resolve it to the epic's title so a
  // history line reads like a sentence rather than a key.
  const everything = useIssues({ limit: 500 });
  const titles = useMemo(
    () => new Map((everything.data?.items ?? []).map((issue) => [issue.id, issue.title])),
    [everything.data],
  );
  const titleOf = (id: string) => titles.get(id);
  const releases = useReleases();

  const rows = useMemo(() => workspaceTimeline(events, comments.data ?? []), [events, comments.data]);

  // --- where we stand -------------------------------------------------------
  const travelling = seq !== null;
  const cutoffEvent = travelling ? events.find((event) => event.seq === seq) : undefined;
  const cutoffDay = cutoffEvent?.ts.slice(0, 10) ?? null;
  const oldestLoadedDay = events.length > 0 ? (events[events.length - 1]?.ts.slice(0, 10) ?? null) : null;

  const rangeDays = RANGE_DAYS[timeline.range];
  const since = Number.isFinite(rangeDays) ? now - rangeDays * DAY_MS : Number.NEGATIVE_INFINITY;

  // The "what changed" card compares against a point in history. When
  // travelling that is the as-of seq; otherwise it is the start of the
  // range, so "Last 30d" counts the last thirty days rather than nothing.
  const rangeStartSeq = useMemo(() => {
    if (!Number.isFinite(since)) return 0;
    const found = seqForDay(toDay(since), events);
    if (found !== null) return found;
    // Nothing loaded before the window: exact only if history is fully loaded.
    return nextBeforeSeq === null && !feed.isPending ? 0 : null;
  }, [since, events, nextBeforeSeq, feed.isPending]);
  const summary = useActivitySummary({ seq: travelling ? seq : rangeStartSeq, days: DENSITY_DAYS });

  // --- the feed --------------------------------------------------------------
  const visible = useMemo(
    () =>
      rows.filter(
        (row) =>
          Date.parse(row.ts) >= since &&
          (!travelling || row.seq <= seq) &&
          (timeline.kinds.size === 0 || timeline.kinds.has(filterBucket(row.kind))) &&
          (timeline.who === null || row.author === timeline.who),
      ),
    [rows, since, travelling, seq, timeline.kinds, timeline.who],
  );
  const groups = useMemo(() => groupByDay(visible), [visible]);
  useRegisterPeekList(useMemo(() => [...new Set(visible.map((row) => row.issue.short_ref))], [visible]));

  const commentsInWindow = rows.filter(
    (row) => row.kind === "comment" && (travelling ? row.seq > seq : Date.parse(row.ts) >= since),
  ).length;

  // --- the density strip ------------------------------------------------------
  const days = useMemo(() => fillDays(summary.data?.days ?? [], today, DENSITY_DAYS), [summary.data, today]);
  const peak = Math.max(1, ...days.map((day) => day.count));
  const markDay = travelling ? (cutoffDay ?? days[0]?.day ?? today) : today;
  const markIndex = Math.max(0, days.findIndex((day) => day.day === markDay));
  const markLeft = (markIndex / (DENSITY_DAYS - 1)) * 100;

  // Resolve a day to a point in history and go there. A day at or after
  // today is "now"; a day older than the loaded window cannot be resolved
  // here, so it is left alone (the bar says why).
  const lastSought = useRef<number | null | undefined>(undefined);
  useEffect(() => {
    lastSought.current = seq;
  }, [seq]);
  const seekDay = (day: string) => {
    const target = day >= today ? null : seqForDay(day, events);
    if (day < today && target === null) return;
    if (lastSought.current !== undefined && lastSought.current === target) return;
    lastSought.current = target;
    onSeek(target);
  };
  const dragging = useRef(false);
  const pick = (e: ReactPointerEvent<HTMLDivElement>) => {
    const bar = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(".db");
    const day = bar?.dataset.day;
    if (day) seekDay(day);
  };

  const shipped = (releases.data ?? []).filter((r) => r.status === "released" && r.target !== null);
  const pickedRelease = shipped.find((r) => r.target === cutoffDay)?.version ?? "";

  const fmtValue = (field: string, value: string | null) => {
    if (value === null || value === "" || value === "—") return <span style={{ color: "var(--faint)" }}>—</span>;
    if (field === "status") {
      const status = schema.data?.workflow.statuses.find((s) => s.id === value);
      if (status) return <StatusPill status={status} />;
    }
    if (field === "priority" && /^p\d$/.test(value)) {
      return (
        <>
          <PriorityDot priority={value as Priority} /> {value.toUpperCase()}
        </>
      );
    }
    return <span className="mono">{resolveIdValue(field, value, titleOf)}</span>;
  };

  const rowText = (row: TimelineEvent) => {
    if (row.kind === "created") {
      return (
        <>
          created{" "}
          {row.type !== null && ISSUE_TYPES.includes(row.type) ? (
            <TypeBadge type={row.type as IssueType} />
          ) : (
            <span className="mono">{row.type ?? "issue"}</span>
          )}
        </>
      );
    }
    if (row.kind === "comment") {
      return (
        <>
          commented <span className="tlq">{row.text.slice(0, 90)}</span>
        </>
      );
    }
    return (
      <>
        set <b>{row.field}</b> {fmtValue(row.field, row.old)} <span style={{ color: "var(--faint)" }}>→</span>{" "}
        {fmtValue(row.field, row.new)}
      </>
    );
  };

  const asOfNote = travelling
    ? `${cutoffDay ?? "before the loaded window"} · seq ≤ ${seq}`
    : "HEAD · now";
  const sinceLabel = travelling
    ? `Since ${cutoffDay ? fmtShort(cutoffDay) : `seq ${seq}`}`
    : `Last ${timeline.range === "all" ? "everything" : timeline.range}`;

  return (
    <div className="tlv">
      <section className="tlhead">
        <div
          className="dens"
          title="Events per day, last 8 weeks — click or drag to set the as-of point"
          onPointerDown={(e) => {
            if (e.button !== 0) return;
            dragging.current = true;
            pick(e);
          }}
          onPointerMove={(e) => {
            if (dragging.current) pick(e);
          }}
          onPointerUp={() => {
            dragging.current = false;
          }}
          onPointerLeave={() => {
            dragging.current = false;
          }}
        >
          {days.map((day) => {
            const reachable = day.day >= today || (oldestLoadedDay !== null && day.day >= oldestLoadedDay);
            return (
              <i
                key={day.day}
                className={cn("db", day.day <= markDay && "on")}
                data-day={day.day}
                aria-disabled={!reachable}
                style={{
                  height: `${Math.max(2, (day.count / peak) * 100)}%`,
                  ...(reachable ? {} : { opacity: 0.35, cursor: "not-allowed" }),
                }}
                title={
                  reachable
                    ? `${fmtShort(day.day)} · ${plural(day.count, "event")}`
                    : `${fmtShort(day.day)} · ${plural(day.count, "event")} · older than the loaded feed — load older first`
                }
              />
            );
          })}
          <i className="mark" style={{ left: `${markLeft}%` }}>
            <b>{travelling ? fmtShort(markDay) : "now"}</b>
          </i>
        </div>

        <div className="asof">
          <div className="asof-l">
            <SectionHeading>
              Board as of <HeadingNote>{asOfNote}</HeadingNote>
            </SectionHeading>
            {summary.isError ? (
              <ErrorBox error={summary.error} title="Could not read the history" onRetry={() => void summary.refetch()} />
            ) : null}
            {summary.data ? (
              <>
                {travelling ? (
                  <CompareRow label={cutoffDay ? fmtShort(cutoffDay) : `≤${seq}`} counts={summary.data.at_cutoff} />
                ) : null}
                <CompareRow label="now" counts={summary.data.now} />
              </>
            ) : summary.isPending ? (
              <Loading label="Reading history…" />
            ) : null}
            <div className="ctl">
              <label className="mono" htmlFor="tl-asof" style={{ fontSize: 11, color: "var(--muted)" }}>
                as of
              </label>
              <input
                id="tl-asof"
                type="date"
                value={cutoffDay ?? ""}
                max={today}
                min={oldestLoadedDay ?? undefined}
                onChange={(e) => (e.target.value ? seekDay(e.target.value) : onSeek(null))}
              />
              <select
                aria-label="as of a release"
                value={pickedRelease}
                onChange={(e) => {
                  const release = shipped.find((r) => r.version === e.target.value);
                  if (release?.target) seekDay(release.target);
                }}
              >
                <option value="">— or a release —</option>
                {shipped.map((release) => {
                  const day = release.target ?? "";
                  const reachable = day >= today || (oldestLoadedDay !== null && day >= oldestLoadedDay);
                  return (
                    <option key={release.version} value={release.version} disabled={!reachable} title={reachable ? undefined : "older than the loaded feed — load older first"}>
                      {release.version} · {day}
                    </option>
                  );
                })}
              </select>
              {travelling ? (
                <Btn onClick={() => onSeek(null)}>
                  <X className="i" aria-hidden />
                  Back to now
                </Btn>
              ) : null}
              <HeadingNote className="text-[11px]">
                resolved to a <span className="mono">seq</span>, computed from field_events — nothing is rebuilt
              </HeadingNote>
            </div>
          </div>

          <div className="asof-r">
            <SectionHeading>
              {sinceLabel} <HeadingNote>semantic diff · what changed, not which bytes</HeadingNote>
            </SectionHeading>
            <div className="sdiff">
              {(
                [
                  [summary.data?.since.finished, "→ done", "done"],
                  [summary.data?.since.created, "new", "doing"],
                  [summary.data?.since.touched, "touched", ""],
                  [summary.data?.since.reprioritized, "reprioritized", ""],
                  [commentsInWindow, "comments", ""],
                ] as ReadonlyArray<[number | undefined, string, string]>
              ).map(([n, label, tone]) => (
                <span key={label} className={cn("sd", tone)}>
                  <b>{n ?? "…"}</b>
                  {label}
                </span>
              ))}
            </div>
            {travelling ? (
              <p className="dql" style={{ margin: "8px 0 0" }}>
                Compare: {cutoffDay ? fmtShort(cutoffDay) : `seq ${seq}`} → now.
              </p>
            ) : (
              <p className="dql" style={{ margin: "8px 0 0" }}>
                Pick a date or a release to time-travel. Dates map to seq via author time (clock skew applies);
                releases are exact.
              </p>
            )}
          </div>
        </div>
      </section>

      <section className="feed">
        {feed.isPending ? <Loading label="Loading activity…" /> : null}
        {feed.isError ? (
          <ErrorBox error={feed.error} title="Could not load the activity feed" onRetry={() => void feed.refetch()} />
        ) : null}
        {comments.isError ? (
          <ErrorBox error={comments.error} title="Could not load comments" tone="warn" onRetry={() => void comments.refetch()} />
        ) : null}

        {groups.map((group) => (
          <div key={group.day} className="day">
            <div className="dayh">
              <span>{group.day === today ? "Today" : fmtShort(group.day)}</span>
              <span className="dql">{plural(group.items.length, "event")}</span>
            </div>
            {group.items.map((row) => {
              const Icon = KIND_ICON[row.kind];
              return (
                <div key={row.key} className="tle">
                  <span className={cn("tk", row.kind)}>
                    <Icon className="i" aria-hidden />
                  </span>
                  <span className="tt mono" title={row.ts}>
                    {row.ts.slice(11, 16)}
                  </span>
                  <span className="tw">
                    <Avatar name={row.author} />
                    <b>{row.author}</b>
                  </span>
                  <span className="tx">{rowText(row)}</span>
                  <button type="button" className="ti" onClick={() => onOpen(row.issue.short_ref)}>
                    <IssueHandle shortRef={row.issue.short_ref} number={row.issue.number} />
                    <span className="lbl">{row.issue.title || "(deleted)"}</span>
                  </button>
                </div>
              );
            })}
          </div>
        ))}

        {feed.data && groups.length === 0 ? (
          <p className="empty" style={{ padding: 20 }}>
            No events in this range with these filters.
          </p>
        ) : null}

        {timeline.range === "all" && nextBeforeSeq !== null ? (
          <div style={{ display: "flex", justifyContent: "center", paddingTop: 8 }}>
            <Btn
              disabled={loadingOlder}
              onClick={() => setCursors((current) => [...current, nextBeforeSeq])}
              title={`${events.length} events loaded · older ones are still in the repo's history`}
            >
              {loadingOlder ? "Loading…" : "Load older"}
            </Btn>
          </div>
        ) : null}
      </section>
    </div>
  );
}
