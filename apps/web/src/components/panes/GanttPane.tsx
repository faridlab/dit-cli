// The Gantt sidebar section, and the display options it owns. The chart and
// this section sit far apart in the tree, so the options travel through a
// context the shell provides above both — zoom and grouping are view state
// shared by two surfaces, not a property of either.
//
// None of it is workspace data: what you are looking at is not a fact about
// the plan, so none of it is written to the repo.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { CheckSquare, SectionHeading } from "../chrome";
import { cn } from "../../lib/cn";

export type GanttZoom = "day" | "week" | "month";
export type GanttGroupBy = "epic" | "assignee" | "none";

export interface GanttOptions {
  zoom: GanttZoom;
  groupBy: GanttGroupBy;
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

const ZOOMS: Array<{ value: GanttZoom; label: string }> = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
];

const GROUPS: Array<{ value: GanttGroupBy; label: string; hint?: string }> = [
  { value: "epic", label: "Epic" },
  { value: "assignee", label: "Assignee" },
  { value: "none", label: "Nothing" },
];

function Row({
  label,
  on,
  onClick,
  title,
  round,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  title?: string;
  round?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      className="flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-hover"
    >
      <span className={cn(round && "[&>span]:rounded-full")}>
        <CheckSquare on={on} />
      </span>
      <span className="text-[12.5px] text-ink-2">{label}</span>
    </button>
  );
}

export function GanttPane() {
  const options = useGanttOptions();

  return (
    <div className="flex flex-col gap-4 p-3">
      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Zoom
        </SectionHeading>
        <div className="flex gap-1 px-1">
          {ZOOMS.map((zoom) => (
            <button
              key={zoom.value}
              type="button"
              onClick={() => options.set("zoom", zoom.value)}
              className={cn(
                "flex-1 rounded-md border px-2 py-1 text-[12px] transition-colors",
                options.zoom === zoom.value
                  ? "border-accent bg-accent text-on-accent"
                  : "border-edge text-ink-2 hover:border-ctl hover:text-ink",
              )}
            >
              {zoom.label}
            </button>
          ))}
        </div>
      </section>

      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Group rows by
        </SectionHeading>
        {GROUPS.map((group) => (
          <Row
            key={group.value}
            label={group.label}
            round
            on={options.groupBy === group.value}
            onClick={() => options.set("groupBy", group.value)}
          />
        ))}
      </section>

      <section>
        <SectionHeading size="sm" className="px-1 pb-2">
          Show
        </SectionHeading>
        <Row
          label="Critical path"
          title="The longest chain of blocked_by dependencies"
          on={options.critical}
          onClick={() => options.set("critical", !options.critical)}
        />
        <Row
          label="Done issues"
          on={options.showDone}
          onClick={() => options.set("showDone", !options.showDone)}
        />
        <Row
          label="Weekend shading"
          on={options.weekends}
          onClick={() => options.set("weekends", !options.weekends)}
        />
      </section>

      <p className="px-1 text-[11.5px] leading-relaxed text-muted">
        Bars read <span className="font-mono">start</span> and{" "}
        <span className="font-mono">due</span> from the issue file. A missing edge is drawn dashed
        and inferred from the estimate — shown, never stored. Dragging a bar commits the real
        fields.
      </p>
    </div>
  );
}
