// The §13 document layer in the browser: plain Markdown pages under the
// four doc roots, served by /api/docs (ADR 0010). The file tree is the
// source of truth — the listing is a walk, a page's history is git, and a
// save is one commit through the same write path issues use.

import { lazy, Suspense, useEffect, useState } from "react";
import { FileText, Pencil, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { useDeleteDoc, useDoc, useDocs, useMarkdownPreview, usePutDoc } from "../lib/queries";
import { relativeTime } from "../lib/format";
import { cn } from "../lib/cn";
import { Markdown } from "../components/Markdown";
import { BUTTON_OUTLINED, BUTTON_PRIMARY, SectionHeading } from "../components/chrome";
import { EditorModeToggle, type EditorMode } from "../components/EditorModeToggle";
import { Empty, ErrorBox, Loading } from "../components/states";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

const DOC_ROOTS = ["docs", "notes", "epics", "changelogs"] as const;

/** Client-side echo of the server's `DocPath` rules, so the create form can
 *  reject a bad path before the round trip. The server remains the
 *  authority — its 400 message lands in the same inline spot. */
function newPathProblem(input: string): string | null {
  const path = input.trim().replace(/\/+$/, "");
  if (path.length === 0) return "give the page a path, e.g. docs/meeting-notes.md";
  const segments = path.split("/");
  const root = segments[0] ?? "";
  if (!(DOC_ROOTS as readonly string[]).includes(root)) {
    return `the first segment must be one of: ${DOC_ROOTS.join(", ")}`;
  }
  for (const folder of segments.slice(0, -1)) {
    if (!/^[a-z0-9-]+$/.test(folder)) {
      return `"${folder}" must be lowercase letters, digits and dashes`;
    }
  }
  const name = segments[segments.length - 1] ?? "";
  if (!/^[a-z0-9][a-z0-9-_.]*\.md$/.test(name)) {
    return `"${name}" must be a .md name of lowercase letters, digits and dashes`;
  }
  return null;
}

function formatBytes(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

export function DocsView({
  p,
  onSelect,
}: {
  p: string | null;
  onSelect: (path: string | null) => void;
}) {
  const docs = useDocs();
  const doc = useDoc(p);
  const put = usePutDoc();
  const remove = useDeleteDoc();

  // Which page is open in the editor, if any. Keyed by path rather than a
  // boolean so navigating away from a page always closes its editor — and
  // creating a page can open the editor on the fresh path in one step.
  const [editingPath, setEditingPath] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [editMode, setEditMode] = useState<EditorMode>("rich");
  const [newPath, setNewPath] = useState("");
  const [newPathError, setNewPathError] = useState<string | null>(null);

  const editing = editingPath !== null && editingPath === p;

  useEffect(() => {
    if (editingPath !== null && editingPath !== p) setEditingPath(null);
  }, [editingPath, p]);

  const startEditing = () => {
    setDraft(doc.data?.body ?? "");
    setEditMode("rich");
    setEditingPath(p);
  };

  // The rich editor serializes asynchronously, so it hands the exact bytes it
  // just produced to this save — no racing a debounced setState.
  const saveNow = (markdown?: string) => {
    if (p === null || put.isPending) return;
    const next = markdown ?? draft;
    if (next === (doc.data?.body ?? "")) return;
    setDraft(next);
    put.mutate(
      { path: p, body: next },
      {
        onSuccess: (saved) => {
          setEditingPath(null);
          toast.success(`Saved ${saved.path}`);
        },
      },
    );
  };

  const create = () => {
    const path = newPath.trim().replace(/\/+$/, "");
    const problem = newPathProblem(path);
    if (problem !== null) {
      setNewPathError(problem);
      return;
    }
    setNewPathError(null);
    const title = (path.split("/").pop() ?? "untitled").replace(/\.md$/, "").replace(/[-_]+/g, " ");
    put.mutate(
      { path, body: `# ${title}\n\n` },
      {
        onSuccess: (saved) => {
          setNewPath("");
          onSelect(saved.path);
          setDraft(saved.body);
          setEditMode("rich");
          setEditingPath(saved.path);
        },
      },
    );
  };

  const removeSelected = () => {
    if (p === null) return;
    const confirmed = window.confirm(
      `Delete ${p}?\n\nThe page is removed in one commit — git history keeps every version, so nothing is lost permanently.`,
    );
    if (!confirmed) return;
    remove.mutate(p, {
      onSuccess: () => {
        toast.success(`Deleted ${p}`);
        onSelect(null);
      },
    });
  };

  const groups = DOC_ROOTS.map((root) => ({
    root,
    entries: (docs.data ?? []).filter((entry) => entry.path.startsWith(`${root}/`)),
  })).filter((group) => group.entries.length > 0);

  // The read pane renders the saved body through the server, exactly like
  // every other markdown surface in the app.
  const savedBody = doc.data?.body ?? "";
  const rendered = useMarkdownPreview(savedBody, p !== null && !editing && savedBody.trim().length > 0);

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-64 shrink-0 flex-col border-r border-edge bg-panel">
        <div className="flex h-[42px] shrink-0 items-center gap-2 border-b border-edge px-3">
          <FileText className="size-4 text-zinc-500" aria-hidden />
          <span className="text-[13px] font-medium text-zinc-200">Docs</span>
          <span className="ml-auto font-mono text-[10px] text-dim">
            {docs.data ? `${docs.data.length} pages` : "…"}
          </span>
        </div>

        <div className="flex-1 overflow-y-auto p-2">
          {docs.isPending ? (
            <Loading label="Loading pages…" className="p-4" />
          ) : docs.isError ? (
            <div className="p-2">
              <ErrorBox
                error={docs.error}
                onRetry={() => void docs.refetch()}
                title="Could not list pages"
              />
            </div>
          ) : groups.length === 0 ? (
            <p className="px-2 py-4 text-xs leading-relaxed text-zinc-600">
              No pages yet. Create one below — it lands as a plain Markdown file under{" "}
              <span className="font-mono">docs/</span>,{" "}
              <span className="font-mono">notes/</span>, <span className="font-mono">epics/</span> or{" "}
              <span className="font-mono">changelogs/</span>.
            </p>
          ) : (
            groups.map((group) => (
              <section key={group.root} className="mb-3">
                <SectionHeading size="sm" className="px-2 py-1.5">
                  {group.root}
                </SectionHeading>
                {group.entries.map((entry) => {
                  const active = entry.path === p;
                  return (
                    <button
                      key={entry.path}
                      type="button"
                      onClick={() => {
                        setEditingPath(null);
                        onSelect(entry.path);
                      }}
                      title={entry.path}
                      className={cn(
                        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left font-mono text-xs",
                        active
                          ? "bg-edge text-zinc-100"
                          : "text-zinc-400 hover:bg-card hover:text-zinc-200",
                      )}
                    >
                      <span className="truncate">{entry.path.slice(group.root.length + 1)}</span>
                      <span
                        className="ml-auto shrink-0 text-[10px] text-zinc-600"
                        title={new Date(entry.updated_ms).toISOString()}
                      >
                        {relativeTime(new Date(entry.updated_ms).toISOString())}
                      </span>
                    </button>
                  );
                })}
              </section>
            ))
          )}
        </div>

        <div className="shrink-0 border-t border-edge p-2">
          <div className="flex items-center gap-1.5">
            <input
              value={newPath}
              onChange={(event) => setNewPath(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") create();
              }}
              placeholder="docs/new-page.md"
              aria-label="New page path"
              className="h-[30px] min-w-0 flex-1 rounded-md border border-ctl bg-card px-2 font-mono text-xs text-zinc-200 outline-none transition-colors focus:border-accent placeholder:text-zinc-600"
            />
            <button
              type="button"
              onClick={create}
              disabled={put.isPending}
              title="Create page"
              className="flex size-[30px] shrink-0 items-center justify-center rounded-md border border-ctl text-zinc-400 transition-colors hover:border-zinc-400 hover:text-zinc-100 disabled:opacity-50"
            >
              <Plus className="size-4" aria-hidden />
            </button>
          </div>
          {newPathError ? <p className="mt-1.5 text-xs text-warn-text">{newPathError}</p> : null}
          {put.isError && newPathError === null ? (
            <p className="mt-1.5 text-xs text-warn-text">
              {put.error instanceof Error ? put.error.message : "Could not create page"}
            </p>
          ) : null}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        {p === null ? (
          <div className="flex flex-1 items-center justify-center">
            <Empty
              title="No page selected"
              hint="Pick a page from the left, or create one — every page is a Markdown file in the repo."
            />
          </div>
        ) : doc.isPending ? (
          <Loading label="Opening page…" />
        ) : doc.isError ? (
          <ErrorBox
            error={doc.error}
            onRetry={() => void doc.refetch()}
            title="Could not open page"
          />
        ) : doc.data === undefined ? null : (
          <>
            <div className="flex h-[42px] shrink-0 items-center gap-3 border-b border-edge px-4">
              <span className="truncate font-mono text-xs text-zinc-300">{p}</span>
              {(() => {
                const entry = (docs.data ?? []).find((candidate) => candidate.path === p);
                return entry ? (
                  <span className="shrink-0 font-mono text-[10px] text-zinc-600">
                    {formatBytes(entry.bytes)} · {relativeTime(new Date(entry.updated_ms).toISOString())}
                  </span>
                ) : null;
              })()}
              <span className="ml-auto flex shrink-0 items-center gap-1.5">
                {editing ? (
                  <>
                    <span className="text-[11px] text-zinc-500">
                      {put.isPending ? "Saving…" : draft === doc.data.body ? "" : "Unsaved changes"}
                    </span>
                    <button
                      type="button"
                      onClick={() => saveNow()}
                      disabled={put.isPending || draft === doc.data.body}
                      className={BUTTON_PRIMARY}
                    >
                      Save
                    </button>
                    <button
                      type="button"
                      onClick={() => setEditingPath(null)}
                      disabled={put.isPending}
                      className={BUTTON_OUTLINED}
                    >
                      Cancel
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      onClick={startEditing}
                      className="flex items-center gap-1.5 rounded-md border border-ctl px-2.5 py-1.5 text-xs text-zinc-400 transition-colors hover:border-zinc-400 hover:text-zinc-100"
                    >
                      <Pencil className="size-3.5" aria-hidden />
                      Edit
                    </button>
                    <button
                      type="button"
                      onClick={removeSelected}
                      disabled={remove.isPending}
                      title="Delete page"
                      className="flex items-center gap-1.5 rounded-md border border-transparent px-2.5 py-1.5 text-xs text-zinc-500 transition-colors hover:border-red-900/60 hover:bg-red-950/30 hover:text-red-300 disabled:opacity-50"
                    >
                      <Trash2 className="size-3.5" aria-hidden />
                      Delete
                    </button>
                  </>
                )}
              </span>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto">
              {editing ? (
                <div className="flex h-full flex-col">
                  <div className="flex shrink-0 items-center gap-3 border-b border-edge px-4 py-1.5">
                    <EditorModeToggle mode={editMode} onChange={setEditMode} showPreview={false} />
                    <p className="text-[11px] text-zinc-600">
                      Mod+Enter saves · one save is one commit, formatted by dit fmt
                    </p>
                  </div>
                  <div className="min-h-0 flex-1 overflow-y-auto">
                    <div className="mx-auto h-full w-full max-w-[860px] px-6 py-4">
                      <Suspense fallback={<Loading label="Loading editor…" />}>
                        {editMode === "rich" ? (
                          <RichEditor
                            value={draft}
                            onChange={setDraft}
                            onSave={saveNow}
                            onFallbackToSource={() => setEditMode("source")}
                            className="h-full"
                          />
                        ) : (
                          <CodeMirrorEditor value={draft} onChange={setDraft} onSave={saveNow} />
                        )}
                      </Suspense>
                    </div>
                  </div>
                </div>
              ) : rendered.data ? (
                <div className="mx-auto w-full max-w-[760px] px-8 py-6">
                  <Markdown html={rendered.data.html} />
                </div>
              ) : rendered.isError ? (
                <ErrorBox
                  error={rendered.error}
                  onRetry={() => void rendered.refetch()}
                  title="Could not render page"
                />
              ) : savedBody.trim().length === 0 ? (
                <Empty title="This page is empty" hint="Edit it to add content." />
              ) : (
                <Loading label="Rendering…" />
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
