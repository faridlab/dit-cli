// The pieces an issue surface is made of, in one place because there are
// two surfaces: the side panel that opens over a list, and the full page.
// Both render the same fields, the same always-on description editor and the
// same activity stream — only the layout around them differs.
//
// Text inputs commit on Enter or blur (no save button per field — this is a
// keyboard tool); they are keyed by the server value, so a live refresh
// never destroys in-progress typing unless the server itself changed that
// field.

import { type KeyboardEvent, lazy, Suspense, useMemo, useState } from "react";
import { Star } from "lucide-react";
import { BodyEditor } from "../BodyEditor";
import { Markdown } from "../Markdown";
import { SelectField } from "../SelectField";
import { PriorityDot } from "../badges";
import { ErrorBox, Loading } from "../states";
import { INPUT_CLASS, SectionHeading } from "../chrome";
import {
  circleColor,
  fullTimestamp,
  initials,
  parseCsvList,
  relativeTime,
} from "../../lib/format";
import { useAddComment, useComments, usePatchIssue, useSchema } from "../../lib/queries";
import { mergeActivity } from "../../lib/activity";
import { toggleStar, useIsStarred } from "../../lib/starred";
import type { FieldEventDto, IssueDto, IssueType, Priority } from "../../lib/types";
import { cn } from "../../lib/cn";

/** `rail` stacks label over control in a narrow column (the page's right
 *  rail); `panel` puts them side by side, which reads better in the wider
 *  side panel and keeps the description closer to the top. */
export type FieldLayout = "rail" | "panel";

const TYPE_OPTIONS: Array<{ value: IssueType; label: string }> = [
  { value: "task", label: "task" },
  { value: "bug", label: "bug" },
  { value: "story", label: "story" },
  { value: "spike", label: "spike" },
  { value: "chore", label: "chore" },
];

const PRIORITY_OPTIONS: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

// The comment composer is the same Notion-like editor the description uses
// (lazy chunk, TipTap + the Rust bridge in WASM).
const RichEditor = lazy(() => import("../../editor/RichEditor"));

function FieldRow({
  label,
  layout,
  blame,
  children,
}: {
  label: string;
  layout: FieldLayout;
  /** "Who touched it last", shown beside the value. */
  blame?: string | null;
  children: React.ReactNode;
}) {
  if (layout === "panel") {
    return (
      <>
        <span className="flex h-[30px] items-center text-[12px] text-muted">{label}</span>
        <span className="flex min-w-0 items-center gap-2">
          {/* Capped so every row's control ends on the same line and the
              blame column stays where the eye expects it. */}
          <span className="min-w-0 max-w-[300px] flex-1">{children}</span>
          {blame ? (
            <span className="ml-auto hidden shrink-0 font-mono text-[10.5px] text-faint min-[560px]:block">
              {blame}
            </span>
          ) : null}
        </span>
      </>
    );
  }
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10.5px] font-medium uppercase tracking-[0.05em] text-muted">
        {label}
      </span>
      {children}
      {blame ? <span className="text-[11px] text-dim">{blame}</span> : null}
    </div>
  );
}

/** Text input that starts from `initial` and commits on Enter/blur. Keyed
 *  remount on server-side change keeps it honest without controlled state. */
function CommitInput({
  initial,
  onCommit,
  placeholder,
  type = "text",
  format,
}: {
  initial: string;
  onCommit: (value: string) => void;
  placeholder?: string;
  type?: "text" | "number" | "date";
  format?: "csv";
}) {
  const commit = (event: React.FocusEvent<HTMLInputElement> | KeyboardEvent<HTMLInputElement>) => {
    const raw = event.currentTarget.value;
    if (format === "csv") {
      const next = parseCsvList(raw);
      const current = parseCsvList(initial);
      if (next.join(",") !== current.join(",")) onCommit(next.join(","));
    } else if (raw !== initial) {
      onCommit(raw);
    }
  };
  return (
    <input
      key={initial}
      type={type}
      defaultValue={initial}
      placeholder={placeholder}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit(event);
      }}
      className={cn(INPUT_CLASS, "w-full")}
    />
  );
}

