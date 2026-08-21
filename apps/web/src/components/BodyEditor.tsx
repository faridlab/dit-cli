// Issue body editing: a rich editor (lazy chunk, TipTap + the Rust bridge
// in WASM) with source and preview modes. The rich editor serializes through
// Rust, so its saves are byte-identical to `dit fmt`; source mode is
// CodeMirror for the bytes the rich editor cannot own. The preview always
// goes through the server — there is no client-side markdown renderer in
// this app, on purpose.

import { lazy, Suspense, useEffect, useState } from "react";
import { Save } from "lucide-react";
import { useMarkdownPreview, usePutIssueBody } from "../lib/queries";
import { EditorModeToggle, type EditorMode } from "./EditorModeToggle";
import { Markdown } from "./Markdown";
import { Loading } from "./states";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

// Typing bursts shouldn't become one render request per keystroke: hold the
// text briefly so the preview renders pauses, not characters.
const PREVIEW_DEBOUNCE_MS = 250;

function useDebounced<T>(value: T, delay: number): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delay);
    return () => clearTimeout(timer);
  }, [value, delay]);
  return settled;
}

export function BodyEditor({ issueId, body }: { issueId: string; body: string }) {
  const [text, setText] = useState(body);
  const [mode, setMode] = useState<EditorMode>("rich");
  const save = usePutIssueBody(issueId);
  const settledText = useDebounced(text, PREVIEW_DEBOUNCE_MS);

  // Preview renders the working text, not the saved issue, so people can
  // check formatting before committing the edit.
  const previewQuery = useMarkdownPreview(settledText, mode === "preview" && settledText.trim().length > 0);

  const dirty = text !== body;
  // The rich editor serializes asynchronously, so it hands the exact bytes it
  // just produced to this save — no racing a debounced setState.
  const saveNow = (markdown?: string) => {
    const next = markdown ?? text;
    if (next !== body && !save.isPending) {
      setText(next);
      save.mutate(next);
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <EditorModeToggle mode={mode} onChange={setMode} showPreview />
        <span className="text-[11px] text-zinc-500">
          {save.isPending ? "Saving…" : dirty ? "Unsaved changes" : ""}
        </span>
        <button
          type="button"
          onClick={() => saveNow()}
          disabled={!dirty || save.isPending}
          className="ml-auto flex items-center gap-1 rounded bg-sky-700 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-sky-600 disabled:cursor-default disabled:bg-zinc-800 disabled:text-zinc-500"
        >
          <Save className="size-3" aria-hidden />
          Save
        </button>
      </div>

      {mode === "preview" ? (
        previewQuery.isFetching ? (
          <Loading label="Rendering…" />
        ) : previewQuery.data ? (
          <Markdown
            html={previewQuery.data.html}
            className="min-h-72 rounded-md border border-zinc-800 bg-zinc-950 p-3"
          />
        ) : (
          <p className="text-xs text-zinc-500">Nothing to preview yet.</p>
        )
      ) : (
        <div className="min-h-72">
          <Suspense fallback={<Loading label="Loading editor…" />}>
            {mode === "rich" ? (
              <RichEditor
                value={text}
                onChange={setText}
                onSave={saveNow}
                onFallbackToSource={() => setMode("source")}
                className="h-full min-h-72 rounded-md border border-zinc-800 bg-zinc-950 p-2"
              />
            ) : (
              <CodeMirrorEditor value={text} onChange={setText} onSave={saveNow} />
            )}
          </Suspense>
        </div>
      )}
      <p className="text-[11px] text-zinc-600">
        Markdown · Mod+Enter saves · preview is rendered by the server
      </p>
    </div>
  );
}
