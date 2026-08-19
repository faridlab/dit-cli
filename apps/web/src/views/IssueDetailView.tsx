// Issue detail: fields on the right rail, description and comments in the
// main column, field history at the bottom of the rail. Text inputs commit
// on Enter or blur (no save button per field — this is a keyboard tool);
// they are keyed by the server value, so a live refresh never destroys
// in-progress typing unless the server itself changed that field.

import { type FormEvent, type KeyboardEvent, useState } from "react";
import { ArrowLeft, Pencil, X } from "lucide-react";
import { BodyEditor } from "../components/BodyEditor";
import { Markdown } from "../components/Markdown";
import { SelectField } from "../components/SelectField";
import { AssigneeCircles, IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";
import { INPUT_CLASS, SectionHeading } from "../components/chrome";
import { ApiError } from "../lib/api";
import {
  circleColor,
  fullTimestamp,
  initials,
  parseCsvList,
  relativeTime,
} from "../lib/format";
import {
  useAddComment,
  useComments,
  useFieldEvents,
  useIssue,
  usePatchIssue,
  useSchema,
} from "../lib/queries";
import type { FieldEventDto, IssueDto, IssueType, Priority } from "../lib/types";
import { cn } from "../lib/cn";

const TYPE_OPTIONS: Array<{ value: IssueType; label: string }> = [
  { value: "task", label: "task" },
  { value: "bug", label: "bug" },
  { value: "story", label: "story" },
  { value: "spike", label: "spike" },
  { value: "chore", label: "chore" },
];

const PRIORITY_OPTIONS: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

function FieldRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10.5px] font-medium uppercase tracking-[0.05em] text-zinc-500">
        {label}
      </span>
      {children}
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
          className="group flex items-center gap-1.5 rounded-[3px] border border-ctl bg-card px-2 py-0.5 font-mono text-[11px] text-zinc-300 hover:border-red-400 hover:text-red-300 disabled:opacity-50"
        >
          {label}
          <span className="text-dim group-hover:text-red-300" aria-hidden>
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
          className="h-[24px] w-28 rounded-[3px] border border-accent bg-app px-1.5 font-mono text-[11px] text-zinc-200 focus:outline-none"
        />
      ) : (
        <button
          type="button"
          disabled={disabled}
          onClick={() => setAdding(true)}
          className="rounded-[3px] border border-dashed border-ctl px-2 py-0.5 font-mono text-[11px] text-dim hover:border-zinc-400 hover:text-zinc-400 disabled:opacity-50"
        >
          + label
        </button>
      )}
    </span>
  );
}

function CommentsSection({ issueId }: { issueId: string }) {
  const comments = useComments(issueId);
  const add = useAddComment(issueId);
  const [draft, setDraft] = useState("");

  const send = () => {
    const body = draft.trim();
    if (body.length === 0 || add.isPending) return;
    add.mutate(body, { onSuccess: () => setDraft("") });
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    send();
  };

  return (
    <section>
      <SectionHeading size="sm" className="mb-3">
        Comments
      </SectionHeading>
      {comments.isPending ? <Loading label="Loading comments…" /> : null}
      {comments.isError ? (
        <ErrorBox
          error={comments.error}
          title="Could not load comments"
          onRetry={() => void comments.refetch()}
        />
      ) : null}
      {comments.data && comments.data.length === 0 ? (
        <p className="text-xs text-zinc-600">No comments yet.</p>
      ) : null}
      <ul className="mb-3 flex flex-col gap-2.5">
        {(comments.data ?? []).map((comment) => (
          <li key={comment.id} className="flex gap-2.5">
            <span
              className={cn(
                "mt-0.5 inline-flex size-[22px] shrink-0 items-center justify-center rounded-full font-mono text-[9px] leading-none text-white",
                circleColor(comment.author),
              )}
              title={comment.author}
            >
              {initials(comment.author)}
            </span>
            <div className="min-w-0 flex-1 rounded-lg border border-edge bg-card/60 px-3 py-2.5">
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-[11.5px] text-zinc-300">{comment.author}</span>
                <span
                  className="text-[11.5px] text-dim"
                  title={fullTimestamp(comment.created)}
                >
                  {relativeTime(comment.created)}
                </span>
              </div>
              <Markdown html={comment.body_html} className="mt-1.5 text-sm" />
            </div>
          </li>
        ))}
      </ul>
      <form onSubmit={submit} className="flex flex-col gap-2">
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            // Ctrl/⌘+Enter sends — the mouse never has to leave the keyboard.
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
              event.preventDefault();
              send();
            }
          }}
          placeholder="Write a comment (markdown)…"
          rows={3}
          className={cn(INPUT_CLASS, "h-auto w-full resize-y bg-app py-2.5 font-mono text-[12.5px]")}
        />
        <div className="flex items-center gap-2.5">
          <button
            type="submit"
            disabled={draft.trim().length === 0 || add.isPending}
            className="rounded-md bg-accent px-2.5 py-1 text-[11px] font-medium text-white hover:bg-accent-hi disabled:bg-card disabled:text-zinc-500"
          >
            Comment
          </button>
          <span className="text-[11.5px] text-dim">Ctrl/⌘+Enter to send</span>
        </div>
      </form>
    </section>
  );
}

