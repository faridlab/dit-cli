// The recurring atoms of issue rows: priority dot, type letter, assignee
// monogram circles, label chips, status pill. Every view that shows an issue
// shows these, so they live in one place.

import { circleColor, initials, priorityDot, typeBadgeClass, typeLetter } from "../lib/format";
import type { IssueType, Priority, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";

export function PriorityDot({
  priority,
  title,
}: {
  priority: Priority | null;
  title?: string;
}) {
  return (
    <span
      title={title ?? priority ?? "no priority"}
      aria-label={`priority: ${priority ?? "none"}`}
      className={cn("inline-block size-2 shrink-0 rounded-full", priorityDot(priority))}
    />
  );
}

export function TypeBadge({ type }: { type: IssueType }) {
  const letter = typeLetter(type);
  if (letter === null) return null;
  return (
    <span
      aria-label={`type: ${type}`}
      title={type}
      className={cn(
        "inline-flex size-4 shrink-0 items-center justify-center rounded-[2px] font-mono text-[10px] leading-none",
        typeBadgeClass(type),
      )}
    >
      {letter}
    </span>
  );
}

export function AssigneeCircles({ assignees }: { assignees: string[] }) {
  if (assignees.length === 0) return null;
  const shown = assignees.slice(0, 3);
  const overflow = assignees.length - shown.length;
  return (
    <span className="flex shrink-0 -space-x-1" title={assignees.join(", ")}>
      {shown.map((name) => (
        <span
          key={name}
          className={cn(
            "inline-flex size-[22px] items-center justify-center rounded-full border border-app font-mono text-[9px] leading-none text-white",
            circleColor(name),
          )}
        >
          {initials(name)}
        </span>
      ))}
      {overflow > 0 ? (
        <span className="inline-flex size-[22px] items-center justify-center rounded-full border border-ctl bg-card font-mono text-[9px] leading-none text-zinc-400">
          +{overflow}
        </span>
      ) : null}
    </span>
  );
}

const CHIP_CLASS =
  "rounded-[3px] border border-white/[0.06] bg-white/[0.04] px-1.5 font-mono text-[10px] leading-4 text-zinc-400";

export function LabelChips({ labels, max = 3 }: { labels: string[]; max?: number }) {
  if (labels.length === 0) return null;
  const shown = labels.slice(0, max);
  const overflow = labels.length - shown.length;
  return (
    <span className="flex min-w-0 flex-wrap gap-1" title={labels.join(", ")}>
      {shown.map((label) => (
        <span key={label} className={CHIP_CLASS}>
          {label}
        </span>
      ))}
      {overflow > 0 ? <span className={CHIP_CLASS}>+{overflow}</span> : null}
    </span>
  );
}

export function StatusPill({ status }: { status: StatusDto }) {
  const tone =
    status.category === "done"
      ? "text-emerald-400 bg-emerald-950/50 border-emerald-900/60"
      : status.category === "doing"
        ? "text-sky-400 bg-sky-950/50 border-sky-900/60"
        : "text-zinc-400 bg-zinc-800/60 border-zinc-700";
  return (
    <span
      className={cn(
        "inline-flex w-fit items-center justify-center truncate rounded-[3px] border px-1.5 text-[11px] leading-4",
        tone,
      )}
    >
      {status.label}
    </span>
  );
}

/** The handle a human reads (ADR 0007): `#12` once the workspace numbered
 *  the issue, the short ref until then. The short ref stays the permanent
 *  identifier everywhere else (URLs, navigation, API calls). */
export function IssueHandle({
  shortRef,
  number,
}: {
  shortRef: string;
  number: number | null;
}) {
  return (
    <span className="font-mono text-xs tabular-nums text-zinc-500">
      {number !== null ? `#${number}` : shortRef}
    </span>
  );
}
