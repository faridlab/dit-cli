// The Gantt sidebar section, and the display options it owns. The chart and
// this section sit far apart in the tree, so the options travel through a
// context the shell provides above both — zoom and grouping are view state
// shared by two surfaces, not a property of either.
//
// None of it is workspace data: what you are looking at is not a fact about
// the plan, so none of it is written to the repo.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { Btn, CheckSquare, Row, SectionHeading } from "../chrome";
import { useIssues, useSchema } from "../../lib/queries";
import { doneIds, isDone, POOL_LIMIT } from "../../lib/lists";
import type { GanttZoom } from "../../lib/schedule";

export type { GanttZoom } from "../../lib/schedule";
export type GanttGroupBy = "epic" | "assignee" | "none";

export interface GanttOptions {
  zoom: GanttZoom;
  groupBy: GanttGroupBy;
  /** Draw the `blocked_by` arrows. */
  deps: boolean;
  critical: boolean;
  showDone: boolean;
  weekends: boolean;
}

interface GanttOptionsValue extends GanttOptions {
  set: <K extends keyof GanttOptions>(key: K, value: GanttOptions[K]) => void;
}

const DEFAULTS: GanttOptions = {
  zoom: "week",
  groupBy: "epic",
  deps: true,
  critical: false,
  showDone: false,
  weekends: true,
};

const GanttOptionsContext = createContext<GanttOptionsValue | null>(null);

export function GanttOptionsProvider({ children }: { children: ReactNode }) {
  const [options, setOptions] = useState<GanttOptions>(DEFAULTS);
  const value = useMemo(
    () => ({
      ...options,
      set: <K extends keyof GanttOptions>(key: K, next: GanttOptions[K]) =>
        setOptions((current) => ({ ...current, [key]: next })),
    }),
    [options],
  );
  return <GanttOptionsContext.Provider value={value}>{children}</GanttOptionsContext.Provider>;
}

export function useGanttOptions(): GanttOptionsValue {
  const value = useContext(GanttOptionsContext);
  if (!value) throw new Error("useGanttOptions outside GanttOptionsProvider");
  return value;
}

const ZOOMS: Array<[GanttZoom, string]> = [
  ["day", "Day"],
  ["week", "Week"],
  ["month", "Month"],
];

const GROUPS: Array<[GanttGroupBy, string]> = [
  ["epic", "Epic"],
  ["assignee", "Assignee"],
  ["none", "Nothing"],
];

const SHOW: Array<[keyof Pick<GanttOptions, "deps" | "critical" | "showDone" | "weekends">, string]> = [
  ["deps", "Dependencies (blocked_by)"],
  ["critical", "Critical path"],
  ["showDone", "Done issues"],
  ["weekends", "Weekend shading"],
];

export function GanttPane() {
  const options = useGanttOptions();
  const issues = useIssues({ limit: POOL_LIMIT });
  const schema = useSchema();

  // How much of the open work has no dates at all — the number that says
  // whether the chart is a plan or a sample.
  const done = doneIds(schema.data?.workflow.statuses);
  const undated = (issues.data?.items ?? []).filter(
    (issue) => issue.type !== "story" && !isDone(issue, done) && !issue.start && !issue.due,
  ).length;

  return (
    <>
      <SectionHeading size="sm">Zoom</SectionHeading>
      <div className="sb-body">
        <div style={{ display: "flex", gap: 4, padding: "2px 8px 6px" }}>
          {ZOOMS.map(([zoom, label]) => (
            <Btn
              key={zoom}
              primary={options.zoom === zoom}
              onClick={() => options.set("zoom", zoom)}
              style={{ flex: 1, justifyContent: "center" }}
            >
              {label}
            </Btn>
          ))}
        </div>

        <SectionHeading size="sm">Group rows by</SectionHeading>
        {GROUPS.map(([key, label]) => (
          <Row key={key} on={options.groupBy === key} onClick={() => options.set("groupBy", key)}>
            <CheckSquare on={options.groupBy === key} radio />
            <span className="lbl">{label}</span>
          </Row>
        ))}

        <SectionHeading size="sm" className="mt-2">
          Show
        </SectionHeading>
        {SHOW.map(([key, label]) => (
          <Row key={key} onClick={() => options.set(key, !options[key])}>
            <CheckSquare on={options[key]} />
            <span className="lbl">{label}</span>
          </Row>
        ))}

        <SectionHeading size="sm" className="mt-2">
          How dates work
        </SectionHeading>
        <p
          className="empty"
          style={{ padding: "2px 8px 0", fontSize: 11.5, lineHeight: 1.5, color: "var(--muted)" }}
        >
          <span className="mono">start</span> and <span className="mono">due</span> live in the issue
          file. A missing side is inferred from the estimate (2 days per point) and drawn dashed — shown,
          never stored. Dragging a bar commits the real fields. {undated} open{" "}
          {undated === 1 ? "issue has" : "issues have"} no dates.
        </p>
      </div>
    </>
  );
}
