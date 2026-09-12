// Issues on a time axis, dragged to reschedule.
//
// The bar is a reading of two ordinary fields, `start` and `due`. Most
// issues carry neither, so a missing edge is inferred from the estimate and
// drawn dashed: visible, obviously a guess, and never written back. Dragging
// is what commits — one field edit per edge, through the same patch endpoint
// every other edit uses, so a reschedule is as auditable as a status change.
//
// Dependencies come from `blocked_by`, which already exists in the file. The
// critical path is computed on read; nothing about it is stored.

import { useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { CalendarPlus } from "lucide-react";
import { useBulkPatchIssue, useIssues, usePatchIssue, useSchema } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import {
  criticalPath,
  DAY_MS,
  dayStart,
  durationDays,
  epicSpan,
  isLate,
  spanOf,
  toDay,
  type Span,
} from "../lib/schedule";
import type { IssueDto, StatusDto } from "../lib/types";
import { AssigneeCircles, IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { Empty, ErrorBox, Loading } from "../components/states";
import { SectionHeading } from "../components/chrome";
import { cn } from "../lib/cn";
import { useGanttOptions, type GanttZoom } from "../components/panes/GanttPane";

const PAGE_SIZE = 500;
const ROW_HEIGHT = 34;
const GROUP_HEIGHT = 30;

/** Pixels per day at each zoom. Wider than a few pixels or the bars stop
 *  being draggable; narrower than ~40 and a quarter stops fitting. */
const PIXELS_PER_DAY: Record<GanttZoom, number> = { day: 36, week: 13, month: 5 };
const WINDOW_DAYS: Record<GanttZoom, { before: number; total: number }> = {
  day: { before: 7, total: 35 },
  week: { before: 21, total: 98 },
  month: { before: 60, total: 260 },
};

interface Row {
  kind: "group" | "issue";
  key: string;
  top: number;
  label?: string;
  count?: number;
  span?: Span | null;
  derived?: boolean;
  issue?: IssueDto;
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

function monthTicks(from: number, days: number, ppd: number) {
  const ticks: Array<{ x: number; width: number; label: string }> = [];
  const end = from + days * DAY_MS;
  let cursor = from;
  while (cursor < end) {
    const date = new Date(cursor);
    const monthStart = Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1);
    const nextMonth = Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1);
    const a = Math.max(monthStart, from);
    const b = Math.min(nextMonth, end);
    ticks.push({
      x: ((a - from) / DAY_MS) * ppd,
      width: ((b - a) / DAY_MS) * ppd,
      label: `${MONTHS[date.getUTCMonth()]} ${date.getUTCFullYear()}`,
    });
    cursor = nextMonth;
  }
  return ticks;
}

export function GanttView({ onOpen }: { onOpen: (id: string) => void }) {
  const issues = useIssues({ limit: PAGE_SIZE });
  const schema = useSchema();
  // The tray schedules one issue at a time through the same endpoint every
  // other edit uses — one PATCH, one commit.
  const schedule = useBulkPatchIssue();
  const options = useGanttOptions();
  const scroller = useRef<HTMLDivElement>(null);

  const statuses: StatusDto[] = schema.data?.workflow.statuses ?? [];
  const doneIds = useMemo(
    () => new Set(statuses.filter((s) => s.category === "done").map((s) => s.id)),
    [statuses],
  );

  const all = issues.data?.items ?? [];
  const epics = useMemo(() => all.filter((issue) => issue.type === "story"), [all]);
  const work = useMemo(
    () =>
      all.filter(
        (issue) =>
          issue.type !== "story" && (options.showDone || !doneIds.has(issue.status)),
      ),
    [all, doneIds, options.showDone],
  );

  const scheduled = useMemo(() => work.filter((issue) => spanOf(issue) !== null), [work]);
  const unscheduled = useMemo(() => work.filter((issue) => spanOf(issue) === null), [work]);

  const critical = useMemo(
    () =>
      options.critical
        ? criticalPath(
            scheduled.map((issue) => ({
              short_ref: issue.short_ref,
              start: issue.start,
              due: issue.due,
              estimate: issue.estimate,
              // `blocked_by` is not on the wire yet; the field exists in the
              // file and the model, so this reads empty until the DTO
              // carries it. The path then appears with no UI change.
              blocked_by: [],
            })),
          )
        : new Set<string>(),
    [options.critical, scheduled],
  );

  // Rows: optionally grouped, each group's issues sorted by when they start.
  const rows = useMemo(() => {
    const byStart = (a: IssueDto, b: IssueDto) =>
      (spanOf(a)?.start ?? 0) - (spanOf(b)?.start ?? 0);

    const groups: Array<{ key: string; label: string; issues: IssueDto[]; epic?: IssueDto }> = [];
    if (options.groupBy === "epic") {
      for (const epic of epics) {
        const children = scheduled.filter((issue) => issue.epic === epic.id);
        if (children.length > 0) {
          groups.push({ key: epic.id, label: epic.title, issues: children, epic });
        }
      }
      const loose = scheduled.filter(
        (issue) => !issue.epic || !epics.some((epic) => epic.id === issue.epic),
      );
      if (loose.length > 0) groups.push({ key: "none", label: "No epic", issues: loose });
    } else if (options.groupBy === "assignee") {
      const people = [...new Set(scheduled.flatMap((issue) => issue.assignees))].sort();
      for (const person of people) {
        groups.push({
          key: person,
          label: person,
          issues: scheduled.filter((issue) => issue.assignees.includes(person)),
        });
      }
      const nobody = scheduled.filter((issue) => issue.assignees.length === 0);
      if (nobody.length > 0) groups.push({ key: "none", label: "Unassigned", issues: nobody });
    } else {
      groups.push({ key: "all", label: "Scheduled", issues: [...scheduled] });
    }

    const out: Row[] = [];
    let top = 0;
    for (const group of groups) {
      if (options.groupBy !== "none") {
        const derived = group.epic
          ? epicSpan(group.epic, group.issues)
          : { span: null as Span | null, derived: true };
        out.push({
          kind: "group",
          key: `g:${group.key}`,
          top,
          label: group.label,
          count: group.issues.length,
          span: derived?.span ?? null,
          derived: derived?.derived ?? true,
        });
        top += GROUP_HEIGHT;
      }
      for (const issue of [...group.issues].sort(byStart)) {
        out.push({ kind: "issue", key: issue.short_ref, top, issue });
        top += ROW_HEIGHT;
      }
    }
    return { rows: out, height: top };
  }, [epics, options.groupBy, scheduled]);

  useRegisterPeekList(
    useMemo(
      () => rows.rows.filter((row) => row.issue).map((row) => row.issue!.short_ref),
      [rows],
    ),
  );

  // The window: a few weeks either side of today, wide enough that dragging
  // has somewhere to go.
  const today = dayStart(new Date().toISOString());
  const ppd = PIXELS_PER_DAY[options.zoom];
  const window = WINDOW_DAYS[options.zoom];
  const from = today - window.before * DAY_MS;
  const width = window.total * ppd;
  const xOf = (ms: number) => ((ms - from) / DAY_MS) * ppd;

  if (issues.isPending) return <Loading label="Loading plan…" className="flex-1" />;
  if (issues.isError) {
    return (
      <ErrorBox
        error={issues.error}
        title="Could not load the plan"
        onRetry={() => void issues.refetch()}
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <h1 className="shrink-0 text-lg font-semibold text-ink">Gantt</h1>
        <span className="font-mono text-[11px] text-dim">
          {scheduled.length} scheduled · {unscheduled.length} without dates
        </span>
        <span className="ml-auto hidden font-mono text-[11px] text-faint min-[1000px]:block">
          drag a bar to move it · drag an edge to change start or due
        </span>
      </header>

      {scheduled.length === 0 && unscheduled.length === 0 ? (
        <Empty
          title="Nothing to plan yet"
          hint="Create an issue, then give it a start or a due date."
          className="flex-1 justify-center"
        />
      ) : (
        <div className="flex min-h-0 flex-1">
          {/* The row labels, fixed while the axis scrolls. */}
          <div className="flex w-[280px] shrink-0 flex-col border-r border-edge bg-panel min-[1200px]:w-[340px]">
            <div className="h-[46px] shrink-0 border-b border-edge" />
            <div className="relative min-h-0 flex-1 overflow-hidden" style={{ height: rows.height }}>
              {rows.rows.map((row) =>
                row.kind === "group" ? (
                  <div
                    key={row.key}
                    style={{ top: row.top, height: GROUP_HEIGHT }}
                    className="absolute inset-x-0 flex items-center gap-2 border-b border-edge bg-sunken px-3 text-[12px] font-medium text-ink"
                  >
                    <span className="truncate">{row.label}</span>
                    <span className="ml-auto font-mono text-[11px] text-muted">{row.count}</span>
                  </div>
                ) : (
                  <button
                    key={row.key}
                    type="button"
                    onClick={() => onOpen(row.issue!.short_ref)}
                    style={{ top: row.top, height: ROW_HEIGHT }}
                    className="absolute inset-x-0 flex items-center gap-2 border-b border-edge px-3 text-left text-[12.5px] text-ink-2 transition-colors hover:bg-hover hover:text-ink"
                  >
                    <IssueHandle shortRef={row.issue!.short_ref} number={row.issue!.number} />
                    <TypeBadge type={row.issue!.type} />
                    <PriorityDot priority={row.issue!.priority} />
                    <span className="min-w-0 flex-1 truncate">{row.issue!.title}</span>
                    <AssigneeCircles assignees={row.issue!.assignees} />
                  </button>
                ),
              )}
            </div>
          </div>

          {/* The axis and the bars. */}
          <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
            <div style={{ width }} className="relative">
              <div className="sticky top-0 z-10 h-[46px] border-b border-edge bg-app">
                <div className="relative h-[24px]">
                  {monthTicks(from, window.total, ppd).map((tick) => (
                    <span
                      key={tick.label + tick.x}
                      style={{ left: tick.x, width: tick.width }}
                      className="absolute top-0 truncate border-l border-edge pl-2 text-[12px] font-medium leading-6 text-ink"
                    >
                      {tick.label}
                    </span>
                  ))}
                </div>
                <div className="relative h-[22px]">
                  {options.zoom !== "month"
                    ? Array.from({ length: window.total }, (_, index) => {
                        const date = new Date(from + index * DAY_MS);
                        const weekend = date.getUTCDay() % 6 === 0;
                        if (options.zoom === "week" && date.getUTCDay() !== 1) return null;
                        return (
                          <span
                            key={index}
                            style={{ left: index * ppd, width: ppd * (options.zoom === "week" ? 7 : 1) }}
                            className={cn(
                              "absolute top-0 border-l border-edge text-center font-mono text-[10.5px] leading-5",
                              weekend ? "text-faint" : "text-muted",
                            )}
                          >
                            {date.getUTCDate()}
                          </span>
                        );
                      })
                    : null}
                </div>
              </div>

              <div className="relative" style={{ height: rows.height }}>
                {/* Weekends, so a bar's length reads as working time. */}
                {options.weekends && options.zoom !== "month"
                  ? Array.from({ length: window.total }, (_, index) => {
                      const date = new Date(from + index * DAY_MS);
                      if (date.getUTCDay() % 6 !== 0) return null;
                      return (
                        <i
                          key={index}
                          style={{ left: index * ppd, width: ppd }}
                          className="absolute inset-y-0 bg-sunken"
                        />
                      );
                    })
                  : null}
                <i
                  style={{ left: xOf(today) }}
                  className="absolute inset-y-0 z-[1] border-l-2 border-dashed border-accent"
                  aria-hidden
                />

                {rows.rows.map((row) =>
                  row.kind === "group" ? (
                    <div
                      key={row.key}
                      style={{ top: row.top, height: GROUP_HEIGHT }}
                      className="absolute inset-x-0 border-b border-edge bg-sunken/60"
                    >
                      {row.span ? (
                        <span
                          title={
                            row.derived
                              ? "Span derived from the issues inside — never stored"
                              : "The epic's own start and target"
                          }
                          style={{
                            left: xOf(row.span.start),
                            width: Math.max(4, xOf(row.span.end) - xOf(row.span.start)),
                          }}
                          className={cn(
                            "absolute top-[11px] block h-2 rounded-full",
                            row.derived ? "border border-dashed border-ctl" : "bg-ctl",
                          )}
                        />
                      ) : null}
                    </div>
                  ) : (
                    <GanttBar
                      key={row.key}
                      issue={row.issue!}
                      top={row.top}
                      ppd={ppd}
                      from={from}
                      today={today}
                      done={doneIds.has(row.issue!.status)}
                      critical={critical.has(row.issue!.short_ref)}
                      onOpen={onOpen}
                    />
                  ),
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Everything with no dates at all. An empty tray is the goal state. */}
      {unscheduled.length > 0 ? (
        <div className="max-h-[34%] shrink-0 overflow-auto border-t border-edge bg-panel px-5 py-3">
          <div className="mb-2 flex items-center gap-2">
            <SectionHeading size="sm">Not scheduled</SectionHeading>
            <span className="font-mono text-[11px] text-faint">
              no start, no due — schedule one to put it on the axis
            </span>
          </div>
          <ul className="flex flex-col">
            {unscheduled.map((issue) => (
              <li
                key={issue.short_ref}
                className="flex items-center gap-2.5 py-1 text-[12.5px] text-ink-2"
              >
                <IssueHandle shortRef={issue.short_ref} number={issue.number} />
                <TypeBadge type={issue.type} />
                <PriorityDot priority={issue.priority} />
                <button
                  type="button"
                  onClick={() => onOpen(issue.short_ref)}
                  className="min-w-0 flex-1 truncate text-left hover:text-ink hover:underline"
                >
                  {issue.title}
                </button>
                <span className="shrink-0 font-mono text-[11px] text-faint">
                  {issue.estimate ? `${issue.estimate}pt → ${durationDays(issue)}d` : "no estimate"}
                </span>
                <button
                  type="button"
                  disabled={schedule.isPending}
                  onClick={() =>
                    schedule.mutate([
                      {
                        id: issue.short_ref,
                        set: {
                          start: toDay(today),
                          due: toDay(today + (durationDays(issue) - 1) * DAY_MS),
                        },
                      },
                    ])
                  }
                  className="flex shrink-0 items-center gap-1.5 rounded-md border border-edge px-2 py-0.5 text-[11.5px] transition-colors hover:border-ctl hover:text-ink disabled:opacity-50"
                >
                  <CalendarPlus className="size-3.5" aria-hidden />
                  Schedule
                </button>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}

/** One draggable bar. Dragging the middle moves both dates; dragging an edge
 *  moves that one. Nothing commits until the pointer is released, and a drag
 *  that ends where it started is a click that opens the issue. */
function GanttBar({
  issue,
  top,
  ppd,
  from,
  today,
  done,
  critical,
  onOpen,
}: {
  issue: IssueDto;
  top: number;
  ppd: number;
  from: number;
  today: number;
  done: boolean;
  critical: boolean;
  onOpen: (id: string) => void;
}) {
  const patch = usePatchIssue(issue.short_ref);
  const [drag, setDrag] = useState<{ mode: "move" | "start" | "end"; days: number } | null>(null);
  const origin = useRef(0);

  const span = spanOf(issue);
  if (!span) return null;

  const shift = (drag?.days ?? 0) * DAY_MS;
  const start = drag?.mode === "end" ? span.start : span.start + shift;
  const end = drag?.mode === "start" ? span.end : span.end + (drag?.mode === "move" ? shift : 0);
  const live = {
    start: drag?.mode === "start" ? Math.min(span.start + shift, span.end - DAY_MS) : start,
    end: drag?.mode === "end" ? Math.max(span.end + shift, span.start + DAY_MS) : end,
  };
  const xOf = (ms: number) => ((ms - from) / DAY_MS) * ppd;
  const late = isLate(span, done, today);

  const begin = (mode: "move" | "start" | "end") => (event: ReactPointerEvent<HTMLElement>) => {
    event.preventDefault();
    event.stopPropagation();
    origin.current = event.clientX;
    setDrag({ mode, days: 0 });
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const move = (event: ReactPointerEvent<HTMLElement>) => {
    if (!drag) return;
    setDrag({ ...drag, days: Math.round((event.clientX - origin.current) / ppd) });
  };
  const finish = (event: ReactPointerEvent<HTMLElement>) => {
    if (!drag) return;
    const moved = drag.days;
    const mode = drag.mode;
    setDrag(null);
    if (moved === 0) {
      if (event.type === "pointerup") onOpen(issue.short_ref);
      return;
    }
    // The bar covers its due date, so the stored `due` is the day before the
    // exclusive end — the same arithmetic `spanOf` reads back.
    const set: { start?: string; due?: string } = {};
    if (mode === "move" || mode === "start") set.start = toDay(live.start);
    if (mode === "move" || mode === "end") set.due = toDay(live.end - DAY_MS);
    patch.mutate(set);
  };

  return (
    <div
      style={{ top, height: ROW_HEIGHT }}
      className="absolute inset-x-0 border-b border-edge"
    >
      <div
        role="button"
        tabIndex={0}
        aria-label={`${issue.title}, ${toDay(live.start)} to ${toDay(live.end - DAY_MS)}`}
        title={`${toDay(live.start)} → ${toDay(live.end - DAY_MS)}${span.inferredStart || span.inferredEnd ? " · dashed edge inferred from the estimate" : ""}`}
        onPointerDown={begin("move")}
        onPointerMove={move}
        onPointerUp={finish}
        onPointerCancel={finish}
        onKeyDown={(event) => {
          if (event.key === "Enter") onOpen(issue.short_ref);
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          const step = (event.key === "ArrowRight" ? 1 : -1) * DAY_MS;
          patch.mutate(
            event.shiftKey
              ? { due: toDay(span.end - DAY_MS + step) }
              : { start: toDay(span.start + step), due: toDay(span.end - DAY_MS + step) },
          );
        }}
        style={{
          left: xOf(live.start),
          width: Math.max(ppd * 0.8, xOf(live.end) - xOf(live.start)),
        }}
        className={cn(
          "absolute top-[7px] flex h-5 cursor-grab select-none items-center rounded-md border px-2 text-[11.5px] text-ink transition-[filter]",
          done
            ? "border-done-line bg-done-bg text-muted"
            : late
              ? "border-crit-text bg-warn-bg"
              : "border-doing-line bg-doing-bg",
          span.inferredStart && "border-l-dashed",
          span.inferredEnd && "border-r-dashed",
          critical && "ring-2 ring-warn-text ring-offset-1 ring-offset-app",
          drag && "z-20 cursor-grabbing shadow-[var(--dit-shadow)]",
        )}
      >
        <span className="truncate">
          {drag ? `${toDay(live.start)} → ${toDay(live.end - DAY_MS)}` : issue.title}
        </span>
        <i
          onPointerDown={begin("start")}
          onPointerMove={move}
          onPointerUp={finish}
          onPointerCancel={finish}
          title="Drag to change the start date"
          className="absolute inset-y-0 -left-1 w-2 cursor-ew-resize"
        />
        <i
          onPointerDown={begin("end")}
          onPointerMove={move}
          onPointerUp={finish}
          onPointerCancel={finish}
          title="Drag to change the due date"
          className="absolute inset-y-0 -right-1 w-2 cursor-ew-resize"
        />
      </div>
    </div>
  );
}
