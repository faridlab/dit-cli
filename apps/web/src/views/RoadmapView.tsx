// Epics across quarters: the altitude above the Gantt, where the unit is an
// outcome rather than a task.
//
// An epic's bar is its own `start` and `due` when it has them, and otherwise
// the span of the issues inside it — derived on read, never written back, so
// a roadmap can never drift from the work underneath it. Progress is the
// same: counted from the children's statuses every time the screen renders.
//
// Release lanes belong here too, and they wait for the release model in
// DESIGN.md §15 — a roadmap that claimed a version shipped without git
// proving it would be exactly the kind of record-keeping DIT exists to
// replace.

import { useMemo } from "react";
import { useIssues, useSchema } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import {
  coveringSpan,
  DAY_MS,
  dayStart,
  epicSpan,
  isLate,
  spanOf,
  type Span,
} from "../lib/schedule";
import type { IssueDto } from "../lib/types";
import { AssigneeCircles, IssueHandle } from "../components/badges";
import { Empty, ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import { useRoadmapOptions } from "../components/panes/RoadmapPane";

const PAGE_SIZE = 500;
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const HORIZON_MONTHS = { quarter: 3, half: 6, year: 12 } as const;
const HORIZON_BEFORE = { quarter: 0, half: 2, year: 3 } as const;

interface Lane {
  key: string;
  label: string;
  issues: IssueDto[];
  span: Span;
  derived: boolean;
  owners: string[];
  done: number;
  doing: number;
  epic?: IssueDto;
}

export function RoadmapView({ onOpen }: { onOpen: (id: string) => void }) {
  const issues = useIssues({ limit: PAGE_SIZE });
  const schema = useSchema();
  const options = useRoadmapOptions();

  const statuses = schema.data?.workflow.statuses ?? [];
  const categoryOf = useMemo(() => {
    const map = new Map(statuses.map((status) => [status.id, status.category]));
    return (id: string) => map.get(id) ?? "todo";
  }, [statuses]);

  const all = issues.data?.items ?? [];
  const today = dayStart(new Date().toISOString());

  // The window: whole months, so quarter boundaries land on real edges.
  const window = useMemo(() => {
    const now = new Date(today);
    const first = Date.UTC(
      now.getUTCFullYear(),
      now.getUTCMonth() - HORIZON_BEFORE[options.horizon],
      1,
    );
    const last = Date.UTC(
      new Date(first).getUTCFullYear(),
      new Date(first).getUTCMonth() + HORIZON_MONTHS[options.horizon],
      1,
    );
    return { from: first, to: last };
  }, [options.horizon, today]);

  const pct = (ms: number) =>
    Math.max(0, Math.min(100, ((ms - window.from) / (window.to - window.from)) * 100));

  const months = useMemo(() => {
    const out: Array<{ left: number; width: number; label: string; quarter: string }> = [];
    let cursor = window.from;
    while (cursor < window.to) {
      const date = new Date(cursor);
      const next = Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1);
      out.push({
        left: pct(cursor),
        width: pct(Math.min(next, window.to)) - pct(cursor),
        label: MONTHS[date.getUTCMonth()] ?? "",
        quarter: `Q${Math.floor(date.getUTCMonth() / 3) + 1} ${date.getUTCFullYear()}`,
      });
      cursor = next;
    }
    return out;
  }, [window]);

  const quarters = useMemo(() => {
    const out: Array<{ left: number; width: number; label: string }> = [];
    for (const month of months) {
      const open = out[out.length - 1];
      if (open && open.label === month.quarter) open.width += month.width;
      else out.push({ left: month.left, width: month.width, label: month.quarter });
    }
    return out;
  }, [months]);

  const lanes = useMemo<Lane[]>((): Lane[] => {
    const counted = (group: IssueDto[]) => ({
      done: group.filter((issue) => categoryOf(issue.status) === "done").length,
      doing: group.filter((issue) => categoryOf(issue.status) === "doing").length,
    });

    if (options.lanes === "assignee") {
      const people = [...new Set(all.flatMap((issue) => issue.assignees))].sort();
      return people
        .map((person): Lane | null => {
          const group = all.filter(
            (issue) => issue.assignees.includes(person) && issue.type !== "story",
          );
          const span = coveringSpan(group.map(spanOf));
          if (!span) return null;
          return {
            key: person,
            label: person,
            issues: group,
            span,
            derived: true,
            owners: [person],
            ...counted(group),
          };
        })
        .filter((lane): lane is Lane => lane !== null);
    }

    // Epic lanes: a top-level story, with the issues that name it.
    return all
      .filter((issue) => issue.type === "story")
      .map((epic): Lane | null => {
        const children = all.filter((issue) => issue.epic === epic.id);
        const resolved = epicSpan(epic, children);
        if (!resolved) return null;
        return {
          key: epic.id,
          label: epic.title,
          issues: children,
          span: resolved.span,
          derived: resolved.derived,
          owners: epic.assignees,
          epic,
          ...counted(children),
        };
      })
      .filter((lane): lane is Lane => lane !== null)
      .sort((a, b) => a.span.start - b.span.start);
  }, [all, categoryOf, options.lanes]);

  useRegisterPeekList(
    useMemo(
      () => lanes.map((lane) => lane.epic?.short_ref).filter((ref): ref is string => !!ref),
      [lanes],
    ),
  );

  if (issues.isPending) return <Loading label="Loading roadmap…" className="flex-1" />;
  if (issues.isError) {
    return (
      <ErrorBox
        error={issues.error}
        title="Could not load the roadmap"
        onRetry={() => void issues.refetch()}
      />
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <h1 className="shrink-0 text-lg font-semibold text-ink">Roadmap</h1>
        <span className="font-mono text-[11px] text-dim">
          {lanes.length} {lanes.length === 1 ? "lane" : "lanes"} · spans without dates are derived
          from the work inside
        </span>
      </header>

      {lanes.length === 0 ? (
        <Empty
          title="Nothing on the roadmap yet"
          hint="Give an epic a start and a target, or schedule one of the issues inside it."
          className="flex-1 justify-center"
        />
      ) : (
        <div className="min-h-0 flex-1 overflow-auto">
          {/* The axis. */}
          <div className="sticky top-0 z-10 flex border-b border-edge bg-app">
            <div className="w-[260px] shrink-0 min-[1200px]:w-[320px]" />
            <div className="relative mr-6 h-[52px] flex-1">
              <div className="relative h-[26px]">
                {quarters.map((quarter) => (
                  <span
                    key={quarter.label}
                    style={{ left: `${quarter.left}%`, width: `${quarter.width}%` }}
                    className="absolute top-0 truncate border-l border-ctl pl-2 text-[12px] font-semibold leading-6 text-ink"
                  >
                    {quarter.label}
                  </span>
                ))}
              </div>
              <div className="relative h-[22px]">
                {months.map((month) => (
                  <span
                    key={`${month.quarter}-${month.label}`}
                    style={{ left: `${month.left}%`, width: `${month.width}%` }}
                    className="absolute top-0 truncate border-l border-edge pl-2 font-mono text-[11px] leading-5 text-muted"
                  >
                    {month.label}
                  </span>
                ))}
                <i
                  style={{ left: `${pct(today)}%` }}
                  className="absolute -top-[26px] bottom-0 border-l-2 border-dashed border-accent"
                  aria-hidden
                />
              </div>
            </div>
          </div>

          {/* The lanes. */}
          <div className="flex flex-col">
            {lanes.map((lane) => {
              const total = lane.issues.length || 1;
              const donePct = (lane.done / total) * 100;
              const doingPct = (lane.doing / total) * 100;
              const left = pct(lane.span.start);
              const width = Math.max(1.2, pct(lane.span.end) - left);
              const finished = lane.done === lane.issues.length && lane.issues.length > 0;
              const late = isLate(lane.span, finished, today);

              return (
                <div key={lane.key} className="flex min-h-[58px] border-b border-edge hover:bg-hover">
                  <div className="flex w-[260px] shrink-0 flex-col justify-center gap-1 px-4 min-[1200px]:w-[320px]">
                    <button
                      type="button"
                      disabled={!lane.epic}
                      onClick={() => lane.epic && onOpen(lane.epic.short_ref)}
                      className="flex min-w-0 items-center gap-2 text-left text-[13px] font-medium text-ink disabled:cursor-default"
                    >
                      {lane.epic ? (
                        <IssueHandle shortRef={lane.epic.short_ref} number={lane.epic.number} />
                      ) : null}
                      <span className={cn("truncate", lane.epic && "hover:underline")}>
                        {lane.label}
                      </span>
                    </button>
                    <div className="flex items-center gap-2 text-[11px] text-muted">
                      <AssigneeCircles assignees={lane.owners} />
                      <span className="font-mono">
                        {lane.done}/{lane.issues.length} done
                      </span>
                      {lane.derived ? (
                        <span
                          className="font-mono text-faint"
                          title="This span is the span of the issues inside — it is not stored anywhere"
                        >
                          derived
                        </span>
                      ) : null}
                    </div>
                  </div>

                  <div className="relative mr-6 flex-1">
                    {months.map((month) => (
                      <i
                        key={`${lane.key}-${month.quarter}-${month.label}`}
                        style={{ left: `${month.left}%` }}
                        className="absolute inset-y-0 border-l border-edge"
                        aria-hidden
                      />
                    ))}
                    <i
                      style={{ left: `${pct(today)}%` }}
                      className="absolute inset-y-0 border-l border-accent/60"
                      aria-hidden
                    />
                    <button
                      type="button"
                      disabled={!lane.epic}
                      onClick={() => lane.epic && onOpen(lane.epic.short_ref)}
                      title={`${new Date(lane.span.start).toISOString().slice(0, 10)} → ${new Date(
                        lane.span.end - DAY_MS,
                      )
                        .toISOString()
                        .slice(0, 10)}${lane.derived ? " · derived from the issues inside" : ""}`}
                      style={{ left: `${left}%`, width: `${width}%` }}
                      className={cn(
                        "absolute top-1/2 flex h-6 -translate-y-1/2 items-center justify-end overflow-hidden rounded-md border px-2 font-mono text-[10.5px] text-ink-2",
                        lane.derived ? "border-dashed border-ctl" : "border-ctl bg-todo-bg",
                        late && "border-crit-text",
                      )}
                    >
                      <i
                        style={{ width: `${donePct}%` }}
                        className="absolute inset-y-0 left-0 bg-done-text/50"
                        aria-hidden
                      />
                      <i
                        style={{ left: `${donePct}%`, width: `${doingPct}%` }}
                        className="absolute inset-y-0 bg-doing-text/45"
                        aria-hidden
                      />
                      {options.progress && lane.issues.length > 0 ? (
                        <span className="relative">{Math.round(donePct)}%</span>
                      ) : null}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>

          <p className="px-5 py-4 text-[11.5px] leading-relaxed text-muted">
            Release lanes — which version each epic lands in, and whether git can prove it
            shipped — arrive with the release model. Until then this reads the work itself.
          </p>
        </div>
      )}
    </div>
  );
}
