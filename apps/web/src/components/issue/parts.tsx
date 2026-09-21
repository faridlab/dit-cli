// The pieces an issue surface is made of, in one place because there are
// two surfaces: the side panel that opens over a list, and the full page.
// Both render the same property rows, the same always-on description editor
// and the same activity stream — only the layout around them differs.
//
// Every row emits the class recipes the approved design is written in
// (styles.css, "Workbench recipes"): `.props` / `.k` / `.v` / `.blame`,
// `.desc` / `.md`, `.act-h` / `.seg` / `.tl` / `.ev` / `.composer`. Each
// property change is one PATCH — one commit — and says so in a toast.

import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Calendar,
  Check,
  ChevronDown,
  Copy,
  Hash,
  Link2,
  Maximize2,
  MessageSquare,
  PanelRight,
  Reply,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { toast } from "sonner";
import { BodyEditor } from "../BodyEditor";
import { Markdown } from "../Markdown";
import { Avatar, AssigneeCircles, Chip, PriorityDot, StatusPill, TypeBadge } from "../badges";
import { Btn, HeadingNote, MenuButton, SectionHeading, Sp, type MenuItem } from "../chrome";
import { ApiError } from "../../lib/api";
import { getToken } from "../../lib/auth";
import { dueInfo, fullTimestamp, relativeTime, resolveIdValue } from "../../lib/format";
import {
  queryKeys,
  useAddComment,
  useComments,
  useCreateIssue,
  useIssues,
  usePatchIssue,
  useSchema,
  useStatus,
} from "../../lib/queries";
import { mergeActivity } from "../../lib/activity";
import { navigate, routeToHash, withPeek, type PeekHost, type Route } from "../../lib/router";
import { isStarred, toggleStar } from "../../lib/starred";
import type { FieldEventDto, FieldPatch, IssueDto, Priority, StatusDto } from "../../lib/types";
import { cn } from "../../lib/cn";

// ---------------------------------------------------------------------------
// Small shared helpers
// ---------------------------------------------------------------------------

/** Copy to the clipboard and say so. When the clipboard is unavailable (an
 *  insecure origin, a denied permission) the text itself goes in the toast so
 *  it can still be picked up by hand. */
