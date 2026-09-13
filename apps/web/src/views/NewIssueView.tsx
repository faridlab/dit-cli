// New issue as a page, not a dialog: it opens looking like the issue page it
// becomes, with the title and the always-on description editor ready. The
// issue is created on the writer's word (⌘↵ or the button) — before that,
// nothing lands in the repo, so an abandoned draft costs nothing. On
// creation the route becomes the real issue and editing simply continues.
//
// The markup is the design's `.compose` recipe; every property is local
// draft state until Create sends one POST.

import { lazy, Suspense, useEffect, useState, type ReactNode } from "react";
import { ChevronDown, Plus } from "lucide-react";
import { toast } from "sonner";
import { AssigneeCircles, Chip, PriorityDot, StatusPill, TypeBadge } from "../components/badges";
import { Btn, MenuButton, SectionHeading, Sp, type MenuItem } from "../components/chrome";
import { Loading } from "../components/states";
import { useKnownLabels, useKnownPeople } from "../components/issue/parts";
import { ISSUE_TYPES } from "../lib/lists";
import { useCreateIssue, useSchema, useSettings } from "../lib/queries";
import { navigate, useRoute } from "../lib/router";
import type { IssueType, Priority } from "../lib/types";
import { cn } from "../lib/cn";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

const TYPES: IssueType[] = ["task", "bug", "story", "spike", "chore"];
const PRIORITIES: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

function isIssueType(value: string | null | undefined): value is IssueType {
  return value !== null && value !== undefined && (ISSUE_TYPES as readonly string[]).includes(value);
}

/** One draft property: label, then the value as the menu's trigger. */
function DraftRow({
  label,
  field,
  items,
  children,
}: {
  label: string;
  field: string;
  items: MenuItem[];
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <div className="k">{label}</div>
      <MenuButton items={items} className="relative" open={open} onOpenChange={setOpen}>
        <div
          className="v nprop cursor-pointer"
          data-f={field}
          role="button"
          tabIndex={0}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") {
              event.preventDefault();
              setOpen(true);
            }
          }}
        >
          {children}
        </div>
      </MenuButton>
    </>
  );
}

