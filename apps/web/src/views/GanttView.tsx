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
//
// The markup follows the approved design's class recipes (styles.css,
// "Workbench recipes": `.gantt`, `.gleft`, `.gscroll`, `.gbar`, `.tray`) so
// the chart is the one that was signed off, not a re-interpretation of it.

import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import { Calendar, Layers, X } from "lucide-react";
import { toast } from "sonner";
import { useBulkPatchIssue, useIssues, usePatchIssue, useSchema, useStatus } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { useViewOptions } from "../lib/viewopts";
import { doneIds, filtersToDql, isDone, matchesFilters, POOL_LIMIT } from "../lib/lists";
import { routeToHash } from "../lib/router";
import {
  criticalPath,
  DAY_MS,
  dayStart,
  durationDays,
  epicSpan,
  ganttRange,
  isLate,
  monthSegments,
  scheduleSequentially,
  shortDate,
  spanOf,
  toDay,
  weekendOffsets,
  type Span,
} from "../lib/schedule";
import type { IssueDto, StatusCategory } from "../lib/types";
import { AssigneeCircles, IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";
import { Btn, HeadingNote, SectionHeading, Sp } from "../components/chrome";
import { cn } from "../lib/cn";
import { useGanttOptions } from "../components/panes/GanttPane";

const ROW = 34;
const HEAD = 30;
/** Where today sits after the canvas scrolls into place. */
const TODAY_OFFSET_PX = 240;

// ---------------------------------------------------------------------------
// Tooltip. One element, positioned by the pointer, shared by every bar and
// milestone on the plan views. It follows the pointer and flips to stay on
// screen, as the design does.
// ---------------------------------------------------------------------------

interface TipContent {
  head: string;
  body: ReactNode;
}

export interface Tip {
  /** Attach to a bar: pointerenter shows, pointermove follows, leave and press hide. */
  handlers: (content: () => TipContent) => {
    onPointerEnter: (event: ReactPointerEvent<HTMLElement>) => void;
    onPointerMove: (event: ReactPointerEvent<HTMLElement>) => void;
    onPointerLeave: () => void;
    onPointerDown: () => void;
  };
  hide: () => void;
  node: ReactNode;
}

export function useTip(): Tip {
  const [tip, setTip] = useState<(TipContent & { x: number; y: number }) | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  // The flip needs the rendered size, so it happens after layout.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !tip) return;
    let x = tip.x + 12;
    let y = tip.y + 14;
    if (x + el.offsetWidth > window.innerWidth - 8) x = tip.x - el.offsetWidth - 12;
    if (y + el.offsetHeight > window.innerHeight - 8) y = tip.y - el.offsetHeight - 12;
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
  }, [tip]);

  const hide = () => setTip(null);
  const handlers: Tip["handlers"] = (content) => ({
    onPointerEnter: (event) => setTip({ ...content(), x: event.clientX, y: event.clientY }),
    onPointerMove: (event) =>
      setTip((current) => (current ? { ...current, x: event.clientX, y: event.clientY } : current)),
    onPointerLeave: hide,
    onPointerDown: hide,
  });

  const node = tip
    ? createPortal(
        <div ref={ref} className="tip" role="tooltip">
          <b>{tip.head}</b>
          {tip.body ? <span>{tip.body}</span> : null}
        </div>,
        document.body,
      )
    : null;

  return { handlers, hide, node };
}

// ---------------------------------------------------------------------------
// Row model.
// ---------------------------------------------------------------------------

interface Group {
  key: string;
  label: string;
  items: IssueDto[];
  epic?: IssueDto;
}

type GanttRow =
  | { kind: "g"; group: Group; y: number }
  | { kind: "i"; issue: IssueDto; span: Span; y: number };

