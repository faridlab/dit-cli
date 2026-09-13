// The recurring atoms of issue rows: handle, type letter, priority dot,
// assignee monograms, label chips, status pill, due copy. Every view that
// shows an issue shows these, so they live in one place — and they emit the
// exact class recipes the approved design is written in (see styles.css,
// "Workbench recipes"), which is what keeps every screen on the same grid.

import { avatarColor, dueInfo, initials, typeLetter } from "../lib/format";
import type { IssueType, Priority, StatusCategory, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";

/** The handle a human reads (ADR 0007): `#12` once the workspace numbered
 *  the issue, the short ref until then. The short ref stays the permanent
 *  identifier everywhere else (URLs, navigation, API calls). */
export function IssueHandle({
  shortRef,
  number,
  className,
}: {
  shortRef: string;
  number: number | null;
  className?: string;
}) {
  return <span className={cn("handle", className)}>{number !== null ? `#${number}` : shortRef}</span>;
}

export function TypeBadge({ type, className }: { type: IssueType; className?: string }) {
  return (
    <span aria-label={`type: ${type}`} title={type} className={cn("tb", type, className)}>
      {typeLetter(type) ?? "?"}
    </span>
  );
}

export function PriorityDot({
  priority,
  title,
  className,
}: {
  priority: Priority | null;
  title?: string;
  className?: string;
}) {
  return (
    <span
      title={title ?? priority ?? "no priority"}
      aria-label={`priority: ${priority ?? "none"}`}
      className={cn("pd", priority ?? "none", className)}
    />
  );
}

/** One monogram circle. `name` is the alias as git knows it. */
export function Avatar({ name, title }: { name: string; title?: string }) {
  return (
    <span className="av" style={{ background: avatarColor(name) }} title={title ?? name}>
      {initials(name)}
    </span>
  );
}

/** Overlapping monograms; a dashed hollow circle when nobody owns it. */
export function AssigneeCircles({
  assignees,
  max = 3,
  hollow = true,
}: {
  assignees: string[];
  max?: number;
  /** Draw the "unassigned" placeholder when the list is empty. */
  hollow?: boolean;
}) {
  if (assignees.length === 0) {
    return hollow ? (
      <span className="av none" title="unassigned">
        –
      </span>
    ) : null;
  }
  const shown = assignees.slice(0, max);
  const overflow = assignees.length - shown.length;
  return (
    <span className="avs" title={assignees.join(", ")}>
      {shown.map((name) => (
        <Avatar key={name} name={name} />
      ))}
      {overflow > 0 ? (
        <span className="av" style={{ background: "var(--edge-2)", color: "var(--ink-2)" }}>
          +{overflow}
        </span>
      ) : null}
    </span>
  );
}

/** A `context:x` label reads as `@x` in accent; every other label is a
 *  quiet mono chip. */
export function Chip({ label, className }: { label: string; className?: string }) {
  const ctx = label.startsWith("context:");
  return (
    <span className={cn("chip", ctx && "ctx", className)} title={label}>
      {ctx ? `@${label.slice("context:".length)}` : label}
    </span>
  );
}

export function LabelChips({
  labels,
  max = 3,
  className,
}: {
  labels: string[];
  max?: number;
  className?: string;
}) {
  if (labels.length === 0) return null;
  const shown = labels.slice(0, max);
  const overflow = labels.length - shown.length;
  return (
    <span className={cn("labels flex min-w-0 gap-1", className)} title={labels.join(", ")}>
      {shown.map((label) => (
        <Chip key={label} label={label} />
      ))}
      {overflow > 0 ? <span className="chip">+{overflow}</span> : null}
    </span>
  );
}

/** Status as a pill with a leading dot in the category's color. Accepts
 *  either the workflow entry or a bare category + label. */
export function StatusPill({
  status,
  category,
  label,
  className,
}: {
  status?: StatusDto;
  category?: StatusCategory;
  label?: string;
  className?: string;
}) {
  const cat = status?.category ?? category ?? "todo";
  const text = status?.label ?? label ?? "";
  return <span className={cn("pill", cat, className)}>{text}</span>;
}

/** Due copy in the design's three tones. Renders nothing without a date
 *  unless `placeholder` is given. */
export function Due({
  iso,
  placeholder,
  className,
}: {
  iso: string | null;
  placeholder?: string;
  className?: string;
}) {
  const info = dueInfo(iso);
  if (!info) return placeholder ? <span className={cn("due", className)}>{placeholder}</span> : null;
  return (
    <span className={cn("due", info.cls, className)} title={iso ?? undefined}>
      {info.text}
    </span>
  );
}
