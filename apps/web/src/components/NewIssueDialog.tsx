// New issue dialog. Deliberately small: title plus the few fields worth
// setting at creation — everything else is edited on the issue itself once
// it exists. Success closes the dialog and jumps straight to the issue.

import { type FormEvent, useEffect, useState } from "react";
import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import { SelectField } from "./SelectField";
import { useCreateIssue, useSchema } from "../lib/queries";
import type { IssueDto, IssueType, Priority } from "../lib/types";
import { parseCsvList } from "../lib/format";
import { cn } from "../lib/cn";

const TYPE_OPTIONS: Array<{ value: IssueType; label: string }> = [
  { value: "task", label: "task" },
  { value: "bug", label: "bug" },
  { value: "story", label: "story" },
  { value: "spike", label: "spike" },
  { value: "chore", label: "chore" },
];

const PRIORITY_OPTIONS: Priority[] = ["p0", "p1", "p2", "p3", "p4"];

const INPUT_CLASS =
  "h-8 w-full rounded border border-zinc-700 bg-zinc-950 px-2 text-xs text-zinc-200 placeholder:text-zinc-600 focus:border-sky-600 focus:outline-none";

function Label({ children }: { children: React.ReactNode }) {
  return (
    <span className="text-[10px] font-medium uppercase tracking-wide text-zinc-500">
      {children}
    </span>
  );
}

export function NewIssueDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: (issue: IssueDto) => void;
}) {
  const schema = useSchema();
  const create = useCreateIssue();

  const [title, setTitle] = useState("");
  const [type, setType] = useState<IssueType>("task");
  const [priority, setPriority] = useState<string>("");
  const [status, setStatus] = useState("");
  const [labels, setLabels] = useState("");
  const [body, setBody] = useState("");
  const [validation, setValidation] = useState<string | null>(null);

  // Reset every time the dialog opens; keep the workflow's first status as
  // the default once the schema is in.
  useEffect(() => {
    if (open) {
      setTitle("");
      setType("task");
      setPriority("");
      setLabels("");
      setBody("");
      setValidation(null);
    }
  }, [open]);

  useEffect(() => {
    if (status === "" && schema.data) {
      const first = schema.data.workflow.statuses[0];
      if (first) setStatus(first.id);
    }
  }, [schema.data, status]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const trimmedTitle = title.trim();
    if (trimmedTitle.length === 0) {
      setValidation("A title is required.");
      return;
    }
    setValidation(null);
    create.mutate(
      {
        title: trimmedTitle,
        type,
        ...(priority ? { priority } : {}),
        ...(status ? { status } : {}),
        labels: parseCsvList(labels),
        body,
      },
      {
        onSuccess: (issue) => {
          onOpenChange(false);
          onCreated(issue);
        },
      },
    );
  };

  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-black/60" />
        <DialogPrimitive.Content
          className="fixed left-1/2 top-24 z-50 w-[520px] max-w-[92vw] -translate-x-1/2 rounded-lg border border-zinc-700 bg-zinc-900 p-4 shadow-2xl"
        >
          <div className="mb-3 flex items-center justify-between">
            <DialogPrimitive.Title className="text-sm font-semibold text-zinc-100">
              New issue
            </DialogPrimitive.Title>
            <DialogPrimitive.Close
              className="rounded p-1 text-zinc-500 hover:bg-zinc-800 hover:text-zinc-300"
              aria-label="Close"
            >
              <X className="size-4" aria-hidden />
            </DialogPrimitive.Close>
          </div>

          <form onSubmit={submit} className="flex flex-col gap-3">
            <label className="flex flex-col gap-1">
              <Label>Title</Label>
              <input
                value={title}
                onChange={(event) => setTitle(event.target.value)}
                placeholder="Short, specific, actionable"
                autoFocus
                className={INPUT_CLASS}
              />
            </label>

            <div className="grid grid-cols-3 gap-2">
              <div className="flex flex-col gap-1">
                <Label>Type</Label>
                <SelectField
                  ariaLabel="Type"
                  value={type}
                  options={TYPE_OPTIONS}
                  onChange={(value) => setType(value as IssueType)}
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label>Priority</Label>
                <SelectField
                  ariaLabel="Priority"
                  value={priority}
                  options={PRIORITY_OPTIONS.map((value) => ({ value, label: value }))}
                  onChange={setPriority}
                />
              </div>
              <div className="flex flex-col gap-1">
                <Label>Status</Label>
                <SelectField
                  ariaLabel="Status"
                  value={status}
                  disabled={schema.isPending}
                  options={
                    schema.data?.workflow.statuses.map((s) => ({ value: s.id, label: s.label })) ?? []
                  }
                  onChange={setStatus}
                />
              </div>
            </div>

            <label className="flex flex-col gap-1">
              <Label>Labels (comma separated)</Label>
              <input
                value={labels}
                onChange={(event) => setLabels(event.target.value)}
                placeholder="area:auth, team:core"
                className={INPUT_CLASS}
              />
            </label>

            <label className="flex flex-col gap-1">
              <Label>Description (markdown)</Label>
              <textarea
                value={body}
                onChange={(event) => setBody(event.target.value)}
                rows={5}
                placeholder="What is going on, what was expected, how to reproduce…"
                className={cn(INPUT_CLASS, "h-auto resize-y py-1.5 font-mono")}
              />
            </label>

            {validation ? <p className="text-xs text-red-400">{validation}</p> : null}
            {create.isError ? (
              <p className="text-xs text-red-400">
                {create.error instanceof Error ? create.error.message : "Could not create the issue"}
              </p>
            ) : null}

            <div className="mt-1 flex justify-end gap-2">
              <button
                type="button"
                onClick={() => onOpenChange(false)}
                className="rounded border border-zinc-700 px-3 py-1.5 text-xs text-zinc-400 hover:border-zinc-500 hover:text-zinc-200"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={create.isPending}
                className="rounded bg-sky-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-sky-600 disabled:bg-zinc-800 disabled:text-zinc-500"
              >
                {create.isPending ? "Creating…" : "Create issue"}
              </button>
            </div>
          </form>
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