/** Labels as chips: click × to drop one, "+ label" to add. Every commit
 *  sends the whole array — the patch replaces the set. */
function LabelEditor({
  labels,
  disabled,
  onCommit,
}: {
  labels: string[];
  disabled?: boolean;
  onCommit: (labels: string[]) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");

  const commit = () => {
    const added = parseCsvList(draft);
    setAdding(false);
    setDraft("");
    if (added.length === 0) return;
    onCommit([...labels, ...added.filter((label) => !labels.includes(label))]);
  };

  return (
    <span className="flex flex-wrap gap-1.5">
      {labels.map((label) => (
        <button
          key={label}
          type="button"
          disabled={disabled}
          onClick={() => onCommit(labels.filter((each) => each !== label))}
          title={`Remove ${label}`}
          className="group flex items-center gap-1.5 rounded-[3px] border border-ctl bg-card px-2 py-0.5 font-mono text-[11px] text-ink-2 hover:border-crit-text hover:text-crit-text disabled:opacity-50"
        >
          {label}
          <span className="text-dim group-hover:text-crit-text" aria-hidden>
            ×
          </span>
        </button>
      ))}
      {adding ? (
        <input
          autoFocus
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") commit();
            if (event.key === "Escape") {
              setAdding(false);
              setDraft("");
            }
          }}
          placeholder="label, label"
          aria-label="Add labels"
          className="h-[24px] w-28 rounded-[3px] border border-accent bg-app px-1.5 font-mono text-[11px] text-ink focus:outline-none"
        />
      ) : (
        <button
          type="button"
          disabled={disabled}
          onClick={() => setAdding(true)}
          className="rounded-[3px] border border-dashed border-ctl px-2 py-0.5 font-mono text-[11px] text-dim hover:border-dim hover:text-ink-2 disabled:opacity-50"
        >
          + label
        </button>
      )}
    </span>
  );
}

/** Every editable frontmatter field, with the per-field blame line derived
 *  from `field_events` — computed on read, never stored (invariant 5). */
