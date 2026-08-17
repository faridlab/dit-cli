// The markdown source editor. This module is deliberately its own chunk:
// the editor stack is the heaviest dependency in the UI and most visits
// never open it, so it must stay out of the entry bundle.

import { useEffect, useRef } from "react";
import { EditorView, basicSetup } from "codemirror";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";

// Dark, quiet, monospaced — matches the surrounding UI without pulling in
// a published theme package.
const theme = EditorView.theme({
  "&": {
    backgroundColor: "transparent",
    color: "#e4e4e7",
    fontSize: "13px",
    height: "100%",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
    lineHeight: "1.55",
  },
  ".cm-content": { caretColor: "#38bdf8", padding: "8px 0" },
  ".cm-gutters": { backgroundColor: "transparent", color: "#52525b", border: "none" },
  ".cm-activeLine": { backgroundColor: "rgba(63, 63, 70, 0.25)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "#a1a1aa" },
  ".cm-selectionBackground, ::selection": { backgroundColor: "rgba(56, 189, 248, 0.25)" },
  ".cm-cursor": { borderLeftColor: "#38bdf8" },
});

export default function CodeMirrorEditor({
  value,
  onChange,
  onSave,
}: {
  value: string;
  onChange: (value: string) => void;
  onSave: () => void;
}) {
  const hostRef = useRef<HTMLDivElement>(null);
  const viewRef = useRef<EditorView | null>(null);
  // Latest callbacks without re-creating the editor on every parent render.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  // Capture the initial text once: the editor owns the document from here.
  const initialValue = useRef(value);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const view = new EditorView({
      doc: initialValue.current,
      parent: host,
      extensions: [
        basicSetup,
        markdown({ base: markdownLanguage }),
        EditorView.lineWrapping,
        theme,
        EditorView.updateListener.of((update) => {
          if (update.docChanged) onChangeRef.current(update.state.doc.toString());
        }),
      ],
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  // External value changes (server refreshed the issue) sync into the
  // document — but never while it already matches, or typing would fight.
  useEffect(() => {
    const view = viewRef.current;
    if (!view) return;
    if (value !== view.state.doc.toString()) {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: value } });
    }
  }, [value]);

  return (
    <div
      ref={hostRef}
      // Mod+Enter saves from the keyboard; CodeMirror does not claim it.
      onKeyDown={(event) => {
        if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
          event.preventDefault();
          onSaveRef.current();
        }
      }}
      className="h-72 overflow-hidden rounded-md border border-zinc-700 bg-zinc-950 focus-within:border-sky-700"
    />
  );
}