export function GanttView({ onOpen }: { onOpen: (id: string) => void }) {
  const issues = useIssues({ limit: POOL_LIMIT });
  const schema = useSchema();
  const status = useStatus();
  const { filters, toggleMine, toggleContext, toggleType } = useViewOptions();
  const options = useGanttOptions();
  // The tray schedules through the same endpoint every other edit uses: one
  // PATCH per issue, one commit per file on the server.
  const bulk = useBulkPatchIssue();
  const tip = useTip();
  const scroller = useRef<HTMLDivElement>(null);
  const leftRows = useRef<HTMLDivElement>(null);

  const me = status.data?.me ?? null;
  const statuses = schema.data?.workflow.statuses ?? [];
  const done = useMemo(() => doneIds(statuses), [statuses]);
  const categoryOf = useMemo(() => {
    const map = new Map(statuses.map((s) => [s.id, s.category]));
    return (id: string): StatusCategory => map.get(id) ?? "todo";
  }, [statuses]);

  const all = issues.data?.items ?? [];
  const today = dayStart(new Date().toISOString());
  const range = ganttRange(options.zoom, today);
  const width = range.days * range.ppd;
  const xOf = (ms: number) => ((ms - range.start) / DAY_MS) * range.ppd;

  const byId = useMemo(() => new Map(all.map((issue) => [issue.id, issue])), [all]);

  // The pool: work (not epics), through the shared filters, done hidden
  // unless asked for. Scheduled is anything with at least one date.
  const pool = useMemo(
    () =>
      all.filter(
        (issue) =>
          issue.type !== "story" &&
          matchesFilters(issue, filters, me) &&
          (options.showDone || !isDone(issue, done)),
      ),
    [all, filters, me, options.showDone, done],
  );
  const scheduled = useMemo(() => pool.filter((issue) => spanOf(issue) !== null), [pool]);
  const unscheduled = useMemo(() => pool.filter((issue) => spanOf(issue) === null), [pool]);

  const critical = useMemo(
    () => (options.critical ? criticalPath(scheduled) : new Set<string>()),
    [options.critical, scheduled],
  );

  const { rows, total } = useMemo(() => {
    let groups: Group[] = [];
    if (options.groupBy === "epic") {
      const epics = all.filter((issue) => issue.type === "story" && !issue.epic);
      const epicIds = new Set(epics.map((epic) => epic.id));
      groups = epics
        .map((epic) => ({
          key: epic.id,
          label: epic.title,
          epic,
          items: scheduled.filter((issue) => issue.epic === epic.id),
        }))
        .filter((group) => group.items.length > 0);
      const none = scheduled.filter((issue) => !issue.epic || !epicIds.has(issue.epic));
      if (none.length > 0) groups.push({ key: "none", label: "No epic", items: none });
    } else if (options.groupBy === "assignee") {
      const people = [...new Set(scheduled.flatMap((issue) => issue.assignees))].sort();
      groups = [...people, null]
        .map((person) => ({
          key: person ?? "none",
          label: person ?? "Unassigned",
          items: scheduled.filter((issue) =>
            person ? issue.assignees.includes(person) : issue.assignees.length === 0,
          ),
        }))
        .filter((group) => group.items.length > 0);
    } else {
      groups = [{ key: "all", label: "All scheduled", items: [...scheduled] }];
    }
    const byStart = (a: IssueDto, b: IssueDto) => (spanOf(a)?.start ?? 0) - (spanOf(b)?.start ?? 0);

    const out: GanttRow[] = [];
    let y = 0;
    for (const group of groups) {
      if (options.groupBy !== "none") {
        out.push({ kind: "g", group, y });
        y += HEAD;
      }
      for (const issue of [...group.items].sort(byStart)) {
        const span = spanOf(issue);
        if (!span) continue;
        out.push({ kind: "i", issue, span, y });
        y += ROW;
      }
    }
    return { rows: out, total: y };
  }, [all, options.groupBy, scheduled]);

  useRegisterPeekList(
    useMemo(
      () => rows.flatMap((row) => (row.kind === "i" ? [row.issue.short_ref] : [])),
      [rows],
    ),
  );

  // Today sits a little in from the left edge: on every zoom, and once the
  // canvas exists (it does not while the plan is still loading).
  const loaded = !issues.isPending;
  useEffect(() => {
    const el = scroller.current;
    if (el) el.scrollLeft = Math.max(0, xOf(today) - TODAY_OFFSET_PX);
    // xOf changes with the range, which is a function of zoom and today.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [options.zoom, today, loaded]);

  const chips = [
    ...(filters.mine ? [{ dql: "assignee = @me", remove: toggleMine }] : []),
    ...[...filters.contexts].sort().map((context) => ({
      dql: `label = context:${context}`,
      remove: () => toggleContext(context),
    })),
    ...[...filters.types].sort().map((type) => ({ dql: `type = ${type}`, remove: () => toggleType(type) })),
  ];
  const chipDql = filtersToDql(filters).join(" and ");

  if (issues.isPending) return <Loading label="Loading plan…" className="flex-1" />;
  if (issues.isError) {
    return (
      <ErrorBox error={issues.error} title="Could not load the plan" onRetry={() => void issues.refetch()} />
    );
  }

  // Bar geometry by issue id, for the dependency arrows.
  const positions = new Map<string, { x1: number; x2: number; yc: number }>();
  for (const row of rows) {
    if (row.kind === "i") {
      positions.set(row.issue.id, { x1: xOf(row.span.start), x2: xOf(row.span.end), yc: row.y + ROW / 2 });
    }
  }
  const deps: Array<{ key: string; d: string; bad: boolean }> = [];
  if (options.deps) {
    for (const row of rows) {
      if (row.kind !== "i") continue;
      for (const blocker of row.issue.blocked_by ?? []) {
        const a = positions.get(blocker);
        const b = positions.get(row.issue.id);
        const blockerIssue = byId.get(blocker);
        if (!a || !b || !blockerIssue) continue;
        const blockerSpan = spanOf(blockerIssue);
        const bad = blockerSpan !== null && blockerSpan.end > row.span.start;
        deps.push({
          key: `${blocker}>${row.issue.id}`,
          d: `M${a.x2} ${a.yc} H${a.x2 + 8} V${b.yc} H${b.x1 - 2}`,
          bad,
        });
      }
    }
  }

  const months = monthSegments(range.start, range.start + range.days * DAY_MS);
  const ticks: Array<{ key: number; left: number; width: number; label: string; we: boolean }> = [];
  if (options.zoom === "day") {
    for (let k = 0; k < range.days; k += 1) {
      const date = new Date(range.start + k * DAY_MS);
      ticks.push({
        key: k,
        left: k * range.ppd,
        width: range.ppd,
        label: String(date.getUTCDate()),
        we: date.getUTCDay() % 6 === 0,
      });
    }
  } else {
    for (let k = 0; k < range.days; k += 7) {
      const date = new Date(range.start + k * DAY_MS);
      ticks.push({
        key: k,
        left: k * range.ppd,
        width: 7 * range.ppd,
        label: options.zoom === "week" ? shortDate(date.getTime()) : String(date.getUTCDate()),
        we: false,
      });
    }
  }
  const weekends =
    options.weekends && options.zoom !== "month" ? weekendOffsets(range.start, range.days) : [];

  const scheduleOne = (issue: IssueDto) => {
    const [plan] = scheduleSequentially([issue], today);
    if (!plan) return;
    bulk.mutate([{ id: issue.short_ref, set: { start: plan.start, due: plan.due } }], {
      onSuccess: () => toast(`Scheduled · ${handleOf(issue)} ${shortDate(today)} → ${shortDate(dayStart(plan.due))}`),
    });
  };
  const scheduleAll = () => {
    const plans = scheduleSequentially(unscheduled, today);
    const refs = new Map(unscheduled.map((issue) => [issue.id, issue.short_ref]));
    bulk.mutate(
      plans.map((plan) => ({ id: refs.get(plan.id) ?? plan.id, set: { start: plan.start, due: plan.due } })),
      { onSuccess: () => toast("Scheduled back-to-back from today · one commit per field") },
    );
  };

  return (
    <>
      {chips.length > 0 ? (
        <div className="filters">
          {chips.map((chip) => (
            <span key={chip.dql} className="fchip">
              {chip.dql}
              <button type="button" className="rmf" title="Remove this filter" onClick={chip.remove}>
                <X className="i" aria-hidden />
              </button>
            </span>
          ))}
          <a className="dq" href={routeToHash({ name: "search", q: chipDql })} title="Run this exact query on the Search page">
            {chipDql}
          </a>
        </div>
      ) : null}

      <div className="gantt">
        <div className="gleft">
          <div className="gcorner">
            <span>
              {scheduled.length} scheduled · {unscheduled.length} unscheduled
            </span>
          </div>
          <div className="grows" ref={leftRows} style={{ height: total }}>
            {rows.map((row) =>
              row.kind === "g" ? (
                <div key={`g:${row.group.key}`} className="grow gh" style={{ top: row.y, height: HEAD }}>
                  {row.group.epic ? <TypeBadge type="story" /> : <Layers className="i" aria-hidden />}
                  <span className="lbl">{row.group.label}</span>
                  <span className="cnt">{row.group.items.length}</span>
                </div>
              ) : (
                <button
                  key={row.issue.id}
                  type="button"
                  className="grow"
                  style={{ top: row.y, height: ROW }}
                  onClick={() => onOpen(row.issue.short_ref)}
                >
                  <IssueHandle shortRef={row.issue.short_ref} number={row.issue.number} />
                  <TypeBadge type={row.issue.type} />
                  <PriorityDot priority={row.issue.priority} />
                  <span className="lbl">{row.issue.title}</span>
                  <AssigneeCircles assignees={row.issue.assignees} />
                </button>
              ),
            )}
          </div>
        </div>

        <div
          className="gscroll"
          ref={scroller}
          onScroll={(event) => {
            // The labels have no scrollbar of their own; they follow the
            // canvas so a row and its bar never drift apart.
            if (leftRows.current) leftRows.current.style.transform = `translateY(${-event.currentTarget.scrollTop}px)`;
          }}
        >
          <div className="gcanvas" style={{ width }}>
            <div className="axis">
              <div className="months">
                {months.map((month) => (
                  <span key={month.label} style={{ left: xOf(month.start), width: xOf(month.end) - xOf(month.start) }}>
                    {month.label}
                  </span>
                ))}
              </div>
              <div className="ticks">
                {ticks.map((tick) => (
                  <span key={tick.key} className={cn("tick", tick.we && "we")} style={{ left: tick.left, width: tick.width }}>
                    {tick.label}
                  </span>
                ))}
              </div>
            </div>
            <div className="gbody" style={{ height: total }}>
              {weekends.map((offset) => (
                <i key={offset} className="we" style={{ left: offset * range.ppd, width: range.ppd }} />
              ))}
              {rows.map((row) =>
                row.kind === "g" ? (
                  <i key={`line:${row.group.key}`} className="gline" style={{ top: row.y, height: HEAD }} />
                ) : null,
              )}
              <i className="today" style={{ left: xOf(today) }}>
                <b>today</b>
              </i>
              <svg className="deps" width={width} height={total}>
                <defs>
                  <marker id="gar" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
                    <path d="M0 0L10 5 0 10z" />
                  </marker>
                  <marker id="garb" viewBox="0 0 10 10" refX="8" refY="5" markerWidth="6" markerHeight="6" orient="auto">
                    <path d="M0 0L10 5 0 10z" />
                  </marker>
                </defs>
                {deps.map((dep) => (
                  <path key={dep.key} d={dep.d} className={cn("dep", dep.bad && "bad")} markerEnd={`url(#gar${dep.bad ? "b" : ""})`} />
                ))}
              </svg>
              {rows.map((row) => {
                if (row.kind === "g") {
                  if (!row.group.epic) return null;
                  const children = all.filter((issue) => issue.epic === row.group.epic!.id);
                  const resolved = epicSpan(row.group.epic, children);
                  if (!resolved) return null;
                  return (
                    <div
                      key={`span:${row.group.key}`}
                      className={cn("espan", resolved.derived && "derived")}
                      style={{
                        left: xOf(resolved.span.start),
                        width: Math.max(4, xOf(resolved.span.end) - xOf(resolved.span.start)),
                        top: row.y + 11,
                      }}
                      title={resolved.derived ? "Span derived from the children — never stored" : "Epic start → target"}
                    />
                  );
                }
                return (
                  <GanttBar
                    key={row.issue.id}
                    issue={row.issue}
                    span={row.span}
                    y={row.y}
                    ppd={range.ppd}
                    xOf={xOf}
                    today={today}
                    category={categoryOf(row.issue.status)}
                    done={isDone(row.issue, done)}
                    critical={critical.has(row.issue.id)}
                    tip={tip}
                    onOpen={onOpen}
                  />
                );
              })}
            </div>
          </div>
        </div>
      </div>

      <div className="tray">
        <SectionHeading>
          Unscheduled <HeadingNote>no start, no due — drag onto the chart or schedule from the estimate</HeadingNote>
          <Sp />
          {unscheduled.length > 0 ? (
            <Btn onClick={scheduleAll} disabled={bulk.isPending}>
              <Calendar className="i" aria-hidden />
              Schedule all from estimates
            </Btn>
          ) : null}
        </SectionHeading>
        <div className="trayrows">
          {unscheduled.length === 0 ? (
            <p className="empty" style={{ padding: "6px 0" }}>
              Everything visible has dates.
            </p>
          ) : (
            unscheduled.map((issue) => (
              <div key={issue.id} className="irow" style={{ gridTemplateColumns: "52px 16px 8px minmax(0,1fr) auto auto" }}>
                <IssueHandle shortRef={issue.short_ref} number={issue.number} />
                <TypeBadge type={issue.type} />
                <PriorityDot priority={issue.priority} />
                <button type="button" className="t" style={{ textAlign: "left" }} onClick={() => onOpen(issue.short_ref)}>
                  {issue.title}
                </button>
                <span className="est mono" style={{ fontSize: 11, color: "var(--muted)" }}>
                  {issue.estimate ? `${issue.estimate}pt → ${durationDays(issue)}d` : "no estimate"}
                </span>
                <Btn onClick={() => scheduleOne(issue)} disabled={bulk.isPending}>
                  <Calendar className="i" aria-hidden />
                  Schedule
                </Btn>
              </div>
            ))
          )}
        </div>
      </div>
      {tip.node}
    </>
  );
}

/** `#12` once numbered, the short ref until then — for toasts. */
function handleOf(issue: Pick<IssueDto, "number" | "short_ref">): string {
  return issue.number !== null ? `#${issue.number}` : issue.short_ref;
}

// ---------------------------------------------------------------------------
// One draggable bar. Dragging the middle moves both dates; dragging an edge
// moves that one. Nothing commits until the pointer is released, and a drag
// that ends where it started is a click that opens the issue.
// ---------------------------------------------------------------------------

type DragMode = "m" | "l" | "r";

interface Drag {
  mode: DragMode;
  x0: number;
  /** Live dates while dragging; null until the pointer has moved. */
  cur: { start: number; end: number } | null;
}

function GanttBar({
  issue,
  span,
  y,
  ppd,
  xOf,
  today,
  category,
  done,
  critical,
  tip,
  onOpen,
}: {
  issue: IssueDto;
  span: Span;
  y: number;
  ppd: number;
  xOf: (ms: number) => number;
  today: number;
  category: StatusCategory;
  done: boolean;
  critical: boolean;
  tip: Tip;
  onOpen: (id: string) => void;
}) {
  const patch = usePatchIssue(issue.short_ref);
  const [drag, setDrag] = useState<Drag | null>(null);

  const live = drag?.cur ?? { start: span.start, end: span.end };
  const late = isLate(span, done, today);
  const days = Math.round((span.end - span.start) / DAY_MS);
  // `end` is exclusive; the due date the file stores is the day before.
  const dueOf = (end: number) => end - DAY_MS;

  const commit = (start: number, end: number) => {
    const set: { start?: string; due?: string } = {};
    if (start !== span.start || !issue.start) set.start = toDay(start);
    if (end !== span.end || !issue.due) set.due = toDay(dueOf(end));
    if (!set.start && !set.due) return;
    patch.mutate(set, {
      onSuccess: () => toast(`Committed · ${handleOf(issue)} ${shortDate(start)} → ${shortDate(dueOf(end))}`),
    });
  };

  const onPointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    const mode: DragMode = target.classList.contains("gh-l") ? "l" : target.classList.contains("gh-r") ? "r" : "m";
    setDrag({ mode, x0: event.clientX, cur: null });
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
    tip.hide();
  };
  const onPointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    const moved = Math.round((event.clientX - drag.x0) / ppd) * DAY_MS;
    let start = span.start;
    let end = span.end;
    if (drag.mode === "m") {
      start += moved;
      end += moved;
    } else if (drag.mode === "l") {
      start = Math.min(span.end - DAY_MS, span.start + moved);
    } else {
      end = Math.max(span.start + DAY_MS, span.end + moved);
    }
    setDrag({ ...drag, cur: { start, end } });
  };
  const finish = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    const current = drag.cur;
    setDrag(null);
    if (!current || (current.start === span.start && current.end === span.end)) {
      if (event.type === "pointerup") onOpen(issue.short_ref);
      return;
    }
    commit(current.start, current.end);
  };
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Enter") onOpen(issue.short_ref);
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const step = (event.key === "ArrowRight" ? 1 : -1) * DAY_MS;
    if (event.shiftKey) commit(span.start, span.end + step);
    else commit(span.start + step, span.end + step);
  };

  const tipHandlers = tip.handlers(() => ({
    head: issue.title,
    body: (
      <>
        {toDay(span.start)} → {toDay(dueOf(span.end))} · {days}d
        {span.inferredStart || span.inferredEnd ? " · inferred from estimate" : ""}
        {late ? (
          <>
            {" · "}
            <b>past due</b>
          </>
        ) : null}
        {critical ? " · on the critical path" : ""}
      </>
    ),
  }));

  return (
    <div
      role="button"
      tabIndex={0}
      aria-label={`${issue.title}, ${toDay(span.start)} to ${toDay(dueOf(span.end))}`}
      className={cn(
        "gbar",
        category,
        critical && "crit",
        late && "late",
        span.inferredStart && "inf-s",
        span.inferredEnd && "inf-e",
        drag && "dragging",
      )}
      style={{
        left: xOf(live.start),
        width: Math.max(ppd * 0.8, xOf(live.end) - xOf(live.start)),
        top: y + 7,
      }}
      {...tipHandlers}
      onPointerDown={onPointerDown}
      onPointerMove={(event) => {
        tipHandlers.onPointerMove(event);
        onPointerMove(event);
      }}
      onPointerUp={finish}
      onPointerCancel={finish}
      onKeyDown={onKeyDown}
    >
      <span className="gl">
        {drag?.cur ? `${shortDate(live.start)} → ${shortDate(dueOf(live.end))}` : issue.title}
      </span>
      <i className="gh-l" title="Drag to change start" />
      <i className="gh-r" title="Drag to change due" />
    </div>
  );
}
