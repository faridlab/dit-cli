// Issue body editing: a rich editor (lazy chunk, TipTap + the Rust bridge
// in WASM) that is always on — there is no edit/done mode and no save
// button. The rich editor serializes through Rust, so its saves are
// byte-identical to `dit fmt`; source mode is CodeMirror for the bytes the
// rich editor cannot own. One commit per typing pause, like the doc editor.

import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { usePutIssueBody } from "../lib/queries";
import { EditorModeToggle, type EditorMode } from "./EditorModeToggle";
import { Loading } from "./states";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

// One commit per typing pause: quiet enough that writing never waits on a
// round trip, soon enough that "did I save?" is never a question. The rich
// editor already serializes on its own ~300ms pause, so the commit lands
// roughly 1.8s after the last keystroke. Mirrors the doc editor's constant.
const AUTOSAVE_DELAY_MS = 1500;

export function BodyEditor({ issueId, body }: { issueId: string; body: string }) {
  const [text, setText] = useState(body);
  const [mode, setMode] = useState<EditorMode>("rich");
  const save = usePutIssueBody(issueId);

  // The body we last saw from the server, and the bytes we last sent — the
  // two anchors that decide whether a fresh `body` prop replaces the buffer.
  const prevBody = useRef(body);
  const sent = useRef<string | null>(null);

  // A fresh body arrives after our own save (canonical bytes) or a live
  // refresh (someone else wrote). Adopt it when the buffer holds nothing
  // newer: exactly what was sent, or the previous body untouched. A buffer
  // with local edits stays — the next autosave is the last writer, the same
  // rule the doc editor uses.
  useEffect(() => {
    if (body === prevBody.current) return;
    const buffered = text === sent.current || text === prevBody.current;
    prevBody.current = body;
    if (buffered) {
      sent.current = null;
      setText(body);
    }
  }, [body, text]);

  const saveNow = useCallback(
    (markdown?: string) => {
      const next = markdown ?? text;
      if (next === body) return;
      sent.current = next;
      setText(next);
      save.mutate(next);
    },
    [body, save, text],
  );

  useEffect(() => {
    if (text === body) return;
    const timer = window.setTimeout(() => saveNow(), AUTOSAVE_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [text, body, saveNow]);

  const dirty = text !== body;

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center gap-2">
        <EditorModeToggle mode={mode} onChange={setMode} showPreview={false} />
        <span className="text-[11px] text-zinc-500">
          {save.isPending ? "Saving…" : dirty ? "Unsaved changes" : ""}
        </span>
      </div>
      <div className="min-h-72">
        <Suspense fallback={<Loading label="Loading editor…" />}>
          {mode === "rich" ? (
            <RichEditor
              value={text}
              onChange={setText}
              onSave={saveNow}
              onFallbackToSource={() => setMode("source")}
              className="h-full min-h-72 rounded-md border border-edge bg-card/60 p-2"
            />
          ) : (
            <CodeMirrorEditor value={text} onChange={setText} onSave={saveNow} />
          )}
        </Suspense>
      </div>
    </div>
  );
}