export async function copyText(text: string, label: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${label} — ${text}`);
  } catch {
    toast(`${label}: ${text}`);
  }
}

/** Star or unstar, with the design's wording: a star is a private bookmark
 *  in this browser, never a file and never a commit. */
export function starWithToast(shortRef: string): void {
  const starred = toggleStar(shortRef);
  toast(starred ? "Starred (kept in this browser, never in git)" : "Unstarred");
}

const PRIORITIES: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

/** Fields the server writes on every commit; showing them as "changes" would
 *  drown the ones a person made. */
const NOISE_FIELDS = new Set(["updated", "number", "created"]);

function isNoise(event: FieldEventDto): boolean {
  if (NOISE_FIELDS.has(event.field)) return true;
  return event.field === "reporter" && event.old_value === null;
}

// The PATCH contract is typed from the server's DTO. `epic` is being added
// to it; until the generated type catches up the field is sent through this
// widening so the menu can be wired now and typed later.
type PatchWithEpic = FieldPatch & { epic?: string };

/** The bounded pool every picker reads: known aliases, known labels and the
 *  stories an issue can belong to. Closed issues count too — a label or a
 *  person is still "known" after their work is done. */
export function useIssuePool(): IssueDto[] {
  const issues = useIssues({ limit: 500 });
  return issues.data?.items ?? [];
}

/** The surfaces fetch an issue by whatever the route carried — usually the
 *  short ref — while the shared mutations invalidate by the ULID. Refresh
 *  the short-ref keys too, so a commit shows up without waiting for the
 *  live index event. */
export function useRefreshIssue(issue: IssueDto): () => void {
  const client = useQueryClient();
  return () => {
    for (const key of [issue.short_ref, issue.id]) {
      void client.invalidateQueries({ queryKey: queryKeys.issue(key) });
      void client.invalidateQueries({ queryKey: ["history", key] });
      void client.invalidateQueries({ queryKey: ["comments", key] });
    }
  };
}

export function useKnownPeople(): string[] {
  const pool = useIssuePool();
  const status = useStatus();
  return useMemo(() => {
    const set = new Set<string>();
    for (const issue of pool) {
      for (const alias of issue.assignees) set.add(alias);
      if (issue.reporter) set.add(issue.reporter);
    }
    if (status.data?.me) set.add(status.data.me);
    return [...set].sort();
  }, [pool, status.data?.me]);
}

export function useKnownLabels(): string[] {
  const pool = useIssuePool();
  return useMemo(() => {
    const set = new Set<string>();
    for (const issue of pool) for (const label of issue.labels) set.add(label);
    return [...set].sort();
  }, [pool]);
}

// ---------------------------------------------------------------------------
// Properties block
// ---------------------------------------------------------------------------

/** One `.k` / `.v` row. Editable rows open their menu on click, Enter or
 *  Space; the blame span inside opens its own menu without opening the row. */
function PropRow({
  label,
  field,
  items,
  align,
  blame,
  readOnly = false,
  children,
}: {
  label: string;
  field: string;
  items?: MenuItem[];
  align?: "start" | "end";
  blame?: ReactNode;
  readOnly?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  if (readOnly || !items) {
    return (
      <>
        <div className="k">{label}</div>
        <div className="v prop ro" data-f={field} title="Derived from git — not editable">
          {children}
          <span className="blame" />
        </div>
      </>
    );
  }
  return (
    <>
      <div className="k">{label}</div>
      {/* `relative` keeps the popover in flow so Radix can measure it; the
          shared `.menu` recipe's position:fixed would measure as 0×0 and
          push end-aligned menus off screen. */}
      <MenuButton items={items} className="relative" open={open} onOpenChange={setOpen} align={align}>
        <div
          className="v prop edit cursor-pointer"
          data-f={field}
          role="button"
          tabIndex={0}
          title="Click to change · commits immediately"
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              setOpen(true);
            }
          }}
        >
          {children}
          {blame}
        </div>
      </MenuButton>
    </>
  );
}

/** "Who touched it last" for one field, from `field_events` — computed on
 *  read, never stored (invariant 5). Click for that field's whole history. */
function Blame({
  field,
  history,
  issue,
  onShowChanges,
}: {
  field: string;
  history: FieldEventDto[];
  issue: IssueDto;
  onShowChanges: (seq?: number) => void;
}) {
  const events = history.filter((event) => event.field === field);
  const last = events[events.length - 1];
  const text = last
    ? `${last.author}, ${relativeTime(last.ts)}`
    : `${issue.reporter ?? "unknown"}, ${relativeTime(issue.created)}`;
  const items: MenuItem[] = [
    { kind: "head", label: `${field} · history (${events.length})` },
    ...(events.length > 0
      ? [...events].reverse().map(
          (event): MenuItem => ({
            label: `${event.old_value ?? "∅"} → ${event.new_value ?? "∅"}`,
            meta: `${event.author} · ${relativeTime(event.ts)}`,
            run: () => onShowChanges(event.seq),
          }),
        )
      : [{ kind: "text" as const, node: "Unchanged since the issue was created." }]),
  ];
  return (
    <MenuButton items={items} className="relative" align="end">
      <span
        className="blame blm cursor-pointer"
        data-f={field}
        role="button"
        tabIndex={-1}
        title="Who touched it last — click for the field's history"
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
      >
        {text}
      </span>
    </MenuButton>
  );
}

/** The workflow's small pill for menu rows: one letter in the category's
 *  color, so a long status list still scans. */
function MiniPill({ status }: { status: StatusDto }) {
  return (
    <span className={cn("pill", status.category)} style={{ height: 16, fontSize: 10.5 }}>
      {status.label[0]}
    </span>
  );
}

function isoInDays(days: number): string {
  return new Date(Date.now() + days * 86_400_000).toISOString().slice(0, 10);
}

/** Every frontmatter field as a `.props` grid. `compact` (the side panel)
 *  leaves out the read-only Reporter row. */
export function IssueProps({
  issue,
  history,
  compact,
  onShowChanges,
}: {
  issue: IssueDto;
  history: FieldEventDto[];
  compact: boolean;
  /** Switch the activity stream to Changes, optionally scrolling to a seq. */
  onShowChanges: (seq?: number) => void;
}) {
  const schema = useSchema();
  const patch = usePatchIssue(issue.id);
  const refresh = useRefreshIssue(issue);
  const people = useKnownPeople();
  const labels = useKnownLabels();
  const pool = useIssuePool();

  const statuses = schema.data?.workflow.statuses ?? [];
  const status = statuses.find((each) => each.id === issue.status);
  const stories = pool.filter((each) => each.type === "story" && each.id !== issue.id);
  const epic = issue.epic ? pool.find((each) => each.id === issue.epic) : undefined;
  // A cleared field can come back as "" from a server that stores the
  // empty string; both mean "none" here.
  const priority = issue.priority || null;
  const due = dueInfo(issue.due || null);

  /** One field, one commit, one toast. */
  const commit = (field: string, set: PatchWithEpic, shown: string) => {
    patch.mutate(set as FieldPatch, {
      onSuccess: () => {
        refresh();
        toast(`Committed · ${field} → ${shown}`);
      },
    });
  };

  const blame = (field: string) => (
    <Blame field={field} history={history} issue={issue} onShowChanges={onShowChanges} />
  );

  const statusItems: MenuItem[] = [
    { kind: "head", label: "Status" },
    ...statuses.map(
      (each): MenuItem => ({
        label: each.label,
        icon: <MiniPill status={each} />,
        on: issue.status === each.id,
        run: () => {
          if (issue.status !== each.id) commit("status", { status: each.id }, each.label);
        },
      }),
    ),
  ];

  const priorityItems: MenuItem[] = [
    { kind: "head", label: "Priority" },
    ...[...PRIORITIES, null].map(
      (each): MenuItem => ({
        label: each
          ? `${each.toUpperCase()}${each === "p0" ? " · drop everything" : each === "p1" ? " · this week" : ""}`
          : "No priority",
        icon: <PriorityDot priority={each} />,
        on: priority === each,
        run: () => {
          if (priority === each) return;
          // Clearing sends the empty string — the server's "unset" spelling.
          // If it refuses, the toast says so; nothing is pretended.
          commit("priority", { priority: each ?? "" }, each ? each.toUpperCase() : "none");
        },
      }),
    ),
  ];

  const assigneeItems: MenuItem[] = [
    { kind: "head", label: "Assignees · click to toggle" },
    ...people.map(
      (alias): MenuItem => ({
        label: alias,
        check: issue.assignees.includes(alias),
        run: () => {
          const next = issue.assignees.includes(alias)
            ? issue.assignees.filter((each) => each !== alias)
            : [...issue.assignees, alias];
          commit("assignees", { assignees: next }, next.join(", ") || "none");
        },
      }),
    ),
    { kind: "sep" },
    {
      label: "Unassign everyone",
      icon: <X className="i" aria-hidden />,
      run: () => commit("assignees", { assignees: [] }, "none"),
    },
  ];

  const labelItems: MenuItem[] = [
    { kind: "head", label: "Labels" },
    {
      kind: "input",
      placeholder: "New label, e.g. area:web",
      button: "Add",
      run: (value) => {
        if (value && !issue.labels.includes(value)) {
          const next = [...issue.labels, value];
          commit("labels", { labels: next }, next.join(", "));
        }
      },
    },
    ...labels.map(
      (label): MenuItem => ({
        label,
        check: issue.labels.includes(label),
        run: () => {
          const next = issue.labels.includes(label)
            ? issue.labels.filter((each) => each !== label)
            : [...issue.labels, label];
          commit("labels", { labels: next }, next.join(", ") || "none");
        },
      }),
    ),
  ];

  const epicItems: MenuItem[] = [
    { kind: "head", label: "Epic" },
    {
      label: "None",
      on: !issue.epic,
      run: () => {
        if (issue.epic) commit("epic", { epic: "" }, "none");
      },
    },
    ...stories.map(
      (story): MenuItem => ({
        label: story.title,
        icon: <TypeBadge type="story" />,
        on: issue.epic === story.id,
        run: () => {
          if (issue.epic !== story.id) commit("epic", { epic: story.id }, story.title);
        },
      }),
    ),
  ];

  const estimateItems: MenuItem[] = [
    { kind: "head", label: "Estimate (points)" },
    {
      kind: "input",
      placeholder: "e.g. 3",
      type: "number",
      value: issue.estimate === null ? "" : String(issue.estimate),
      run: (value) => {
        if (value === "") return;
        const n = Number(value);
        if (Number.isFinite(n) && n !== issue.estimate) commit("estimate", { estimate: n }, `${n} pt`);
      },
    },
    ...[1, 2, 3, 5, 8].map(
      (n): MenuItem => ({
        label: `${n} pt`,
        on: issue.estimate === n,
        run: () => {
          if (issue.estimate !== n) commit("estimate", { estimate: n }, `${n} pt`);
        },
      }),
    ),
  ];

  const dueItems: MenuItem[] = [
    { kind: "head", label: "Due date" },
    {
      kind: "input",
      placeholder: "YYYY-MM-DD",
      type: "date",
      value: issue.due ?? "",
      run: (value) => {
        if ((value || null) !== (issue.due || null)) commit("due", { due: value }, value || "none");
      },
    },
    {
      label: "Tomorrow",
      icon: <Calendar className="i" aria-hidden />,
      run: () => commit("due", { due: isoInDays(1) }, isoInDays(1)),
    },
    {
      label: "In a week",
      icon: <Calendar className="i" aria-hidden />,
      run: () => commit("due", { due: isoInDays(7) }, isoInDays(7)),
    },
    {
      label: "Clear",
      icon: <X className="i" aria-hidden />,
      run: () => {
        if (issue.due) commit("due", { due: "" }, "none");
      },
    },
  ];

  return (
    <div className="props">
      <PropRow label="Status" field="status" items={statusItems} blame={blame("status")}>
        {status ? <StatusPill status={status} /> : <StatusPill label={issue.status} />}
        <ChevronDown className="i" aria-hidden />
      </PropRow>

      <PropRow label="Priority" field="priority" items={priorityItems} blame={blame("priority")}>
        <PriorityDot priority={priority} />
        <span>{priority ? priority.toUpperCase() : <span className="ph">No priority</span>}</span>
      </PropRow>

      <PropRow label="Assignees" field="assignees" items={assigneeItems} blame={blame("assignees")}>
        {issue.assignees.length > 0 ? (
          <>
            <AssigneeCircles assignees={issue.assignees} hollow={false} />
            <span>{issue.assignees.join(", ")}</span>
          </>
        ) : (
          <span className="ph">Unassigned</span>
        )}
      </PropRow>

      <PropRow label="Labels" field="labels" items={labelItems} blame={blame("labels")}>
        <span className="lbls">
          {issue.labels.map((label) => (
            <Chip key={label} label={label} />
          ))}
          <span
            className="chip"
            style={{ background: "transparent", border: "1px dashed var(--edge-2)", color: "var(--faint)" }}
          >
            +
          </span>
        </span>
      </PropRow>

      <PropRow label="Epic" field="epic" items={epicItems} blame={blame("epic")}>
        {issue.epic ? (
          <>
            <TypeBadge type="story" />
            <span>{epic?.title ?? issue.epic}</span>
          </>
        ) : (
          <span className="ph">None</span>
        )}
      </PropRow>

      <PropRow label="Estimate" field="estimate" items={estimateItems} blame={blame("estimate")}>
        {issue.estimate !== null ? (
          <span className="mono">{issue.estimate} pt</span>
        ) : (
          <span className="ph">—</span>
        )}
      </PropRow>

      <PropRow label="Due" field="due" items={dueItems} blame={blame("due")}>
        {due ? (
          <span className={cn("due", due.cls)} style={{ fontSize: 12.5 }}>
            {issue.due} · {due.text}
          </span>
        ) : (
          <span className="ph">No date</span>
        )}
      </PropRow>

      {compact ? null : (
        <PropRow label="Reporter" field="reporter" readOnly>
          {issue.reporter ? (
            <>
              <Avatar name={issue.reporter} />
              <span>{issue.reporter}</span>
            </>
          ) : (
            <span className="ph">—</span>
          )}{" "}
          <span className="ph" style={{ fontSize: 11 }}>
            · from the creating commit
          </span>
        </PropRow>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Title and description
// ---------------------------------------------------------------------------

/** The title, edited in place like the design's contenteditable heading:
 *  Enter commits, Escape abandons, blur commits when something changed. An
 *  empty title is refused — it is how the issue is read everywhere else. */
export function IssueTitle({ issue, as, className }: { issue: IssueDto; as: "h1" | "h2"; className?: string }) {
  const patch = usePatchIssue(issue.id);
  const refresh = useRefreshIssue(issue);
  const Tag = as;
  const commit = (element: HTMLElement) => {
    const title = element.textContent?.trim() ?? "";
    if (title.length > 0 && title !== issue.title) {
      patch.mutate(
        { title },
        {
          onSuccess: () => {
            refresh();
            toast(`Committed · title → ${title}`);
          },
        },
      );
    } else {
      element.textContent = issue.title;
    }
  };
  return (
    <Tag
      // Remount when the server's title changes so a live refresh never
      // overwrites what someone is typing unless the title itself moved.
      key={issue.title}
      className={cn("ttl issue-title", className)}
      contentEditable
      suppressContentEditableWarning
      spellCheck={false}
      title="Edit the title — Enter commits"
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          event.currentTarget.blur();
        }
        // The panel's Escape closes it; inside the title it should only
        // abandon the edit.
        if (event.key === "Escape") {
          event.stopPropagation();
          event.currentTarget.textContent = issue.title;
          event.currentTarget.blur();
        }
      }}
      onBlur={(event) => commit(event.currentTarget)}
    >
      {issue.title}
    </Tag>
  );
}

/** The description is the editor: always on, one commit per typing pause.
 *  The heading says where the bytes stand — saved when the buffer matches
 *  the repo, "editing" while it does not. */
export function DescriptionSection({ issue }: { issue: IssueDto }) {
  const refresh = useRefreshIssue(issue);
  return (
    <div className="group/desc">
      <BodyEditor
        key={issue.id}
        issueId={issue.id}
        body={issue.body}
        onSaved={refresh}
        className="desc"
        editorClassName="md min-h-6 [&_.dit-rich]:px-0 [&_.dit-rich]:text-[13.5px] [&_.dit-rich]:leading-[1.6]"
        header={({ dirty, saving, mode, setMode }) => (
          <SectionHeading className="mb-2">
            Description
            <Sp />
            {/* Source mode is the escape hatch for bytes the rich editor
                cannot own; it stays out of the way until the section is
                hovered or focused. */}
            <span
              className="seg opacity-0 transition-opacity group-hover/desc:opacity-100 group-focus-within/desc:opacity-100"
              style={{ marginLeft: 0 }}
              role="group"
              aria-label="Editor mode"
            >
              <button type="button" className={cn(mode === "rich" && "on")} onClick={() => setMode("rich")}>
                Rich
              </button>
              <button type="button" className={cn(mode === "source" && "on")} onClick={() => setMode("source")}>
                Source
              </button>
            </span>
            <HeadingNote>
              {saving
                ? "committing…"
                : dirty
                  ? "editing · commits after a pause"
                  : `saved · ${relativeTime(issue.updated)}`}
            </HeadingNote>
          </SectionHeading>
        )}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Activity
// ---------------------------------------------------------------------------

export type ActivityFilter = "all" | "comments" | "changes";

const FILTERS: Array<{ value: ActivityFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "comments", label: "Comments" },
  { value: "changes", label: "Changes" },
];

/** A field value in the stream: status as its pill, priority as dot + P1,
 *  nothing as a quiet dash, everything else mono. */
function ChangeValue({
  field,
  value,
  statuses,
  titleOf,
}: {
  field: string;
  value: string | null;
  statuses: StatusDto[];
  /** Resolves an issue id to its title, for `epic` and `blocked_by`. */
  titleOf?: (id: string) => string | undefined;
}) {
  if (value === null || value.length === 0 || value === "—" || value === "null") {
    return <span style={{ color: "var(--faint)" }}>—</span>;
  }
  if (field === "status") {
    const status = statuses.find((each) => each.id === value);
    return status ? <StatusPill status={status} /> : <StatusPill label={value} />;
  }
  if (field === "priority" && /^p\d$/.test(value)) {
    return (
      <>
        <PriorityDot priority={value as Priority} /> {value.toUpperCase()}
      </>
    );
  }
  return <span className="mono">{titleOf ? resolveIdValue(field, value, titleOf) : value}</span>;
}

/** Comments and field changes as one stream — what happened to this issue,
 *  newest first, down to the line that says it was created. Both come out of
 *  git; neither is stored as an activity log (invariant 5). The segment is owned by the surface so the
 *  blame menus and the page's History rail can switch it. */
export function IssueActivity({
  issue,
  history,
  filter,
  onFilterChange,
  highlight,
}: {
  issue: IssueDto;
  history: FieldEventDto[];
  filter: ActivityFilter;
  onFilterChange: (filter: ActivityFilter) => void;
  /** Scroll the change with this seq into view and flash it. The nonce
   *  makes clicking the same row twice work twice. */
  highlight: { seq: number; nonce: number } | null;
}) {
  const comments = useComments(issue.id);
  const add = useAddComment(issue.id);
  const refresh = useRefreshIssue(issue);
  const schema = useSchema();
  const statuses = schema.data?.workflow.statuses ?? [];
  // An `epic` change records an issue id; the pool turns it into the title.
  const pool = useIssuePool();
  const titleOf = (id: string) => pool.find((candidate) => candidate.id === id)?.title;
  const [draft, setDraft] = useState("");
  const listRef = useRef<HTMLDivElement>(null);

  const entries = useMemo(() => mergeActivity(history, comments.data ?? []), [history, comments.data]);

  // Who wrote each comment, for the "replied to AUTHOR" line a thread shows.
  const authorOf = useMemo(() => {
    const byId = new Map<string, string>();
    for (const comment of comments.data ?? []) byId.set(comment.id, comment.author);
    return (id: string): string | null => byId.get(id) ?? null;
  }, [comments.data]);
  // Which comment the composer is answering, if any.
  const [replyingTo, setReplyingTo] = useState<{ id: string; author: string } | null>(null);

  type Shown =
    | {
        kind: "cm";
        id: string;
        author: string;
        ts: string;
        html: string;
        replyTo: string | null;
      }
    | { kind: "chg"; seq: number; author: string; ts: string; event: FieldEventDto };
  // Newest first: the panel is opened to see what just happened, so the
  // stream runs backwards from now down to the issue's creation, which is
  // the last line. The merge itself is untouched — this walks it in reverse
  // rather than re-sorting, so field events keep their `seq` order
  // (invariant 9). Within one commit the events stay as given: they happened
  // in one act and share one timestamp, so reversing them says nothing true.
  const shown: Shown[] = [];
  for (let i = entries.length - 1; i >= 0; i -= 1) {
    const entry = entries[i];
    if (entry === undefined) continue;
    if (entry.kind === "comment") {
      if (filter !== "changes") {
        shown.push({
          kind: "cm",
          id: entry.id,
          author: entry.author,
          ts: entry.ts,
          html: entry.bodyHtml,
          replyTo: entry.replyTo,
        });
      }
      continue;
    }
    // The birth of the issue is the last line of the stream, not a burst
    // of "changed" rows.
    if (entry.creation || filter === "comments") continue;
    for (const event of entry.events) {
      if (isNoise(event)) continue;
      shown.push({ kind: "chg", seq: event.seq, author: entry.author, ts: event.ts, event });
    }
  }

  useEffect(() => {
    if (!highlight || !listRef.current) return;
    const target = listRef.current.querySelector<HTMLElement>(`[data-seq="${highlight.seq}"]`);
    if (!target) return;
    target.scrollIntoView({ behavior: "smooth", block: "center" });
    target.animate([{ background: "var(--active)" }, { background: "transparent" }], { duration: 1200 });
  }, [highlight]);

  // A comment is a discrete message in git history, so the composer sends
  // when the writer says so — the button or ⌘↵ — never on a typing pause.
  // A reply rides `reply_to` (§4.4) and lands in the same thread.
  const send = () => {
    const body = draft.trim();
    if (body.length === 0) {
      toast("Write something first");
      return;
    }
    if (add.isPending) return;
    add.mutate(
      { body, replyTo: replyingTo?.id ?? null },
      {
        onSuccess: () => {
          setDraft("");
          setReplyingTo(null);
          refresh();
          toast(replyingTo ? "Reply committed" : "Comment committed");
        },
      },
    );
  };

  return (
    <div>
      <div className="act-h">
        <SectionHeading>Activity</SectionHeading>
        <div className="seg" role="group" aria-label="Filter activity">
          {FILTERS.map((option) => (
            <button
              key={option.value}
              type="button"
              className={cn("actf", filter === option.value && "on")}
              aria-pressed={filter === option.value}
              onClick={() => onFilterChange(option.value)}
            >
              {option.label}
            </button>
          ))}
        </div>
      </div>
      <div className="tl" ref={listRef}>
        {shown.map((entry) =>
          entry.kind === "cm" ? (
            <div className={cn("ev", entry.replyTo && "reply")} key={entry.id}>
              <Avatar name={entry.author} />
              <div>
                <div className="who">
                  <b>{entry.author}</b>{" "}
                  {entry.replyTo ? (
                    <>
                      replied to <b>{authorOf(entry.replyTo) ?? "a comment"}</b>
                    </>
                  ) : (
                    "commented"
                  )}
                  <span className="ts" title={fullTimestamp(entry.ts)}>
                    {relativeTime(entry.ts)}
                  </span>
                </div>
                <div className="cm">
                  <Markdown html={entry.html} className="md" />
                </div>
                <button
                  type="button"
                  className="replyBtn"
                  onClick={() =>
                    setReplyingTo({ id: entry.id, author: entry.author })
                  }
                  title="Answer in this thread"
                >
                  <Reply className="i" aria-hidden /> Reply
                </button>
              </div>
            </div>
          ) : (
            <div className="ev sys" key={`chg-${entry.seq}`} data-seq={entry.seq}>
              <span className="dotc">
                <i />
              </span>
              <div>
                <div className="who">
                  <b>{entry.author}</b> changed {entry.event.field}
                  <span className="ts" title={`${fullTimestamp(entry.ts)}\ncommit ${entry.event.commit_sha}`}>
                    {relativeTime(entry.ts)}
                  </span>
                </div>
                <div className="chg">
                  <ChangeValue field={entry.event.field} value={entry.event.old_value} statuses={statuses} titleOf={titleOf} />
                  <span className="arr">→</span>
                  <ChangeValue field={entry.event.field} value={entry.event.new_value} statuses={statuses} titleOf={titleOf} />
                </div>
              </div>
            </div>
          ),
        )}
        {/* The oldest line there can be, so it closes a newest-first stream. */}
        <div className="ev sys">
          <span className="dotc">
            <i />
          </span>
          <div>
            <div className="who">
              <b>{issue.reporter ?? history[0]?.author ?? "unknown"}</b> created this issue
              <span className="ts" title={fullTimestamp(issue.created)}>
                {relativeTime(issue.created)}
              </span>
            </div>
          </div>
        </div>
        {shown.length === 0 && !comments.isPending ? <p className="empty">Nothing here yet.</p> : null}
        {comments.isError ? (
          <p className="empty" style={{ color: "var(--crit)" }}>
            Could not load comments: {comments.error instanceof Error ? comments.error.message : String(comments.error)}
          </p>
        ) : null}
      </div>
      <div className="composer">
        <MessageSquare className="i" aria-hidden />
        <input
          className="cmIn"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
              event.preventDefault();
              send();
            }
          }}
          placeholder={
            replyingTo
              ? `Reply to ${replyingTo.author}… Markdown`
              : "Write a comment… Markdown"
          }
          aria-label={replyingTo ? "Reply" : "Comment"}
        />
        <div className="bar2">
          {replyingTo ? (
            <span className="chip ctx" style={{ alignItems: "center", display: "inline-flex", gap: 6 }}>
              replying to {replyingTo.author}
              <button
                type="button"
                onClick={() => setReplyingTo(null)}
                title="Write a top-level comment instead"
                style={{ border: 0, background: "transparent", cursor: "pointer", padding: 0, color: "inherit" }}
              >
                <X className="i" aria-hidden />
              </button>
            </span>
          ) : (
            <span style={{ fontSize: 11, color: "var(--faint)" }}>
              Comments are discrete messages in git history — sent when you say so.
            </span>
          )}
          <Sp />
          <Btn primary className="cmSend" onClick={send} disabled={add.isPending}>
            {replyingTo ? "Reply" : "Comment"}
            <kbd>⌘↵</kbd>
          </Btn>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The "…" menu
// ---------------------------------------------------------------------------

/** DELETE /api/issues/{id}. Lives here rather than in lib/api so the two
 *  issue surfaces can ship it without waiting on the shared client; it
 *  follows the same token and error contract. */
async function deleteIssueRequest(id: string): Promise<void> {
  const token = getToken();
  const headers: Record<string, string> = {};
  if (token) headers.Authorization = `Bearer ${token}`;
  let res: Response;
  try {
    res = await fetch(`/api/issues/${encodeURIComponent(id)}`, { method: "DELETE", headers });
  } catch (cause) {
    throw new ApiError(cause instanceof Error ? `Network error: ${cause.message}` : "Network error", 0);
  }
  if (!res.ok) {
    let message = `Request failed (${res.status})`;
    try {
      const body = (await res.json()) as { error?: unknown };
      if (typeof body.error === "string" && body.error.length > 0) message = body.error;
    } catch {
      // Non-JSON error body — keep the generic message.
    }
    throw new ApiError(message, res.status);
  }
}

function useDeleteIssue() {
  const client = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteIssueRequest(id),
    onSuccess: (_data, id) => {
      client.removeQueries({ queryKey: queryKeys.issue(id) });
      void client.invalidateQueries({ queryKey: ["issues"] });
      void client.invalidateQueries({ queryKey: queryKeys.board });
      void client.invalidateQueries({ queryKey: queryKeys.status });
      void client.invalidateQueries({ queryKey: ["activity"] });
    },
    onError: (error) => {
      toast.error(`Could not delete: ${error instanceof Error ? error.message : String(error)}`);
    },
  });
}

/** The items behind the "…" button, shared by the panel and the page. */
export function useMoreMenu({
  issue,
  inPeek,
  route,
  from,
  onToggleSurface,
  onDeleted,
}: {
  issue: IssueDto;
  inPeek: boolean;
  /** The route the surface sits on — what "Copy link" points at. */
  route: Route;
  /** For the page: the list it came from, where a deletion returns to. */
  from: PeekHost;
  /** "Open as page" in the panel, "Show as panel" on the page. */
  onToggleSurface: () => void;
  onDeleted: () => void;
}): MenuItem[] {
  const schema = useSchema();
  const patch = usePatchIssue(issue.id);
  const create = useCreateIssue();
  const remove = useDeleteIssue();
  const refresh = useRefreshIssue(issue);
  const starred = isStarred(issue.short_ref);
  const statuses = schema.data?.workflow.statuses ?? [];
  const doneStatus = statuses.find((each) => each.category === "done");
  const todoStatus = statuses.find((each) => each.category === "todo");
  const isDone = statuses.find((each) => each.id === issue.status)?.category === "done";
  const link = `${window.location.origin}${window.location.pathname}${routeToHash(withPeek(route, issue.short_ref))}`;
  const handle = issue.number !== null ? `#${issue.number}` : issue.short_ref;

  const items: MenuItem[] = [
    {
      label: "Copy short ref",
      icon: <Copy className="i" aria-hidden />,
      meta: issue.short_ref,
      run: () => void copyText(issue.short_ref, "Short ref copied"),
    },
    { label: "Copy link", icon: <Link2 className="i" aria-hidden />, run: () => void copyText(link, "Link copied") },
    {
      label: "Copy as markdown link",
      icon: <Hash className="i" aria-hidden />,
      run: () => void copyText(`[[${issue.short_ref}]] ${issue.title}`, "Wiki-link copied"),
    },
    { kind: "sep" },
    {
      label: inPeek ? "Open as page" : "Show as panel",
      icon: inPeek ? <Maximize2 className="i" aria-hidden /> : <PanelRight className="i" aria-hidden />,
      kbd: inPeek ? "⌘↵" : undefined,
      run: onToggleSurface,
    },
    {
      label: starred ? "Unstar" : "Star",
      icon: <Star className="i" aria-hidden />,
      run: () => starWithToast(issue.short_ref),
    },
    {
      label: "Duplicate",
      icon: <Copy className="i" aria-hidden />,
      disabled: create.isPending,
      run: () => {
        const first = statuses[0];
        create.mutate(
          {
            title: `${issue.title} (copy)`,
            type: issue.type,
            ...(issue.priority ? { priority: issue.priority } : {}),
            ...(first ? { status: first.id } : {}),
            assignees: [...issue.assignees],
            labels: [...issue.labels],
            ...(issue.estimate !== null ? { estimate: issue.estimate } : {}),
            body: issue.body,
          },
          {
            onSuccess: (created) => {
              toast(`Created ${created.number !== null ? `#${created.number}` : created.short_ref}`);
              if (inPeek) navigate(withPeek(route, created.short_ref));
              else navigate({ name: "issue", id: created.short_ref, from });
            },
          },
        );
      },
    },
    { kind: "sep" },
  ];

  if (isDone ? todoStatus : doneStatus) {
    const target = isDone ? todoStatus : doneStatus;
    if (target) {
      items.push({
        label: isDone ? "Reopen" : "Mark done",
        icon: <Check className="i" aria-hidden />,
        run: () =>
          patch.mutate(
            { status: target.id },
            {
              onSuccess: () => {
                refresh();
                toast(`Committed · status → ${target.label}`);
              },
            },
          ),
      });
    }
  }

  items.push({
    label: "Delete issue…",
    icon: <Trash2 className="i" aria-hidden />,
    danger: true,
    confirm: "Click again to confirm delete",
    disabled: remove.isPending,
    run: () =>
      remove.mutate(issue.id, {
        onSuccess: () => {
          toast(`Deleted ${handle} · committed (git keeps the history)`);
          onDeleted();
        },
      }),
  });

  return items;
}
