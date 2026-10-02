// The Notion-like editor. TipTap renders and edits; the Rust bridge in the
// tab (via WASM) is the only thing that turns the document into markdown —
// so a save here is byte-identical to `dit fmt` on the same content, and
// reopening a saved document never produces a diff (DESIGN.md §12.2).
//
// Prop contract matches CodeMirrorEditor, plus one addition: `onSave` may
// receive the serialized markdown as its first argument. The serialization
// is async (WASM), so Mod+Enter flushes it and hands the exact bytes to the
// parent rather than racing a setState.

import { useEffect, useRef, useState } from "react";
import { EditorContent, useEditor, type Editor } from "@tiptap/react";

import { toast } from "sonner";

import { docToMarkdown, markdownToDoc, type PmDoc } from "./bridge";
import { uploadAttachment } from "../lib/api";
import { altFrom, imageFiles, resolveAttachmentSrc, type AttachContext } from "../lib/attachments";
import { useDocs, useIssues } from "../lib/queries";
import { navigate } from "../lib/router";
import { resolveWikiTarget, wikiItemsFrom, type WikiItem } from "../lib/wikilinks";
import { ditExtensions } from "./extensions";
import { BubbleToolbar } from "./BubbleToolbar";
import { BlockHandle } from "./BlockHandle";
import { TableToolbar } from "./TableToolbar";
import { InsertDialog } from "./InsertDialog";
import { Loading } from "../components/states";
import "./editor.css";

// Typing bursts serialize once per pause, not once per keystroke.
const SERIALIZE_DEBOUNCE_MS = 300;

/** Commit each picture beside its page or issue (ADR 0026), then put its
 *  relative link where it was pasted or dropped. */
export async function uploadImagesInto(
  editor: Editor,
  attach: AttachContext,
  files: File[],
  at?: number,
): Promise<void> {
  for (const file of files) {
    try {
      const done = await uploadAttachment(attach.target, file, file.name || "image.png");
      if (editor.isDestroyed) return;
      const alt = altFrom(file.name);
      const node = { type: "image", attrs: { src: done.link, title: "" }, content: alt ? [{ type: "text", text: alt }] : [] };
      const chain = editor.chain().focus();
      (at === undefined ? chain.insertContent(node) : chain.insertContentAt(at, node)).run();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "The picture could not be added");
    }
  }
}

