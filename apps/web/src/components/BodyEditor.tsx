// Issue body editing: markdown source in CodeMirror (lazy chunk) with a
// server-rendered preview. The preview always goes through the server —
// there is no client-side markdown renderer in this app, on purpose.

import { lazy, Suspense, useEffect, useState } from "react";
import { Eye, Pencil, Save } from "lucide-react";
import { useMarkdownPreview, usePutIssueBody } from "../lib/queries";
import { cn } from "../lib/cn";
import { Markdown } from "./Markdown";
import { Loading } from "./states";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));

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
  const [preview, setPreview] = useState(false);
  const save = usePutIssueBody(issueId);
  const settledText = useDebounced(text, PREVIEW_DEBOUNCE_MS);

  // Preview renders the working text, not the saved issue, so people can
  // check formatting before committing the edit.
  const previewQuery = useMarkdownPreview(settledText, preview && settledText.trim().length > 0);

  const dirty = text !== body;
  const saveNow = () => {
    if (dirty && !save.isPending) save.mutate(text);
  };

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <div className="flex overflow-hidden rounded-md border border-zinc-700">
          <button
            type="button"
            onClick={() => setPreview(false)}
            aria-pressed={!preview}
            className={cn(
              "flex items-center gap-1 px-2 py-1 text-[11px]",
              !preview ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300",
            )}
          >
            <Pencil className="size-3" aria-hidden />
            Edit
          </button>
          <button
            type="button"
            onClick={() => setPreview(true)}
            aria-pressed={preview}
            className={cn(
              "flex items-center gap-1 border-l border-zinc-700 px-2 py-1 text-[11px]",
              preview ? "bg-zinc-800 text-zinc-100" : "text-zinc-500 hover:text-zinc-300",
            )}
          >
            <Eye className="size-3" aria-hidden />
            Preview
          </button>
        </div>
        <span className="text-[11px] text-zinc-500">
          {save.isPending ? "Saving…" : dirty ? "Unsaved changes" : ""}
        </span>
        <button
          type="button"
          onClick={saveNow}
          disabled={!dirty || save.isPending}
          className="ml-auto flex items-center gap-1 rounded bg-sky-700 px-2.5 py-1 text-[11px] font-medium text-white hover:bg-sky-600 disabled:cursor-default disabled:bg-zinc-800 disabled:text-zinc-500"
        >
          <Save className="size-3" aria-hidden />
          Save
        </button>
      </div>

      {preview ? (
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
        <Suspense fallback={<Loading label="Loading editor…" />}>
          <CodeMirrorEditor value={text} onChange={setText} onSave={saveNow} />
        </Suspense>
      )}
      <p className="text-[11px] text-zinc-600">
        Markdown · Mod+Enter saves · preview is rendered by the server
      </p>
    </div>
  );
}