function HistorySection({ events }: { events: FieldEventDto[] }) {
  return (
    <section>
      <div className="mb-2.5 flex items-center justify-between gap-2">
        <SectionHeading size="sm">History</SectionHeading>
        <span className="text-[11px] text-dim">all fields · by seq</span>
      </div>
      {events.length === 0 ? (
        <p className="text-xs text-zinc-600">No recorded changes yet.</p>
      ) : null}
      <ol className="flex flex-col">
        {events.map((event) => (
          // The server orders by seq — the order things actually happened,
          // newest last. Re-sorting by timestamp would invent contradictions.
          <li key={event.seq} className="flex gap-2.5 border-l border-edge py-2 pl-3">
            <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-edge" aria-hidden />
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline gap-2 text-[11px] text-zinc-500">
                <span className="font-mono tabular-nums text-dim">#{event.seq}</span>
                <span className="font-mono text-zinc-400">{event.author}</span>
                <span className="ml-auto" title={fullTimestamp(event.ts)}>
                  {relativeTime(event.ts)}
                </span>
              </div>
              <p className="mt-0.5 text-[12.5px] text-zinc-300">{event.field}</p>
              <p className="mt-px font-mono text-[11px] text-zinc-500">
                {event.old_value ?? "∅"} → {event.new_value ?? "∅"}
              </p>
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}

function FieldRail({
  issue,
  history,
}: {
  issue: IssueDto;
  history: FieldEventDto[];
}) {
  const schema = useSchema();
  const patch = usePatchIssue(issue.id);

  const statuses = schema.data?.workflow.statuses ?? [];
  const statusOptions =
    statuses.length > 0
      ? statuses.map((status) => ({ value: status.id, label: status.label }))
      : [{ value: issue.status, label: issue.status }];

  // "Who touched it last", per field, derived from the one unfiltered
  // history query — never stored (invariant 5).
  const blameFor = (field: string): string | null => {
    for (let i = history.length - 1; i >= 0; i--) {
      const event = history[i];
      if (event && event.field === field) {
        return `${event.author}, ${relativeTime(event.ts)}`;
      }
    }
    return null;
  };
  const statusBlame = blameFor("status");

  return (
    <aside className="flex w-[288px] shrink-0 flex-col gap-5 overflow-y-auto border-l border-edge px-4 pb-7 pt-5 min-[1180px]:w-[336px]">
      <section>
        <div className="mb-2.5 flex items-center justify-between gap-2">
          <SectionHeading size="sm">Fields</SectionHeading>
          <span className="text-[11px] text-dim">who touched it last</span>
        </div>
        <div className="flex flex-col gap-3">
          <FieldRow label="Status">
            <SelectField
              ariaLabel="Status"
              value={issue.status}
              options={statusOptions}
              disabled={patch.isPending}
              onChange={(status) => patch.mutate({ status })}
            />
            {statusBlame ? <span className="text-[11px] text-dim">{statusBlame}</span> : null}
          </FieldRow>
          <FieldRow label="Type">
            <SelectField
              ariaLabel="Type"
              value={issue.type}
              options={TYPE_OPTIONS}
              disabled={patch.isPending}
              onChange={(type) => patch.mutate({ type })}
            />
          </FieldRow>
          <FieldRow label="Priority">
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
          <FieldRow label="Assignees">
            <CommitInput
              initial={issue.assignees.join(", ")}
              format="csv"
              placeholder="alias, alias"
              onCommit={(assignees) => patch.mutate({ assignees: parseCsvList(assignees) })}
            />
          </FieldRow>
          <FieldRow label="Labels">
            <LabelEditor
              labels={issue.labels}
              disabled={patch.isPending}
              onCommit={(labels) => patch.mutate({ labels })}
            />
          </FieldRow>
          <FieldRow label="Estimate">
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
          <FieldRow label="Due">
            <CommitInput
              initial={issue.due ?? ""}
              type="date"
              onCommit={(value) => {
                const trimmed = value.trim();
                if (trimmed.length > 0) patch.mutate({ due: trimmed });
              }}
            />
          </FieldRow>
          <FieldRow label="Sprint">
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
      </section>

      <HistorySection events={history} />

      {/* Epic is read-only here: v0.1's patch contract has no epic field, so
          linking an issue into an epic is an edit to the issue file itself. */}
      <p className="mt-auto border-t border-edge pt-3 font-mono text-[11px] leading-relaxed text-dim">
        {issue.epic ? (
          <>
            epic {issue.epic.slice(0, 8)}
            <br />
          </>
        ) : null}
        reported by {issue.reporter ?? "—"}
        <br />
        created {relativeTime(issue.created)} · updated {relativeTime(issue.updated)}
      </p>
    </aside>
  );
}

function DescriptionSection({ issue }: { issue: IssueDto }) {
  const [editing, setEditing] = useState(false);

  return (
    <section>
      <div className="mb-3 flex items-center justify-between gap-2">
        <SectionHeading size="sm">Description</SectionHeading>
        <button
          type="button"
          onClick={() => setEditing((value) => !value)}
          className={cn(
            "flex items-center gap-1 rounded-md border border-ctl px-2 py-0.5 text-[11.5px] transition-colors hover:border-zinc-400",
            editing ? "text-zinc-200" : "text-zinc-400",
          )}
        >
          {editing ? <X className="size-3" aria-hidden /> : <Pencil className="size-3" aria-hidden />}
          {editing ? "Done" : "Edit"}
        </button>
      </div>
      {editing ? (
        <BodyEditor issueId={issue.id} body={issue.body} />
      ) : issue.body.trim().length === 0 ? (
        <p className="rounded-lg border border-dashed border-edge px-3 py-6 text-center text-xs text-zinc-600">
          No description.
        </p>
      ) : (
        <Markdown html={issue.body_html} className="text-[14.5px] leading-relaxed" />
      )}
    </section>
  );
}

export function IssueDetailView({ id }: { id: string }) {
  const issue = useIssue(id);
  // Header and rail both edit this issue; one mutation hook before any early
  // return keeps hook order stable and the saving indicator shared.
  const patch = usePatchIssue(id);
  // One unfiltered history query feeds both the blame lines and the
  // timeline — the field param exists on the wire but costs a second request.
  const history = useFieldEvents(id);

  if (issue.isPending) {
    return <Loading label={`Loading ${id}…`} className="flex-1" />;
  }
  if (issue.isError) {
    const notFound = issue.error instanceof ApiError && issue.error.status === 404;
    return (
      <ErrorBox
        error={issue.error}
        title={notFound ? `No issue “${id}” in this workspace` : "Could not load the issue"}
        onRetry={() => void issue.refetch()}
      />
    );
  }

  const data = issue.data;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <a
          href="#/issues"
          title="Back to issues"
          className="rounded-md p-1 text-zinc-500 hover:bg-card hover:text-zinc-300"
        >
          <ArrowLeft className="size-4" aria-hidden />
        </a>
        <IssueHandle shortRef={data.short_ref} number={data.number} />
        <span
          className="font-mono text-[10px] text-dim"
          title="the permanent short ref behind the number"
        >
          {data.short_ref}
        </span>
        <TypeBadge type={data.type} />
        <PriorityDot priority={data.priority} />
        <input
          key={data.title}
          defaultValue={data.title}
          aria-label="Title"
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.currentTarget.blur();
            }
          }}
          onBlur={(event) => {
            const title = event.currentTarget.value.trim();
            if (title.length > 0 && title !== data.title) patch.mutate({ title });
          }}
          className="min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1.5 py-0.5 text-base font-semibold text-zinc-100 hover:border-ctl focus:border-accent focus:outline-none"
        />
        {patch.isPending ? <span className="text-[11.5px] text-dim">Saving…</span> : null}
        <AssigneeCircles assignees={data.assignees} />
      </header>

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col gap-[22px] overflow-y-auto px-6 pb-8 pt-5">
          <DescriptionSection issue={data} />
          <CommentsSection issueId={data.id} />
        </div>
        <FieldRail issue={data} history={history.data ?? []} />
      </div>
    </div>
  );
}