export function IssueFields({
  issue,
  history,
  layout,
}: {
  issue: IssueDto;
  history: FieldEventDto[];
  layout: FieldLayout;
}) {
  const schema = useSchema();
  const patch = usePatchIssue(issue.id);

  const statuses = schema.data?.workflow.statuses ?? [];
  const statusOptions =
    statuses.length > 0
      ? statuses.map((status) => ({ value: status.id, label: status.label }))
      : [{ value: issue.status, label: issue.status }];

  // The events arrive ordered by seq — the order things actually happened —
  // so the last one mentioning a field is that field's latest change.
  const blameFor = (field: string): string | null => {
    for (let i = history.length - 1; i >= 0; i--) {
      const event = history[i];
      if (event && event.field === field) {
        return `${event.author}, ${relativeTime(event.ts)}`;
      }
    }
    return null;
  };

  return (
    <div
      className={cn(
        layout === "panel"
          ? "grid grid-cols-[92px_minmax(0,1fr)] items-center gap-x-3 gap-y-1.5"
          : "flex flex-col gap-3",
      )}
    >
      <FieldRow label="Status" layout={layout} blame={blameFor("status")}>
        <SelectField
          ariaLabel="Status"
          value={issue.status}
          options={statusOptions}
          disabled={patch.isPending}
          onChange={(status) => patch.mutate({ status })}
        />
      </FieldRow>
      <FieldRow label="Type" layout={layout} blame={blameFor("type")}>
        <SelectField
          ariaLabel="Type"
          value={issue.type}
          options={TYPE_OPTIONS}
          disabled={patch.isPending}
          onChange={(type) => patch.mutate({ type })}
        />
      </FieldRow>
      <FieldRow label="Priority" layout={layout} blame={blameFor("priority")}>
        {/* No "clear" affordance: v0.1's patch contract treats null as
            "absent", so a field can be changed but not emptied. */}
        <SelectField
          ariaLabel="Priority"
          value={issue.priority ?? ""}
          options={PRIORITY_OPTIONS.map((value) => ({ value, label: value }))}
          disabled={patch.isPending}
          onChange={(priority) => patch.mutate({ priority })}
        />
      </FieldRow>
      <FieldRow label="Assignees" layout={layout} blame={blameFor("assignees")}>
        <CommitInput
          initial={issue.assignees.join(", ")}
          format="csv"
          placeholder="alias, alias"
          onCommit={(assignees) => patch.mutate({ assignees: parseCsvList(assignees) })}
        />
      </FieldRow>
      <FieldRow label="Labels" layout={layout} blame={blameFor("labels")}>
        <LabelEditor
          labels={issue.labels}
          disabled={patch.isPending}
          onCommit={(labels) => patch.mutate({ labels })}
        />
      </FieldRow>
      <FieldRow label="Estimate" layout={layout} blame={blameFor("estimate")}>
        <CommitInput
          initial={issue.estimate === null ? "" : String(issue.estimate)}
          type="number"
          placeholder="—"
          onCommit={(value) => {
            const trimmed = value.trim();
            if (trimmed.length > 0) patch.mutate({ estimate: Number(trimmed) });
          }}
        />
      </FieldRow>
      <FieldRow label="Due" layout={layout} blame={blameFor("due")}>
        <CommitInput
          initial={issue.due ?? ""}
          type="date"
          onCommit={(value) => {
            const trimmed = value.trim();
            if (trimmed.length > 0) patch.mutate({ due: trimmed });
          }}
        />
      </FieldRow>
      <FieldRow label="Sprint" layout={layout} blame={blameFor("sprint")}>
        <CommitInput
          initial={issue.sprint ?? ""}
          placeholder="—"
          onCommit={(value) => {
            const trimmed = value.trim();
            if (trimmed.length > 0) patch.mutate({ sprint: trimmed });
          }}
        />
      </FieldRow>
    </div>
  );
}

/** The description is the editor: always on, autosaving per pause — the
 *  same surface for reading and writing, like the doc editor. Keyed by the
 *  issue so switching issues never carries a buffer across. */
export function DescriptionSection({ issue }: { issue: IssueDto }) {
  return (
    <section>
      <SectionHeading size="sm" className="mb-3">
        Description
      </SectionHeading>
      <BodyEditor key={issue.id} issueId={issue.id} body={issue.body} />
    </section>
  );
}

/** One line of a commit entry: what a single field became. Creation reads
 *  as "set to X" — there is no "from" when the issue did not exist yet. */
function FieldChange({ event, statusLabel }: { event: FieldEventDto; statusLabel: (id: string) => string }) {
  const render = (value: string | null) => {
    if (value === null || value.length === 0) {
      return <span className="text-faint">—</span>;
    }
    if (event.field === "status") {
      return <span className="text-ink-2">{statusLabel(value)}</span>;
    }
    if (event.field === "priority") {
      return (
        <span className="inline-flex items-center gap-1.5">
          <PriorityDot priority={value as Priority} />
          <span className="font-mono text-ink-2">{value}</span>
        </span>
      );
    }
    return <span className="font-mono text-ink-2">{value}</span>;
  };

  return (
    <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-[12px]">
      <span className="text-muted">{event.field}</span>
      {event.old_value === null ? (
        render(event.new_value)
      ) : (
        <>
          {render(event.old_value)}
          <span className="text-faint" aria-label="changed to">
            →
          </span>
          {render(event.new_value)}
        </>
      )}
    </div>
  );
}

