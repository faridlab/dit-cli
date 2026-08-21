// New issue as a page, not a dialog: it opens looking exactly like the
// issue detail view it becomes, with the title input and the always-on
// description editor ready. The issue is created on the writer's word
// (⌘Enter or the button) — before that, nothing lands in the repo, so an
// abandoned draft costs nothing. On creation the route becomes the real
// issue and editing simply continues there.

import { lazy, Suspense, useEffect, useState } from "react";
import { ArrowLeft } from "lucide-react";
import { SelectField } from "../components/SelectField";
import { PriorityDot, TypeBadge } from "../components/badges";
import { BUTTON_PRIMARY, INPUT_CLASS, SectionHeading } from "../components/chrome";
import { Loading } from "../components/states";
import { EditorModeToggle, type EditorMode } from "../components/EditorModeToggle";
import { useCreateIssue, useSchema } from "../lib/queries";
import { parseCsvList } from "../lib/format";
import type { IssueType, Priority } from "../lib/types";
import { cn } from "../lib/cn";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

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
    <div className="flex w-[128px] flex-col gap-1">
      <span className="text-[10.5px] font-medium uppercase tracking-[0.05em] text-zinc-500">
        {label}
      </span>
      {children}
    </div>
  );
}

export function NewIssueView({ onCreated }: { onCreated: (shortRef: string) => void }) {
  const schema = useSchema();
  const create = useCreateIssue();

  const [title, setTitle] = useState("");
  const [type, setType] = useState<IssueType>("task");
  const [priority, setPriority] = useState("");
  const [status, setStatus] = useState("");
  const [labels, setLabels] = useState("");
  const [body, setBody] = useState("");
  const [mode, setMode] = useState<EditorMode>("rich");
  const [validation, setValidation] = useState<string | null>(null);

  // The workflow's first status is the default — the same hand-off the
  // quick-capture on Home uses.
  useEffect(() => {
    if (status === "" && schema.data) {
      const first = schema.data.workflow.statuses[0];
      if (first) setStatus(first.id);
    }
  }, [schema.data, status]);

  const submit = (markdown?: string) => {
    const trimmed = title.trim();
    if (trimmed.length === 0) {
      setValidation("A title is required.");
      return;
    }
    if (create.isPending) return;
    setValidation(null);
    create.mutate(
      {
        title: trimmed,
        type,
        ...(priority ? { priority } : {}),
        ...(status ? { status } : {}),
        labels: parseCsvList(labels),
        // The editor hands over its just-serialized bytes on Mod+Enter; the
        // button path uses the draft as it stands.
        body: markdown ?? body,
      },
      { onSuccess: (issue) => onCreated(issue.short_ref) },
    );
  };

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
        <span className="rounded bg-edge px-1.5 py-0.5 font-mono text-[10.5px] font-medium text-zinc-400">
          NEW
        </span>
        <TypeBadge type={type} />
        {priority ? <PriorityDot priority={priority as Priority} /> : null}
        <input
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
          placeholder="Issue title"
          aria-label="Title"
          autoFocus
          className="min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-1.5 py-0.5 text-base font-semibold text-zinc-100 placeholder:text-zinc-600 hover:border-ctl focus:border-accent focus:outline-none"
        />
        {create.isPending ? <span className="text-[11.5px] text-dim">Creating…</span> : null}
      </header>

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col gap-[22px] overflow-y-auto px-6 pb-8 pt-5">
          <section className="flex flex-wrap items-end gap-3">
            <FieldRow label="Type">
              <SelectField
                ariaLabel="Type"
                value={type}
                options={TYPE_OPTIONS}
                onChange={(value) => setType(value as IssueType)}
              />
            </FieldRow>
            <FieldRow label="Priority">
              <SelectField
                ariaLabel="Priority"
                value={priority}
                options={PRIORITY_OPTIONS.map((value) => ({ value, label: value }))}
                onChange={setPriority}
              />
            </FieldRow>
            <FieldRow label="Status">
              <SelectField
                ariaLabel="Status"
                value={status}
                disabled={schema.isPending}
                options={
                  schema.data?.workflow.statuses.map((s) => ({ value: s.id, label: s.label })) ?? []
                }
                onChange={setStatus}
              />
            </FieldRow>
            <FieldRow label="Labels">
              <input
                value={labels}
                onChange={(event) => setLabels(event.target.value)}
                placeholder="area:auth, team:core"
                aria-label="Labels"
                className={cn(INPUT_CLASS, "w-[200px]")}
              />
            </FieldRow>
          </section>

          <section>
            <div className="mb-3 flex items-center gap-2">
              <SectionHeading size="sm">Description</SectionHeading>
              <EditorModeToggle mode={mode} onChange={setMode} showPreview={false} />
            </div>
            <div className="min-h-72">
              <Suspense fallback={<Loading label="Loading editor…" />}>
                {mode === "rich" ? (
                  <RichEditor
                    value={body}
                    onChange={setBody}
                    onSave={submit}
                    onFallbackToSource={() => setMode("source")}
                    className="h-full min-h-72 rounded-md border border-edge bg-card/60 p-2"
                  />
                ) : (
                  <CodeMirrorEditor value={body} onChange={setBody} onSave={submit} />
                )}
              </Suspense>
            </div>
          </section>

          <section className="mt-auto flex items-center gap-3 border-t border-edge pt-4">
            <button
              type="button"
              onClick={() => submit()}
              disabled={title.trim().length === 0 || create.isPending}
              className={BUTTON_PRIMARY}
            >
              {create.isPending ? "Creating…" : "Create issue"}
            </button>
            <span className="text-[11.5px] text-dim">⌘Enter creates · nothing is committed until then</span>
            {validation ? <p className="text-xs text-red-400">{validation}</p> : null}
            {create.isError ? (
              <p className="text-xs text-red-400">
                {create.error instanceof Error
                  ? create.error.message
                  : "Could not create the issue"}
              </p>
            ) : null}
          </section>
        </div>
      </div>
    </div>
  );
}
