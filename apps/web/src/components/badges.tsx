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
        "inline-flex size-4 shrink-0 items-center justify-center rounded-sm border font-mono text-[10px] leading-none",
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
    <span className="flex -space-x-1" title={assignees.join(", ")}>
      {shown.map((name) => (
        <span
          key={name}
          className={cn(
            "inline-flex size-5 items-center justify-center rounded-full border border-zinc-900 font-mono text-[9px] text-white",
            circleColor(name),
          )}
        >
          {initials(name)}
        </span>
      ))}
      {overflow > 0 ? (
        <span className="inline-flex size-5 items-center justify-center rounded-full border border-zinc-700 bg-zinc-800 font-mono text-[9px] text-zinc-400">
          +{overflow}
        </span>
      ) : null}
    </span>
  );
}

export function LabelChips({ labels }: { labels: string[] }) {
  if (labels.length === 0) return null;
  return (
    <span className="flex flex-wrap gap-1">
      {labels.map((label) => (
        <span
          key={label}
          className="rounded-sm bg-zinc-800 px-1.5 py-px font-mono text-[10px] text-zinc-400"
        >
          {label}
        </span>
      ))}
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
      className={cn("inline-flex items-center rounded border px-1.5 py-px text-[11px]", tone)}
    >
      {status.label}
    </span>
  );
}

export function ShortRef({ shortRef }: { shortRef: string }) {
  return (
    <span className="font-mono text-xs tabular-nums text-zinc-500">{shortRef}</span>
  );
}
