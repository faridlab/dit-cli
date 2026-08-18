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
import { ApiError } from "../lib/api";
import { fullTimestamp, parseCsvList, relativeTime } from "../lib/format";
import {
  useAddComment,
  useComments,
  useFieldEvents,
  useIssue,
  usePatchIssue,
  useSchema,
} from "../lib/queries";
import type { IssueDto, IssueType, Priority } from "../lib/types";
import { cn } from "../lib/cn";

const INPUT_CLASS =
  "h-7 w-full rounded border border-zinc-700 bg-zinc-950 px-2 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none";

const TYPE_OPTIONS: Array<{ value: IssueType; label: string }> = [
  { value: "task", label: "task" },
  { value: "bug", label: "bug" },
  { value: "story", label: "story" },
  { value: "spike", label: "spike" },
  { value: "chore", label: "chore" },
];

const PRIORITY_OPTIONS: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

const HISTORY_FIELDS = [
  "status",
  "title",
  "priority",
  "type",
  "assignees",
  "labels",
  "estimate",
  "due",
  "sprint",
  "epic",
] as const;

function FieldRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[10px] font-medium uppercase tracking-wide text-zinc-500">
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
      className={INPUT_CLASS}
    />
  );
}

function CommentsSection({ issueId }: { issueId: string }) {
  const comments = useComments(issueId);
  const add = useAddComment(issueId);
  const [draft, setDraft] = useState("");

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const body = draft.trim();
    if (body.length === 0 || add.isPending) return;
    add.mutate(body, { onSuccess: () => setDraft("") });
  };

  return (
    <section className="mt-2 border-t border-zinc-800 pt-3">
      <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-zinc-400">
        Comments
      </h2>
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
      <ul className="flex flex-col gap-3">
        {(comments.data ?? []).map((comment) => (
          <li key={comment.id} className="flex gap-2">
            <span
              className="mt-0.5 inline-flex size-5 shrink-0 items-center justify-center rounded-full bg-zinc-700 font-mono text-[9px] text-zinc-200"
              title={comment.author}
            >
              {comment.author.slice(0, 2).toUpperCase()}
            </span>
            <div className="min-w-0 flex-1 rounded-md border border-zinc-800 bg-zinc-950/60 px-3 py-2">
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-[11px] text-zinc-300">{comment.author}</span>
                <span
                  className="text-[11px] text-zinc-600"
                  title={fullTimestamp(comment.created)}
                >
                  {relativeTime(comment.created)}
                </span>
              </div>
              <Markdown html={comment.body_html} className="mt-1 text-[13px]" />
            </div>
          </li>
        ))}
      </ul>
      <form onSubmit={submit} className="mt-3 flex flex-col gap-2">
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          placeholder="Write a comment (markdown)…"
          rows={3}
          className={cn(INPUT_CLASS, "h-auto resize-y py-1.5 font-mono")}
        />
        <div className="flex items-center gap-2">
          <button
            type="submit"
            disabled={draft.trim().length === 0 || add.isPending}
            className="rounded bg-sky-700 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-sky-600 disabled:bg-zinc-800 disabled:text-zinc-500"
          >
            Comment
          </button>
          <span className="text-[11px] text-zinc-600">Ctrl/⌘+Enter to send</span>
        </div>
      </form>
    </section>
  );
}

