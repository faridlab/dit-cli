// The Roadmap sidebar section and the options it owns: how far ahead to
// look, and what a lane stands for. View state shared between the sidebar
// and the chart, so it lives in a context above both — and nowhere near the
// repo, because "I am looking at six months" is not a fact about the plan.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { CheckSquare, SectionHeading } from "../chrome";
import { cn } from "../../lib/cn";

export type RoadmapHorizon = "quarter" | "half" | "year";
export type RoadmapLanes = "epic" | "assignee";

export interface RoadmapOptions {
  horizon: RoadmapHorizon;
  lanes: RoadmapLanes;
  progress: boolean;
}

interface RoadmapOptionsValue extends RoadmapOptions {
  set: <K extends keyof RoadmapOptions>(key: K, value: RoadmapOptions[K]) => void;
}

const DEFAULTS: RoadmapOptions = { horizon: "half", lanes: "epic", progress: true };

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

const HORIZONS: Array<{ value: RoadmapHorizon; label: string }> = [
  { value: "quarter", label: "Quarter" },
  { value: "half", label: "6 months" },
  { value: "year", label: "Year" },
];

const LANES: Array<{ value: RoadmapLanes; label: string; hint: string }> = [
  { value: "epic", label: "Epics", hint: "one lane per epic, spanning the work inside it" },
  { value: "assignee", label: "People", hint: "one lane per person, spanning their scheduled work" },
];

export function RoadmapPane() {
  const options = useRoadmapOptions();

  return (
    <div className="flex flex-col gap-4 p-3">
      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Horizon
        </SectionHeading>
        <div className="flex gap-1 px-1">
          {HORIZONS.map((horizon) => (
            <button
              key={horizon.value}
              type="button"
              onClick={() => options.set("horizon", horizon.value)}
              className={cn(
                "flex-1 rounded-md border px-2 py-1 text-[12px] transition-colors",
                options.horizon === horizon.value
                  ? "border-accent bg-accent text-on-accent"
                  : "border-edge text-ink-2 hover:border-ctl hover:text-ink",
              )}
            >
              {horizon.label}
            </button>
          ))}
        </div>
      </section>

      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Lanes
        </SectionHeading>
        {LANES.map((lane) => (
          <button
            key={lane.value}
            type="button"
            title={lane.hint}
            onClick={() => options.set("lanes", lane.value)}
            className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-hover"
          >
            <span className="[&>span]:rounded-full">
              <CheckSquare on={options.lanes === lane.value} />
            </span>
            <span className="text-[12.5px] text-ink-2">{lane.label}</span>
          </button>
        ))}
      </section>

      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Show
        </SectionHeading>
        <button
          type="button"
          onClick={() => options.set("progress", !options.progress)}
          className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-hover"
        >
          <CheckSquare on={options.progress} />
          <span className="text-[12.5px] text-ink-2">Progress %</span>
        </button>
      </section>

      <p className="px-1 text-[11.5px] leading-relaxed text-muted">
        An epic's bar is its own <span className="font-mono">start</span> and{" "}
        <span className="font-mono">due</span> when it has them, otherwise the span of the issues
        inside it. Progress is counted from those issues on every read — neither is stored.
      </p>
    </div>
  );
}