export default function RichEditor({
  value,
  onChange,
  onSave,
  onFallbackToSource,
  className,
  compact = false,
  placeholder,
  flushRef,
  attach,
}: {
  value: string;
  onChange: (markdown: string) => void;
  /** Called with the just-serialized markdown (parents may ignore the arg). */
  onSave: (markdown?: string) => void;
  /** The bridge refused this document (conflict markers, wasm failure); the
   *  parent should switch to source mode. The reason is in our own banner. */
  onFallbackToSource?: () => void;
  className?: string;
  /** A small editor (a comment box): no block handle beside each line. */
  compact?: boolean;
  /** Replaces the default "type '/' for commands" hint on an empty editor. */
  placeholder?: string;
  /** Set to a function that serializes what is on screen right now — for a
   *  button outside the editor that must act on the last keystrokes, which
   *  the debounced `onChange` may not have delivered yet. */
  flushRef?: { current: (() => Promise<string | null>) | null };
  /** Where pasted and dropped pictures are committed, and the folder their
   *  relative links resolve against (ADR 0026). Without it (an issue not
   *  created yet) pictures cannot be added. */
  attach?: AttachContext;
}) {
  const attachRef = useRef(attach);
  attachRef.current = attach;

  // What "[[" offers, and where a followed link goes. Read through refs: the
  // lists arrive after the editor is built, and rebuilding it would lose
  // the caret.
  const docs = useDocs();
  const issues = useIssues({ limit: 500 });
  const wikiItems = useRef<readonly WikiItem[]>([]);
  wikiItems.current = wikiItemsFrom(docs.data ?? [], issues.data?.items ?? []);
  const docList = useRef(docs.data ?? []);
  docList.current = docs.data ?? [];
  const openWikiLink = (target: string) => {
    const route = resolveWikiTarget(target, docList.current);
    if (route) navigate(route);
    else toast(`No single page or issue is called “${target}”`);
  };
  const takePictures = (files: File[], at?: number): boolean => {
    if (files.length === 0) return false;
    const context = attachRef.current;
    const target = editorRef.current;
    if (!context || !target) {
      toast("Pictures can be added once this is saved");
      return true;
    }
    void uploadImagesInto(target, context, files, at);
    return true;
  };
  const [initialDoc, setInitialDoc] = useState<PmDoc | null>(null);
  const [bridgeError, setBridgeError] = useState<string | null>(null);

  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSaveRef = useRef(onSave);
  onSaveRef.current = onSave;
  // Emit a completed serialization to the parent, or keep quiet on failure
  // (the error path already surfaced a banner).
  const emit = (markdown: string | null) => {
    if (markdown !== null) onChangeRef.current(markdown);
  };

  // What this editor last emitted — the anti-fight rule: an external value
  // that matches it is already in the document, and re-setting it would
  // clobber the cursor mid-typing.
  const lastEmitted = useRef<string | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const editorRef = useRef<Editor | null>(null);
  // The document as of the last keystroke. Unmount cleanup runs after
  // TipTap has destroyed the editor, so the flush must serialize from this
  // snapshot instead of asking a dead editor for its JSON.
  const pendingJson = useRef<PmDoc | null>(null);

  // markdown -> PM doc, once per mount and per genuine external change.
  useEffect(() => {
    let alive = true;
    void (async () => {
      const doc = await markdownToDoc(value);
      if (!alive) return;
      if (doc.ok) {
        setBridgeError(null);
        setInitialDoc(doc.value);
      } else {
        setBridgeError(doc.error);
        onFallbackToSource?.();
      }
    })();
    return () => {
      alive = false;
    };
    // Deliberately not [value]: external changes sync through the effect
    // below, against lastEmitted, not by rebuilding the editor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const serializeDoc = async (doc: PmDoc): Promise<string | null> => {
    const result = await docToMarkdown(doc);
    if (result.ok) {
      lastEmitted.current = result.value;
      return result.value;
    }
    setBridgeError(result.error);
    return null;
  };

  const serialize = (): Promise<string | null> => {
    const editor = editorRef.current;
    if (!editor || editor.isDestroyed) return Promise.resolve(null);
    return serializeDoc(editor.getJSON() as PmDoc);
  };

  const editor = useEditor(
    {
      content: initialDoc ?? { type: "doc", content: [] },
      extensions: ditExtensions({
        placeholder,
        resolveImageSrc: attach ? (src) => resolveAttachmentSrc(src, attach.baseDir) : undefined,
        wikiItems: () => wikiItems.current,
        openWikiLink,
      }),
      editorProps: {
        attributes: { class: "dit-rich", spellcheck: "true" },
        handleKeyDown: (_view, event) => {
          if ((event.metaKey || event.ctrlKey) && (event.key === "Enter" || event.key.toLowerCase() === "s")) {
            event.preventDefault();
            // Flush the pending serialization, then save those exact bytes —
            // the parent's state may not have the last keystrokes yet.
            if (timer.current !== null) clearTimeout(timer.current);
            void (async () => {
              const markdown = await serialize();
              if (markdown !== null) {
                onChangeRef.current(markdown);
                onSaveRef.current(markdown);
              }
            })();
            return true;
          }
          return false;
        },
        handleDrop: (view, event, _slice, moved) => {
          if (moved) return false;
          const files = imageFiles(event.dataTransfer?.files);
          if (files.length === 0) return false;
          event.preventDefault();
          const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
          return takePictures(files, at);
        },
        handlePaste: (_view, event) => {
          const data = event.clipboardData;
          if (!data) return false;
          // A screenshot on the clipboard arrives as a file: commit it.
          const pictures = imageFiles(data.files);
          if (pictures.length > 0) {
            event.preventDefault();
            return takePictures(pictures);
          }
          // Rich clipboard goes through ProseMirror's HTML parsing; plain
          // text is markdown here, so it goes through the Rust bridge.
          if (data.getData("text/html")) return false;
          const text = data.getData("text/plain");
          if (!text) return false;
          event.preventDefault();
          void (async () => {
            const doc = await markdownToDoc(text);
            if (doc.ok) {
              editorRef.current?.commands.insertContent(doc.value.content ?? []);
            } else {
              // Fall back honestly: lines joined by soft breaks.
              const nodes: Array<Record<string, unknown>> = [];
              text.split("\n").forEach((line, index) => {
                if (index > 0) nodes.push({ type: "hardBreak", attrs: { soft: true } });
                if (line) nodes.push({ type: "text", text: line });
              });
              editorRef.current?.commands.insertContent(nodes);
            }
          })();
          return true;
        },
      },
      onUpdate: ({ editor }) => {
        pendingJson.current = editor.getJSON() as PmDoc;
        if (timer.current !== null) clearTimeout(timer.current);
        timer.current = setTimeout(() => {
          timer.current = null;
          void serialize().then(emit);
        }, SERIALIZE_DEBOUNCE_MS);
      },
      onBlur: () => {
        if (timer.current !== null) {
          clearTimeout(timer.current);
          timer.current = null;
        }
        void serialize().then(emit);
      },
    },
    // Rebuild the editor only when the parsed initial document arrives.
    [initialDoc],
  );

  editorRef.current = editor;
  if (flushRef) {
    flushRef.current = async () => {
      if (timer.current !== null) {
        clearTimeout(timer.current);
        timer.current = null;
      }
      const markdown = await serialize();
      emit(markdown);
      return markdown;
    };
  }

  // External value changes (a different issue, a server refresh) sync into
  // the document — but never the ones we emitted ourselves.
  useEffect(() => {
    if (!editor || value === lastEmitted.current) return;
    void (async () => {
      const doc = await markdownToDoc(value);
      // The parse is a round trip through WASM, and the editor can be gone
      // by the time it lands — closing the issue panel, or stepping to the
      // next issue with J. Touching a destroyed editor throws inside TipTap.
      if (editor.isDestroyed) return;
      if (!doc.ok) {
        setBridgeError(doc.error);
        return;
      }
      setBridgeError(null);
      editor.commands.setContent(doc.value, { emitUpdate: false });
    })();
  }, [editor, value]);

  // Unmount with a pending serialization: flush it from the snapshot so the
  // last words typed are not lost. (Not from the editor — this cleanup runs
  // after TipTap's own, which has already destroyed it.)
  useEffect(
    () => () => {
      if (timer.current === null) return;
      clearTimeout(timer.current);
      timer.current = null;
      const doc = pendingJson.current;
      if (doc) void serializeDoc(doc).then(emit);
    },
    // Once is the intent: re-creating this cleanup per render would flush
    // mid-session.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  if (bridgeError) {
    return (
      <div className="dit-rich-error">
        <p className="dit-rich-error-title">This document can only be edited as source.</p>
        <p className="dit-rich-error-detail">{bridgeError}</p>
      </div>
    );
  }

  // Until the parsed document arrives there is nothing to show — an empty
  // editor would flash the placeholder over the real content.
  if (!editor || initialDoc === null) {
    return <Loading label="Loading editor…" />;
  }

  return (
    <div className={className ?? "h-full"}>
      <EditorContent editor={editor} />
      {compact ? null : <BlockHandle editor={editor} />}
      <BubbleToolbar editor={editor} />
      <TableToolbar editor={editor} />
      <InsertDialog editor={editor} attach={attach} />
    </div>
  );
}