export function NewIssueView({ onCreated }: { onCreated: (shortRef: string) => void }) {
  const route = useRoute();
  const schema = useSchema();
  const settings = useSettings();
  const create = useCreateIssue();
  const people = useKnownPeople();
  const knownLabels = useKnownLabels();

  const [title, setTitle] = useState("");
  // The roadmap's "New epic" arrives with the type in the route.
  const [type, setType] = useState<IssueType>(() =>
    route.name === "new-issue" && isIssueType(route.type) ? route.type : "task",
  );
  const [status, setStatus] = useState("");
  const [priority, setPriority] = useState<Priority | null>(null);
  const [assignees, setAssignees] = useState<string[]>([]);
  const [labels, setLabels] = useState<string[]>([]);
  const [body, setBody] = useState("");
  const [mode, setMode] = useState<"rich" | "source">("rich");

  const statuses = schema.data?.workflow.statuses ?? [];
  // The workflow's first status is the default — the same hand-off the
  // quick-capture on Home uses.
  useEffect(() => {
    if (status === "" && statuses[0]) setStatus(statuses[0].id);
  }, [statuses, status]);
  const statusDto = statuses.find((each) => each.id === status);

  const submit = (markdown?: string) => {
    const trimmed = title.trim();
    if (trimmed.length === 0) {
      toast("A title is required");
      document.getElementById("nTitle")?.focus();
      return;
    }
    if (create.isPending) return;
    create.mutate(
      {
        title: trimmed,
        type,
        ...(priority ? { priority } : {}),
        ...(status ? { status } : {}),
        assignees,
        labels,
        // The editor hands over its just-serialized bytes on ⌘↵; the button
        // path uses the draft as it stands.
        body: markdown ?? body,
      },
      {
        onSuccess: (issue) => {
          const label = statusDto?.label ?? status;
          toast(`Created ${issue.number !== null ? `#${issue.number}` : issue.short_ref}${label ? ` in ${label}` : ""}`);
          onCreated(issue.short_ref);
        },
      },
    );
  };

  const cancel = () => {
    if (window.history.length > 1) window.history.back();
    else navigate({ name: "issues", q: null });
  };

  const typeItems: MenuItem[] = TYPES.map((each) => ({
    label: each,
    icon: <TypeBadge type={each} />,
    on: type === each,
    run: () => setType(each),
  }));
  const statusItems: MenuItem[] = statuses.map((each) => ({
    label: each.label,
    on: status === each.id,
    run: () => setStatus(each.id),
  }));
  const priorityItems: MenuItem[] = [...PRIORITIES, null].map((each) => ({
    label: each ? each.toUpperCase() : "No priority",
    icon: <PriorityDot priority={each} />,
    on: priority === each,
    run: () => setPriority(each),
  }));
  const assigneeItems: MenuItem[] = people.map((alias) => ({
    label: alias,
    check: assignees.includes(alias),
    run: () =>
      setAssignees((current) =>
        current.includes(alias) ? current.filter((each) => each !== alias) : [...current, alias],
      ),
  }));
  const labelItems: MenuItem[] = [
    {
      kind: "input",
      placeholder: "New label",
      button: "Add",
      run: (value) => {
        if (value && !labels.includes(value)) setLabels((current) => [...current, value]);
      },
    },
    ...knownLabels.map(
      (label): MenuItem => ({
        label,
        check: labels.includes(label),
        run: () =>
          setLabels((current) =>
            current.includes(label) ? current.filter((each) => each !== label) : [...current, label],
          ),
      }),
    ),
  ];

  const templates = settings.data?.templates ?? [];
  const template = templates.includes(type) ? `${type}.md` : "default.md";

  return (
    <div
      className="compose"
      onKeyDown={(event) => {
        if (!(event.metaKey || event.ctrlKey) || event.key !== "Enter") return;
        // The rich editor handles its own ⌘↵ (it hands over the serialized
        // bytes through onSave); anywhere else on the page it creates.
        if (event.target instanceof HTMLElement && event.target.closest(".ProseMirror")) return;
        event.preventDefault();
        submit();
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 8, color: "var(--muted)", fontSize: 12 }}>
        <Plus className="i" aria-hidden />
        Nothing is committed until you press Create. The page becomes the issue.
      </div>
      <input
        id="nTitle"
        className="ttl-in"
        placeholder="Issue title"
        aria-label="Title"
        autoFocus
        value={title}
        onChange={(event) => setTitle(event.target.value)}
      />
      <div className="props" style={{ gridTemplateColumns: "88px minmax(0,1fr)" }}>
        <DraftRow label="Type" field="type" items={typeItems}>
          <TypeBadge type={type} />
          <span>{type}</span>
          <ChevronDown className="i" aria-hidden />
        </DraftRow>
        <DraftRow label="Status" field="status" items={statusItems}>
          {statusDto ? <StatusPill status={statusDto} /> : <StatusPill label={status || "—"} />}
          <ChevronDown className="i" aria-hidden />
        </DraftRow>
        <DraftRow label="Priority" field="priority" items={priorityItems}>
          <PriorityDot priority={priority} />
          <span>{priority ? priority.toUpperCase() : <span className="ph">No priority</span>}</span>
          <ChevronDown className="i" aria-hidden />
        </DraftRow>
        <DraftRow label="Assignees" field="assignees" items={assigneeItems}>
          {assignees.length > 0 ? (
            <>
              <AssigneeCircles assignees={assignees} hollow={false} />
              <span>{assignees.join(", ")}</span>
            </>
          ) : (
            <span className="ph">Unassigned</span>
          )}
          <ChevronDown className="i" aria-hidden />
        </DraftRow>
        <DraftRow label="Labels" field="labels" items={labelItems}>
          <span className="lbls">
            {labels.map((label) => (
              <Chip key={label} label={label} />
            ))}
            <span
              className="chip"
              style={{ background: "transparent", border: "1px dashed var(--edge-2)", color: "var(--faint)" }}
            >
              +
            </span>
          </span>
        </DraftRow>
      </div>

      <div>
        <SectionHeading className="mb-2">Description</SectionHeading>
        <div
          className={cn(
            "body-in md focus-within:[border-color:var(--accent)]",
            "[&_.dit-rich]:px-0 [&_.dit-rich]:text-[13.5px] [&_.dit-rich]:leading-[1.6]",
          )}
          id="nBody"
        >
          <Suspense fallback={<Loading label="Loading editor…" />}>
            {mode === "rich" ? (
              <RichEditor
                value={body}
                onChange={setBody}
                onSave={submit}
                onFallbackToSource={() => setMode("source")}
              />
            ) : (
              <CodeMirrorEditor value={body} onChange={setBody} onSave={submit} />
            )}
          </Suspense>
        </div>
      </div>

      <div className="actions">
        <span style={{ fontSize: 11.5, color: "var(--faint)" }}>
          Template: <span className="mono">{template}</span>
          {template !== "default.md" ? " (evidence-first)" : ""}
        </span>
        <Sp />
        <Btn onClick={cancel}>Cancel</Btn>
        <Btn primary onClick={() => submit()} disabled={create.isPending}>
          {create.isPending ? "Creating…" : "Create"}
          <kbd>⌘↵</kbd>
        </Btn>
      </div>
    </div>
  );
}
