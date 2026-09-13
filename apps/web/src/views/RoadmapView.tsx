// Epics, releases and people across quarters: the altitude above the Gantt,
// where the unit is an outcome rather than a task.
//
// An epic's bar is its own `start` and `due` when it has them, and otherwise
// the span of the issues inside it — derived on read, never written back, so
// a roadmap can never drift from the work underneath it. Progress is the
// same: counted from the children's statuses every time the screen renders.
// A release lane runs from the previous target to its own; the diamond on the
// axis is the `target` field in its release.md, and moving it is a commit.
//
// The markup follows the approved design's class recipes (styles.css,
// "Workbench recipes": `.road`, `.rhead`, `.rlane`, `.rbar`, `.rfoot`).

import { useMemo, type ReactNode } from "react";
import { Copy, List, Plus, Tag, User } from "lucide-react";
import { toast } from "sonner";
import { useIssues, usePatchRelease, useReleases, useSchema } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { navigate } from "../lib/router";
import { doneIds, isDone } from "../lib/lists";
import {
  coveringSpan,
  DAY_MS,
  dayStart,
  epicSpan,
  monthSegments,
  MONTHS,
  roadmapRange,
  shortDate,
  spanOf,
} from "../lib/schedule";
import type { IssueDto, ReleaseDto, StatusCategory } from "../lib/types";
import { AssigneeCircles, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";
import { Btn, MenuButton, Sp, type MenuItem } from "../components/chrome";
import { cn } from "../lib/cn";
import { useRoadmapOptions } from "../components/panes/RoadmapPane";
import { useTip } from "./GanttView";

const PAGE_SIZE = 500;
/** A first release with nothing before it gets a six-week run-up. */
const FIRST_RELEASE_RUNUP_DAYS = 42;
/** Milestones a month either side of the window still get a diamond. */
const MILESTONE_SLACK_DAYS = 30;

/** Milestone tone by release status. */
const RELEASE_TONE: Record<ReleaseDto["status"], StatusCategory> = {
  released: "done",
  in_uat: "doing",
  in_dev: "doing",
  planned: "todo",
  rolled_back: "todo",
};

interface Lane {
  key: string;
  label: string;
  items: IssueDto[];
  /** Left edge and exclusive right edge of the bar. */
  span: { start: number; end: number };
  /** The last day the bar stands for, as the tooltip reads it. */
  lastDay: number;
  derived: boolean;
  meta: string;
  /** Category the lateness rule reads: done lanes are never late. */
  category: StatusCategory;
  owners?: string[];
  epic?: IssueDto;
  release?: ReleaseDto;
  /** The release an epic ships in, for the chip. */
  shipsIn?: ReleaseDto;
  /** A people lane names the person, for the search it opens. */
  person?: string;
}

export function RoadmapView({ onOpen }: { onOpen: (id: string) => void }) {
  const issues = useIssues({ limit: PAGE_SIZE });
  const schema = useSchema();
  const releases = useReleases();
  const patchRelease = usePatchRelease();
  const options = useRoadmapOptions();
  const tip = useTip();

  const statuses = schema.data?.workflow.statuses ?? [];
  const done = useMemo(() => doneIds(statuses), [statuses]);
  const categoryOf = useMemo(() => {
    const map = new Map(statuses.map((status) => [status.id, status.category]));
    return (id: string): StatusCategory => map.get(id) ?? "todo";
  }, [statuses]);

  const all = issues.data?.items ?? [];
  const releaseList = useMemo(
    () =>
      [...(releases.data ?? [])].sort(
        (a, b) => (a.target ?? "9").localeCompare(b.target ?? "9") || a.version.localeCompare(b.version),
      ),
    [releases.data],
  );

  const today = dayStart(new Date().toISOString());
  const range = roadmapRange(options.horizon, today);
  const pct = (ms: number) => Math.max(0, Math.min(100, ((ms - range.start) / (range.end - range.start)) * 100));

  const months = monthSegments(range.start, range.end).map((month) => ({
    ...month,
    left: pct(month.start),
    width: pct(month.end) - pct(month.start),
    short: MONTHS[month.month] ?? "",
    quarter: `Q${Math.floor(month.month / 3) + 1} ${month.year}`,
  }));
  const quarters: Array<{ key: string; left: number; width: number }> = [];
  for (const month of months) {
    const open = quarters.find((quarter) => quarter.key === month.quarter);
    if (open) open.width += month.width;
    else quarters.push({ key: month.quarter, left: month.left, width: month.width });
  }

  const milestones = releaseList.filter((release) => {
    if (!release.target) return false;
    const at = dayStart(release.target);
    return at >= range.start - MILESTONE_SLACK_DAYS * DAY_MS && at <= range.end + MILESTONE_SLACK_DAYS * DAY_MS;
  });

  const lanes = useMemo<Lane[]>(() => {
    const count = (items: IssueDto[]) => `${items.filter((issue) => isDone(issue, done)).length}/${items.length} done`;

    if (options.lanes === "release") {
      return releaseList.flatMap((release, index): Lane[] => {
        if (!release.target) return [];
        const items = all.filter((issue) => release.includes.includes(issue.id));
        const previous = releaseList[index - 1];
        const end = dayStart(release.target);
        const start = previous?.target ? dayStart(previous.target) : end - FIRST_RELEASE_RUNUP_DAYS * DAY_MS;
        return [
          {
            key: release.version,
            label: release.version,
            items,
            span: { start, end },
            lastDay: end,
            derived: false,
            meta: count(items),
            category: RELEASE_TONE[release.status],
            release,
          },
        ];
      });
    }

    if (options.lanes === "assignee") {
      const people = [...new Set(all.flatMap((issue) => issue.assignees))].sort();
      return people.flatMap((person): Lane[] => {
        const items = all.filter(
          (issue) => issue.assignees.includes(person) && issue.type !== "story" && spanOf(issue) !== null,
        );
        const span = coveringSpan(items.map(spanOf));
        if (!span) return [];
        return [
          {
            key: person,
            label: person,
            items,
            span,
            lastDay: span.end - DAY_MS,
            derived: true,
            meta: count(items),
            category: "doing",
            owners: [person],
            person,
          },
        ];
      });
    }

    return all
      .filter((issue) => issue.type === "story" && !issue.epic)
      .flatMap((epic): Lane[] => {
        const kids = all.filter((issue) => issue.epic === epic.id);
        const resolved = epicSpan(epic, kids);
        if (!resolved) return [];
        return [
          {
            key: epic.id,
            label: epic.title,
            items: kids,
            span: resolved.span,
            lastDay: resolved.span.end - DAY_MS,
            derived: resolved.derived,
            meta: count(kids),
            category: categoryOf(epic.status),
            owners: epic.assignees,
            epic,
            shipsIn: releaseList.find((release) => release.includes.includes(epic.id)),
          },
        ];
      });
  }, [all, categoryOf, done, options.lanes, releaseList]);

  useRegisterPeekList(
    useMemo(() => lanes.flatMap((lane) => (lane.epic ? [lane.epic.short_ref] : [])), [lanes]),
  );

  if (issues.isPending) return <Loading label="Loading roadmap…" className="flex-1" />;
  if (issues.isError) {
    return (
      <ErrorBox error={issues.error} title="Could not load the roadmap" onRetry={() => void issues.refetch()} />
    );
  }

  /** The milestone menu: what the release is, where its issues are, and
   *  the one field a person moves from here. */
  const releaseMenu = (release: ReleaseDto): MenuItem[] => [
    { kind: "head", label: `${release.version} · ${release.status.replace("_", " ")}` },
    {
      kind: "text",
      node: (
        <>
          Target <span className="mono">{release.target ?? "unset"}</span>
          {release.target_ref ? (
            <>
              {" · "}
              <span className="mono">{release.target_ref}</span>
            </>
          ) : null}
          <br />
          {release.includes.length} issues in scope
        </>
      ),
    },
    {
      label: "Show issues in this release",
      icon: <List className="i" aria-hidden />,
      disabled: release.includes.length === 0,
      // Ids are ULIDs and may start with a digit, which DQL reads as a
      // number — quoting keeps them strings. The Issues list runs its query
      // as DQL as written; Search first guesses whether text is DQL, and a
      // bare `IN (…)` does not pass that guess yet.
      run: () =>
        navigate({ name: "search", q: `id IN (${release.includes.map((id) => `"${id}"`).join(", ")})` }),
    },
    {
      label: "Copy release path",
      icon: <Copy className="i" aria-hidden />,
      run: () => {
        (navigator.clipboard?.writeText(release.path) ?? Promise.reject(new Error("no clipboard"))).then(
          () => toast(`Copied — ${release.path}`),
          () => toast(`Copy: ${release.path}`),
        );
      },
    },
    { kind: "sep" },
    {
      kind: "input",
      placeholder: "Move target date",
      type: "date",
      value: release.target ?? "",
      button: "Set",
      run: (value) => {
        if (!value || value === release.target) return;
        patchRelease.mutate(
          { version: release.version, patch: { target: value } },
          { onSuccess: () => toast(`Committed ${release.path} · target ${value}`) },
        );
      },
    },
  ];

  /** What a click on a lane does: an epic opens, a release shows its menu,
   *  a person opens the search for their work. */
  const laneAction = (lane: Lane, child: ReactNode, className: string, style?: React.CSSProperties, extra?: object) => {
    if (lane.release) {
      return (
        <MenuButton items={releaseMenu(lane.release)}>
          <button type="button" className={className} style={style} {...extra}>
            {child}
          </button>
        </MenuButton>
      );
    }
    const onClick = lane.epic
      ? () => onOpen(lane.epic!.short_ref)
      : () => navigate({ name: "search", q: `assignee = ${lane.person}` });
    return (
      <button type="button" className={className} style={style} onClick={onClick} {...extra}>
        {child}
      </button>
    );
  };

  return (
    <>
      <div className="road">
        <div className="rhead">
          <div className="rlbl" />
          <div className="rax">
            <div className="rq">
              {quarters.map((quarter) => (
                <span key={quarter.key} style={{ left: `${quarter.left}%`, width: `${quarter.width}%` }}>
                  {quarter.key}
                </span>
              ))}
            </div>
            <div className="rm">
              {months.map((month) => (
                <span key={month.label} style={{ left: `${month.left}%`, width: `${month.width}%` }}>
                  {month.short}
                </span>
              ))}
            </div>
            {options.milestones ? (
              <div className="rmil">
                {milestones.map((release) => (
                  <MenuButton key={release.version} items={releaseMenu(release)}>
                    <button
                      type="button"
                      className={cn("mil", RELEASE_TONE[release.status])}
                      style={{ left: `${pct(dayStart(release.target ?? ""))}%` }}
                      {...tip.handlers(() => ({
                        head: `${release.version} · ${release.status.replace("_", " ")}`,
                        body: `target ${release.target} · ${release.includes.length} issues in scope`,
                      }))}
                    >
                      <i />
                      <span>{release.version}</span>
                    </button>
                  </MenuButton>
                ))}
              </div>
            ) : null}
            <i className="today" style={{ left: `${pct(today)}%` }}>
              <b>today</b>
            </i>
          </div>
        </div>

        <div className="rlanes">
          {lanes.length === 0 ? (
            <p className="empty" style={{ padding: 20 }}>
              No lanes in this horizon.
            </p>
          ) : (
            lanes.map((lane) => {
              const doneCount = lane.items.filter((issue) => isDone(issue, done)).length;
              const doing = lane.items.filter((issue) => categoryOf(issue.status) === "doing").length;
              const n = lane.items.length || 1;
              const left = pct(lane.span.start);
              const width = Math.max(1.5, pct(lane.span.end) - left);
              const late = lane.category !== "done" && lane.span.end < today;
              const icon = lane.epic ? <TypeBadge type="story" /> : lane.release ? <Tag className="i" aria-hidden /> : <User className="i" aria-hidden />;
              const nameTitle = lane.epic ? "Open the epic" : lane.release ? "Release options" : `Search ${lane.label}'s work`;
              return (
                <div key={lane.key} className="rlane">
                  <div className="rlbl">
                    {laneAction(
                      lane,
                      <>
                        {icon}
                        <span className="lbl">{lane.label}</span>
                      </>,
                      cn("rname", "open"),
                      undefined,
                      { title: nameTitle },
                    )}
                    <span className="rmeta">
                      {lane.owners ? <AssigneeCircles assignees={lane.owners} /> : null}
                      <span className="mono">{lane.meta}</span>
                      {lane.shipsIn && options.lanes === "epic" ? <span className="chip">{lane.shipsIn.version}</span> : null}
                    </span>
                  </div>
                  <div className="rtrack">
                    {months.map((month) => (
                      <i key={month.label} className="rgrid" style={{ left: `${month.left}%` }} />
                    ))}
                    <i className="today" style={{ left: `${pct(today)}%` }} />
                    {laneAction(
                      lane,
                      <>
                        <i className="fill done" style={{ width: `${(doneCount / n) * 100}%` }} />
                        <i className="fill doing" style={{ left: `${(doneCount / n) * 100}%`, width: `${(doing / n) * 100}%` }} />
                        <span className="rlabel">{options.progress ? `${Math.round((doneCount / n) * 100)}%` : ""}</span>
                      </>,
                      cn("rbar", lane.derived && "derived", late && "late", "open"),
                      { left: `${left}%`, width: `${width}%` },
                      tip.handlers(() => ({
                        head: lane.label,
                        body: (
                          <>
                            {shortDate(lane.span.start)} → {shortDate(lane.lastDay)}
                            {lane.derived ? " · span derived from children" : ""} · {doneCount} done, {doing} in flight,{" "}
                            {lane.items.length - doneCount - doing} to do
                            {late ? (
                              <>
                                {" · "}
                                <b>past target</b>
                              </>
                            ) : null}
                          </>
                        ),
                      })),
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>

        <div className="rfoot">
          <span className="legend">
            <i className="sw done" />
            done <i className="sw doing" />
            in flight <i className="sw todo" />
            to do <i className="sw derived" />
            span derived from children <i className="sw mil" />
            release target
          </span>
          <Sp />
          <Btn onClick={() => navigate({ name: "new-issue", type: "story" })}>
            <Plus className="i" aria-hidden />
            New epic
          </Btn>
        </div>
      </div>
      {tip.node}
    </>
  );
}
