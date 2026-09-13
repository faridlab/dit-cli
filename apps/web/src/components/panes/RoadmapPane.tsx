// The Roadmap sidebar section and the options it owns: how far ahead to
// look, what a lane stands for, and the list of releases the milestones
// come from. View state shared between the sidebar and the chart, so it
// lives in a context above both — and nowhere near the repo, because "I am
// looking at six months" is not a fact about the plan.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { Btn, CheckSquare, HeadingNote, Row, SectionHeading, Sp } from "../chrome";
import { StatusPill } from "../badges";
import { useReleases } from "../../lib/queries";
import type { RoadmapHorizon } from "../../lib/schedule";
import type { ReleaseStatus } from "../../lib/types";

export type { RoadmapHorizon } from "../../lib/schedule";
export type RoadmapLanes = "epic" | "release" | "assignee";

export interface RoadmapOptions {
  horizon: RoadmapHorizon;
  lanes: RoadmapLanes;
  /** Release targets as diamonds on the axis. */
  milestones: boolean;
  progress: boolean;
}

interface RoadmapOptionsValue extends RoadmapOptions {
  set: <K extends keyof RoadmapOptions>(key: K, value: RoadmapOptions[K]) => void;
}

const DEFAULTS: RoadmapOptions = { horizon: "half", lanes: "epic", milestones: true, progress: true };

const RoadmapOptionsContext = createContext<RoadmapOptionsValue | null>(null);

export function RoadmapOptionsProvider({ children }: { children: ReactNode }) {
  const [options, setOptions] = useState<RoadmapOptions>(DEFAULTS);
  const value = useMemo(
    () => ({
      ...options,
      set: <K extends keyof RoadmapOptions>(key: K, next: RoadmapOptions[K]) =>
        setOptions((current) => ({ ...current, [key]: next })),
    }),
    [options],
  );
  return (
    <RoadmapOptionsContext.Provider value={value}>{children}</RoadmapOptionsContext.Provider>
  );
}

export function useRoadmapOptions(): RoadmapOptionsValue {
  const value = useContext(RoadmapOptionsContext);
  if (!value) throw new Error("useRoadmapOptions outside RoadmapOptionsProvider");
  return value;
}

const HORIZONS: Array<[RoadmapHorizon, string]> = [
  ["quarter", "Quarter"],
  ["half", "6 months"],
  ["year", "Year"],
];

const LANES: Array<[RoadmapLanes, string]> = [
  ["epic", "Epics"],
  ["release", "Releases"],
  ["assignee", "People"],
];

const SHOW: Array<[keyof Pick<RoadmapOptions, "milestones" | "progress">, string]> = [
  ["milestones", "Release targets"],
  ["progress", "Progress %"],
];

/** The pill tone for a release: shipped is done, planned is to do, and every
 *  state in between is in flight. */
export function releaseCategory(status: ReleaseStatus): "todo" | "doing" | "done" {
  if (status === "released") return "done";
  if (status === "planned") return "todo";
  return "doing";
}

export function RoadmapPane() {
  const options = useRoadmapOptions();
  const releases = useReleases();
  const list = releases.data ?? [];

  return (
    <>
      <SectionHeading size="sm">Horizon</SectionHeading>
      <div className="sb-body">
        <div style={{ display: "flex", gap: 4, padding: "2px 8px 6px" }}>
          {HORIZONS.map(([horizon, label]) => (
            <Btn
              key={horizon}
              primary={options.horizon === horizon}
              onClick={() => options.set("horizon", horizon)}
              style={{ flex: 1, justifyContent: "center" }}
            >
              {label}
            </Btn>
          ))}
        </div>

        <SectionHeading size="sm">Lanes</SectionHeading>
        {LANES.map(([key, label]) => (
          <Row key={key} on={options.lanes === key} onClick={() => options.set("lanes", key)}>
            <CheckSquare on={options.lanes === key} radio />
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
          Releases
          <Sp />
          {/* The heading is uppercase; a path is not. */}
          <HeadingNote className="font-mono text-[10.5px] font-normal normal-case tracking-normal text-faint">
            .dit/releases/
          </HeadingNote>
        </SectionHeading>
        {list.length === 0 ? (
          <p className="empty" style={{ padding: "4px 8px" }}>
            No releases yet — add .dit/releases/&lt;version&gt;/release.md
          </p>
        ) : (
          list.map((release) => (
            <Row
              key={release.version}
              title={`${release.includes.length} issues · target ${release.target ?? "unset"}`}
              onClick={() => options.set("lanes", "release")}
            >
              <StatusPill
                category={releaseCategory(release.status)}
                label={release.status.replace("_", " ")}
                className="h-4 px-[5px] text-[10.5px]"
              />
              <span className="lbl mono">{release.version}</span>
              <span className="cnt">{release.target ? release.target.slice(5) : "—"}</span>
            </Row>
          ))
        )}
      </div>
    </>
  );
}