function HistorySection({ issueId }: { issueId: string }) {
  const [field, setField] = useState<string>("status");
  const events = useFieldEvents(issueId, field);

  return (
    <section className="mt-4 border-t border-zinc-800 pt-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-zinc-400">History</h2>
        <select
          value={field}
          onChange={(event) => setField(event.target.value)}
          aria-label="History field"
          className="h-6 rounded border border-zinc-700 bg-zinc-950 px-1 text-[11px] text-zinc-300 focus:border-sky-600 focus:outline-none"
        >
          {HISTORY_FIELDS.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
      </div>
      {events.isPending ? <Loading label="Loading history…" /> : null}
      {events.isError ? (
        <ErrorBox
          error={events.error}
          title="Could not load history"
          onRetry={() => void events.refetch()}
        />
      ) : null}
      {events.data && events.data.length === 0 ? (
        <p className="text-xs text-zinc-600">No recorded changes for {field}.</p>
      ) : null}
      <ol className="flex flex-col">
        {(events.data ?? []).map((event) => (
          // The server orders by seq — the order things actually happened,
          // newest last. Re-sorting by timestamp would invent contradictions.
          <li key={event.seq} className="flex flex-col border-l border-zinc-800 py-1 pl-2">
            <div className="flex items-baseline gap-2 text-[11px] text-zinc-500">
              <span className="font-mono tabular-nums text-zinc-600">#{event.seq}</span>
              <span title={fullTimestamp(event.ts)}>{relativeTime(event.ts)}</span>
              {event.author ? <span className="font-mono text-zinc-400">{event.author}</span> : null}
            </div>
            <div className="font-mono text-[11px] text-zinc-300">
              <span className="text-zinc-500">{event.old_value ?? "∅"}</span>
              <span className="mx-1.5 text-zinc-600">→</span>
              <span>{event.new_value ?? "∅"}</span>
            </div>
          </li>
        ))}
      </ol>
    </section>
  );
}

function FieldRail({ issue }: { issue: IssueDto }) {
  const schema = useSchema();
  const patch = usePatchIssue(issue.id);

  const statuses = schema.data?.workflow.statuses ?? [];
  const statusOptions =
    statuses.length > 0
      ? statuses.map((status) => ({ value: status.id, label: status.label }))
      : [{ value: issue.status, label: issue.status }];

  return (
    <aside className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto border-l border-zinc-800 p-3">
      <FieldRow label="Status">
        <SelectField
          ariaLabel="Status"
          value={issue.status}
          options={statusOptions}
          disabled={patch.isPending}
          onChange={(status) => patch.mutate({ status })}
        />
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
        <CommitInput
          initial={issue.labels.join(", ")}
          format="csv"
          placeholder="area, team"
          onCommit={(labels) => patch.mutate({ labels: parseCsvList(labels) })}
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
      <FieldRow label="Epic">
        <span className="font-mono text-xs text-zinc-400">{issue.epic ?? "—"}</span>
      </FieldRow>
      <FieldRow label="Reporter">
        <span className="font-mono text-xs text-zinc-400">{issue.reporter ?? "—"}</span>
      </FieldRow>
      <FieldRow label="Created">
        <span className="text-xs text-zinc-500" title={fullTimestamp(issue.created)}>
          {relativeTime(issue.created)}
        </span>
      </FieldRow>
      <FieldRow label="Updated">
        <span className="text-xs text-zinc-500" title={fullTimestamp(issue.updated)}>
          {relativeTime(issue.updated)}
        </span>
      </FieldRow>
      <HistorySection issueId={issue.id} />
    </aside>
  );
}

function DescriptionSection({ issue }: { issue: IssueDto }) {
  const [editing, setEditing] = useState(false);

  return (
    <section>
      <div className="mb-2 flex items-center justify-between gap-2">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-zinc-400">
          Description
        </h2>
        <button
          type="button"
          onClick={() => setEditing((value) => !value)}
          className={cn(
            "flex items-center gap-1 rounded border border-zinc-700 px-2 py-0.5 text-[11px]",
            editing ? "text-zinc-300 hover:border-zinc-500" : "text-zinc-400 hover:border-zinc-500",
          )}
        >
          {editing ? <X className="size-3" aria-hidden /> : <Pencil className="size-3" aria-hidden />}
          {editing ? "Done" : "Edit"}
        </button>
      </div>
      {editing ? (
        <BodyEditor issueId={issue.id} body={issue.body} />
      ) : issue.body.trim().length === 0 ? (
        <p className="rounded-md border border-dashed border-zinc-800 px-3 py-6 text-center text-xs text-zinc-600">
          No description.
        </p>
      ) : (
        <Markdown html={issue.body_html} className="text-[13px]" />
      )}
    </section>
  );
}

export function IssueDetailView({ id }: { id: string }) {
  const issue = useIssue(id);
  // Header and rail both edit this issue; one mutation hook before any early
  // return keeps hook order stable and the saving indicator shared.
  const patch = usePatchIssue(id);

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
      <header className="flex items-center gap-3 border-b border-zinc-800 px-3 py-2">
        <a
          href="#/issues"
          title="Back to issues"
          className="rounded p-1 text-zinc-500 hover:bg-zinc-900 hover:text-zinc-300"
        >
          <ArrowLeft className="size-4" aria-hidden />
        </a>
        <IssueHandle shortRef={data.short_ref} number={data.number} />
        <span
          className="font-mono text-[10px] text-zinc-600"
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
          className="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1.5 py-0.5 text-sm font-medium text-zinc-100 hover:border-zinc-700 focus:border-sky-600 focus:outline-none"
        />
        {patch.isPending ? <span className="text-[11px] text-zinc-500">Saving…</span> : null}
        <AssigneeCircles assignees={data.assignees} />
      </header>

      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-y-auto p-4">
          <DescriptionSection issue={data} />
          <CommentsSection issueId={data.id} />
        </div>
        <FieldRail issue={data} />
      </div>
    </div>
  );
}