/** "status, priority and due" — the fields one commit touched. */
function fieldList(fields: string[]): string {
  if (fields.length <= 1) return fields[0] ?? "";
  return `${fields.slice(0, -1).join(", ")} and ${fields[fields.length - 1]}`;
}

type ActivityFilter = "all" | "comments" | "changes";

const FILTERS: Array<{ value: ActivityFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "comments", label: "Comments" },
  { value: "changes", label: "Changes" },
];

/** Comments and field changes as one stream — what happened to this issue,
 *  in the order it happened. Both come out of git; neither is stored as an
 *  activity log (invariant 5). */
export function IssueActivity({
  issueId,
  history,
}: {
  issueId: string;
  history: FieldEventDto[];
}) {
  const comments = useComments(issueId);
  const add = useAddComment(issueId);
  const schema = useSchema();
  const [draft, setDraft] = useState("");
  const [filter, setFilter] = useState<ActivityFilter>("all");

  const statusLabel = (id: string) =>
    schema.data?.workflow.statuses.find((status) => status.id === id)?.label ?? id;

  const entries = useMemo(
    () => mergeActivity(history, comments.data ?? []),
    [history, comments.data],
  );
  const shown = entries.filter((entry) =>
    filter === "all" ? true : filter === "comments" ? entry.kind === "comment" : entry.kind === "commit",
  );

  // A comment is a discrete message in git history, so the composer sends
  // when the writer says so rather than on a typing pause: Mod+Enter (the
  // editor hands over its just-serialized bytes) or the button.
  const send = (markdown?: string) => {
    const body = (markdown ?? draft).trim();
    if (body.length === 0 || add.isPending) return;
    add.mutate(body, { onSuccess: () => setDraft("") });
  };

  return (
    <section>
      <div className="mb-3 flex items-center gap-2">
        <SectionHeading size="sm">Activity</SectionHeading>
        <div className="ml-auto flex overflow-hidden rounded-md border border-edge" role="group" aria-label="Filter activity">
          {FILTERS.map((option, index) => (
            <button
              key={option.value}
              type="button"
              aria-pressed={filter === option.value}
              onClick={() => setFilter(option.value)}
              className={cn(
                "px-2 py-0.5 text-[11.5px] transition-colors",
                index > 0 && "border-l border-edge",
                filter === option.value ? "bg-sunken text-ink" : "text-muted hover:text-ink",
              )}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>

      {comments.isPending ? <Loading label="Loading activity…" /> : null}
      {comments.isError ? (
        <ErrorBox
          error={comments.error}
          title="Could not load comments"
          onRetry={() => void comments.refetch()}
        />
      ) : null}

      <ol className="mb-4 flex flex-col gap-3">
        {shown.map((entry) =>
          entry.kind === "comment" ? (
            <li key={entry.id} className="flex gap-2.5">
              <span
                className={cn(
                  "mt-0.5 inline-flex size-[22px] shrink-0 items-center justify-center rounded-full font-mono text-[9px] leading-none text-white",
                  circleColor(entry.author),
                )}
                title={entry.author}
              >
                {initials(entry.author)}
              </span>
              <div className="min-w-0 flex-1 rounded-lg border border-edge bg-card/60 px-3 py-2.5">
                <div className="flex items-baseline gap-2">
                  <span className="font-mono text-[11.5px] text-ink-2">{entry.author}</span>
                  <span className="text-[11.5px] text-dim" title={fullTimestamp(entry.ts)}>
                    {relativeTime(entry.ts)}
                  </span>
                </div>
                <Markdown html={entry.bodyHtml} className="mt-1.5 text-sm" />
              </div>
            </li>
          ) : (
            <li key={`${entry.sha}-${entry.seq}`} className="flex gap-2.5">
              <span
                className="mt-[7px] size-[7px] shrink-0 rounded-full bg-ctl"
                aria-hidden
              />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-baseline gap-x-2 text-[12px] text-muted">
                  <span className="font-mono text-ink-2">{entry.author}</span>
                  <span>
                    {entry.creation
                      ? "created this issue"
                      : `changed ${fieldList(entry.events.map((event) => event.field))}`}
                  </span>
                  <span
                    className="ml-auto font-mono text-[11px] text-faint"
                    title={`${fullTimestamp(entry.ts)}\ncommit ${entry.sha}`}
                  >
                    {relativeTime(entry.ts)}
                  </span>
                </div>
                <div className="mt-1 flex flex-col gap-0.5 border-l border-edge pl-2.5">
                  {entry.events.map((event) => (
                    <FieldChange key={event.seq} event={event} statusLabel={statusLabel} />
                  ))}
                </div>
              </div>
            </li>
          ),
        )}
      </ol>
      {shown.length === 0 && !comments.isPending ? (
        <p className="mb-4 text-xs text-faint">
          {filter === "comments"
            ? "No comments yet."
            : filter === "changes"
              ? "No recorded changes yet."
              : "Nothing has happened to this issue yet."}
        </p>
      ) : null}

      <div className="flex flex-col gap-2">
        <Suspense fallback={<Loading label="Loading editor…" />}>
          <RichEditor
            value={draft}
            onChange={setDraft}
            onSave={send}
            className="min-h-20 rounded-md border border-edge bg-card/60 p-2"
          />
        </Suspense>
        <div className="flex items-center gap-2.5">
          <button
            type="button"
            onClick={() => send()}
            disabled={draft.trim().length === 0 || add.isPending}
            className="rounded-md bg-accent px-2.5 py-1 text-[11px] font-medium text-on-accent hover:bg-accent-hi disabled:bg-card disabled:text-muted"
          >
            Comment
          </button>
          <span className="text-[11.5px] text-dim">Ctrl/⌘+Enter to send</span>
        </div>
      </div>
    </section>
  );
}

/** Star toggle. A star is a private bookmark — this browser only, never a
 *  file, never a commit — so the button says so on hover. */
export function StarButton({ shortRef, className }: { shortRef: string; className?: string }) {
  const starred = useIsStarred(shortRef);
  return (
    <button
      type="button"
      aria-pressed={starred}
      aria-label={starred ? "Unstar" : "Star"}
      title={starred ? "Starred — kept in this browser" : "Star (kept in this browser, never in the repo)"}
      onClick={() => toggleStar(shortRef)}
      className={cn(
        "flex size-7 shrink-0 items-center justify-center rounded-md transition-colors hover:bg-hover",
        starred ? "text-warn-text" : "text-muted hover:text-ink",
        className,
      )}
    >
      <Star className={cn("size-4", starred && "fill-current")} aria-hidden />
    </button>
  );
}

/** The title, edited in place. Commits on Enter or blur; an empty title is
 *  refused rather than committed, since the title is how an issue is read
 *  everywhere else. */
export function IssueTitle({
  issue,
  size,
}: {
  issue: IssueDto;
  size: "panel" | "page";
}) {
  const patch = usePatchIssue(issue.id);
  return (
    <input
      key={issue.title}
      defaultValue={issue.title}
      aria-label="Title"
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
        // The panel's Escape closes it; inside the title it should only
        // abandon the edit.
        if (event.key === "Escape") {
          event.stopPropagation();
          event.currentTarget.value = issue.title;
          event.currentTarget.blur();
        }
      }}
      onBlur={(event) => {
        const title = event.currentTarget.value.trim();
        if (title.length > 0 && title !== issue.title) patch.mutate({ title });
        else event.currentTarget.value = issue.title;
      }}
      className={cn(
        "w-full min-w-0 rounded-md border border-transparent bg-transparent px-1.5 py-0.5 font-semibold text-ink hover:border-ctl focus:border-accent focus:outline-none",
        size === "page" ? "text-base" : "text-[17px] leading-snug",
      )}
    />
  );
}
