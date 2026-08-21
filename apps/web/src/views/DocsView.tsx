// The §13 document layer in the browser: plain Markdown pages under the
// four doc roots, served by /api/docs (ADR 0010). Pages open in VS
// Code-style tabs — single click from the pane previews, double click pins
// — each tab carries its own editing buffer, and the editor is always on:
// there is no edit/done mode. Saves happen on their own, one commit per
// typing pause; Mod+Enter commits immediately. The file tree is the source
// of truth: a page's history is git, and every save is one commit through
// the same write path issues use.

import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { FileText, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { queryKeys, useDeleteDoc, useDoc, useDocs, usePutDoc } from "../lib/queries";
import { relativeTime } from "../lib/format";
import type { DocTabs } from "../lib/doctabs";
import type { DocBodyDto } from "../lib/types";
import { EditorModeToggle, type EditorMode } from "../components/EditorModeToggle";
import { Empty, ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

// One commit per typing pause: quiet enough that writing never waits on a
// round trip, soon enough that "did I save?" is never a question. The rich
// editor already serializes on its own ~300ms pause, so the commit lands
// roughly 1.8s after the last keystroke.
const AUTOSAVE_DELAY_MS = 1500;

function formatBytes(bytes: number): string {
  return bytes < 1024 ? `${bytes} B` : `${(bytes / 1024).toFixed(1)} KB`;
}

function TabBar({
  tabs,
  active,
  onActivate,
  onPin,
  onClose,
}: {
  tabs: DocTabs;
  active: string | null;
  onActivate: (path: string) => void;
  onPin: (path: string) => void;
  onClose: (path: string) => void;
}) {
  if (tabs.paths.length === 0) return null;
  return (
    <div
      role="tablist"
      aria-label="Open pages"
      className="flex h-[34px] shrink-0 items-stretch overflow-x-auto border-b border-edge bg-panel/50"
    >
      {tabs.paths.map((path) => {
        const isActive = path === active;
        const pinned = tabs.pinned.has(path);
        const dirty = tabs.isDirty(path);
        return (
          <div
            key={path}
            role="tab"
            aria-selected={isActive}
            onDoubleClick={() => onPin(path)}
            // Middle-click closes — the reflex every tabbed UI teaches.
            onAuxClick={(event) => {
              if (event.button === 1) {
                event.preventDefault();
                onClose(path);
              }
            }}
            title={path}
            className={cn(
              "group relative flex min-w-0 max-w-[200px] shrink-0 cursor-pointer select-none items-center gap-1.5 border-r border-edge px-2.5",
              isActive
                ? "bg-app text-zinc-100"
                : "text-zinc-400 hover:bg-card/60 hover:text-zinc-200",
            )}
          >
            {isActive ? (
              <span className="absolute inset-x-0 top-0 h-[1.5px] bg-accent" aria-hidden />
            ) : null}
            <FileText
              className={cn("size-3.5 shrink-0", pinned ? "text-zinc-400" : "text-zinc-500")}
              aria-hidden
            />
            <button
              type="button"
              onClick={() => onActivate(path)}
              className={cn(
                "truncate py-0 font-mono text-[11.5px]",
                !pinned && isActive && "italic",
              )}
            >
              {path.split("/").pop() ?? path}
            </button>
            {dirty ? (
              // A dot, not a times sign: there is something to lose — though
              // with autosave it clears itself within seconds.
              <span className="size-1.5 shrink-0 rounded-full bg-accent" aria-label="unsaved" />
            ) : (
              <button
                type="button"
                onClick={(event) => {
                  event.stopPropagation();
                  onClose(path);
                }}
                title="Close tab"
                className="flex size-4 shrink-0 items-center justify-center rounded text-zinc-500 hover:bg-edge hover:text-zinc-100"
              >
                <X className="size-3" aria-hidden />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

export function DocsView({
  p,
  onSelect,
  tabs,
  onCloseTab,
}: {
  p: string | null;
  onSelect: (path: string | null) => void;
  /** Open tabs, pins and per-path buffers — owned above the view so they
   *  outlive navigation between views. */
  tabs: DocTabs;
  onCloseTab: (path: string) => void;
}) {
  const client = useQueryClient();
  const docs = useDocs();
  const doc = useDoc(p);
  const put = usePutDoc();
  const remove = useDeleteDoc();
  const [editMode, setEditMode] = useState<EditorMode>("rich");

  // Rich by default, source as the escape hatch — one mode for the view,
  // remembered across tabs within a visit.
  const draft = p === null ? undefined : tabs.drafts[p];

  // The buffer materializes once, when the page's content first arrives;
  // afterwards only editing (or a save landing) touches it.
  useEffect(() => {
    if (p === null || doc.data === undefined || draft !== undefined) return;
    tabs.initDraft(p, doc.data.body);
  }, [p, doc.data, draft, tabs]);

  // The single save path. The canonical body the server sends back is
  // adopted only if the buffer still is what was sent — keystrokes typed
  // during the round trip stay ahead, and the next autosave carries them.
  const save = useCallback(
    (path: string, body: string) => {
      put.mutate(
        { path, body },
        {
          onSuccess: (saved) => {
            tabs.syncIfUnchanged(saved.path, body, saved.body);
          },
        },
      );
    },
    [put, tabs],
  );

  // Autosave: every buffer that differs from its cached saved body commits
  // once its owner pauses. Any keystroke (the drafts object changes)
  // restarts the pause; a save landing re-runs this and finds nothing due.
  useEffect(() => {
    const due = Object.entries(tabs.drafts).filter(([path, body]) => {
      const saved = client.getQueryData<DocBodyDto>(queryKeys.doc(path))?.body;
      return saved !== undefined && body !== saved;
    });
    if (due.length === 0) return;
    const timer = window.setTimeout(() => {
      for (const [path, body] of due) save(path, body);
    }, AUTOSAVE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [tabs.drafts, client, save]);

  // Mod+Enter / Mod+S from the editor: commit the exact bytes the editor
  // just serialized, without waiting out the pause.
  const saveNow = (markdown?: string) => {
    if (p === null || put.isPending) return;
    const body = markdown ?? draft;
    if (body === undefined) return;
    const saved = client.getQueryData<DocBodyDto>(queryKeys.doc(p))?.body;
    if (saved !== undefined && body === saved) return;
    save(p, body);
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
        onCloseTab(p);
      },
    });
  };

  return (
    <div className="flex min-w-0 flex-1 flex-col">
      <TabBar tabs={tabs} active={p} onActivate={onSelect} onPin={tabs.pin} onClose={onCloseTab} />

      {p === null ? (
        <div className="flex flex-1 items-center justify-center">
          <Empty
            title="No page open"
            hint="Pick a page from the side pane — single click previews it, double click pins it as a tab. Every page is a Markdown file in the repo."
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
            <span className="ml-auto flex shrink-0 items-center gap-2">
              <span className="text-[11px] text-zinc-500">
                {put.isPending ? "Saving…" : draft !== undefined && draft !== doc.data.body ? "Unsaved" : ""}
              </span>
              <EditorModeToggle mode={editMode} onChange={setEditMode} showPreview={false} />
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
            </span>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto">
            {draft === undefined ? (
              <Loading label="Preparing editor…" />
            ) : (
              <div className="mx-auto h-full w-full max-w-[860px] px-6 py-4">
                <Suspense fallback={<Loading label="Loading editor…" />}>
                  {editMode === "rich" ? (
                    <RichEditor
                      key={p}
                      value={draft}
                      onChange={(next) => tabs.setDraft(p, next)}
                      onSave={saveNow}
                      onFallbackToSource={() => setEditMode("source")}
                      className="h-full"
                    />
                  ) : (
                    <CodeMirrorEditor
                      key={p}
                      value={draft}
                      onChange={(next) => tabs.setDraft(p, next)}
                      onSave={saveNow}
                    />
                  )}
                </Suspense>
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
