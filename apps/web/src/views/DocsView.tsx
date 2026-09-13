// The §13 document layer in the browser: plain Markdown pages under the
// four doc roots, served by /api/docs (ADR 0010). Pages open in VS
// Code-style tabs — single click from the pane previews, double click pins
// — each tab carries its own editing buffer, and the editor is always on:
// there is no edit/done mode. Saves happen on their own, one commit per
// typing pause; ⌘S / Mod+Enter commit immediately. The file tree is the
// source of truth: a page's history is git, and every save is one commit
// through the same write path issues use.
//
// The markup follows the approved workbench design verbatim: a `.tabs`
// strip, then a scrolling `article.doc` with a `.path` row, the editor
// (`.md` rendered blocks or `.src` markdown) and a one-line hint.

import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  type CSSProperties,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Copy, FileText, Pen, X } from "lucide-react";
import { toast } from "sonner";
import { ApiError } from "../lib/api";
import { queryKeys, useDoc, usePutDoc } from "../lib/queries";
import { useViewOptions } from "../lib/viewopts";
import type { DocTabs } from "../lib/doctabs";
import type { DocBodyDto } from "../lib/types";
import { ContextMenuFor, type MenuItem } from "../components/chrome";
import { Empty, Loading } from "../components/states";
import { cn } from "../lib/cn";

const CodeMirrorEditor = lazy(() => import("../editor/CodeMirrorEditor"));
const RichEditor = lazy(() => import("../editor/RichEditor"));

// One commit per typing pause: quiet enough that writing never waits on a
// round trip, soon enough that "did I save?" is never a question. The rich
// editor already serializes on its own ~300ms pause, so the commit lands
// roughly 1.8s after the last keystroke.
const AUTOSAVE_DELAY_MS = 1500;

async function copyText(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(label);
  } catch {
    // Clipboard access needs a secure context; the toast still shows the
    // value so it can be copied by hand.
    toast(`${label}: ${text}`);
  }
}

function Tab({
  path,
  active,
  pinned,
  dirty,
  onActivate,
  onPin,
  onUnpin,
  onClose,
  onCloseOthers,
}: {
  path: string;
  active: boolean;
  pinned: boolean;
  dirty: boolean;
  onActivate: (path: string) => void;
  onPin: (path: string) => void;
  onUnpin: (path: string) => void;
  onClose: (path: string) => void;
  onCloseOthers: (path: string) => void;
}) {
  const items: MenuItem[] = [
    {
      label: pinned ? "Unpin" : "Pin",
      icon: <FileText className="i" aria-hidden />,
      run: () => (pinned ? onUnpin(path) : onPin(path)),
    },
    {
      label: "Close",
      icon: <X className="i" aria-hidden />,
      run: () => onClose(path),
    },
    {
      label: "Close others",
      icon: <X className="i" aria-hidden />,
      run: () => onCloseOthers(path),
    },
    {
      label: "Copy path",
      icon: <Copy className="i" aria-hidden />,
      run: () => void copyText(path, "Path copied"),
    },
  ];
  return (
    <ContextMenuFor items={items}>
      <div
        role="tab"
        tabIndex={0}
        aria-selected={active}
        title={pinned ? path : "preview tab — double-click to pin"}
        className={cn(
          "tab cursor-pointer select-none",
          active && "on",
          pinned && "pin",
        )}
        onClick={() => onActivate(path)}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onActivate(path);
          }
        }}
        onDoubleClick={() => {
          onPin(path);
          toast("Tab pinned");
        }}
        // Middle-click closes — the reflex every tabbed UI teaches.
        onAuxClick={(event) => {
          if (event.button === 1) {
            event.preventDefault();
            onClose(path);
          }
        }}
      >
        <FileText className="i" aria-hidden />
        {path.split("/").pop() ?? path}
        {dirty ? (
          // A dot, not a times sign: there is something to lose — though
          // with autosave it clears itself within seconds.
          <span
            className="dirty"
            title="unsaved — autosaves after a pause"
            aria-label="unsaved"
          />
        ) : (
          <button
            type="button"
            className="x"
            title="Close tab"
            onClick={(event) => {
              event.stopPropagation();
              onClose(path);
            }}
          >
            <X className="i" aria-hidden />
          </button>
        )}
      </div>
    </ContextMenuFor>
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
  const doc = useDoc(p);
  const put = usePutDoc();
  // The header's Source button flips this; the view only reads it.
  const { docSource, setDocSource } = useViewOptions();

  const draft = p === null ? undefined : tabs.drafts[p];

  // Paths the server answered 404 for: a page deleted from the tree, or a
  // deep link that never existed. Such a tab closes itself — there is
  // nothing to edit — and is never restored. The shell re-adds the active
  // tab while the URL is still changing, so the set outlives that instant
  // and the stray tab is pruned on the next render.
  const missing = useRef(new Set<string>());
  const closing = useRef<string | null>(null);
  const notFound =
    doc.isError && doc.error instanceof ApiError && doc.error.status === 404;
  useEffect(() => {
    if (p === null || !notFound) {
      closing.current = null;
      return;
    }
    if (closing.current === p) return;
    closing.current = p;
    missing.current.add(p);
    toast(`No page at ${p}`);
    onCloseTab(p);
  }, [p, notFound, onCloseTab]);
  useEffect(() => {
    // A path that loads again (a page re-created under the same name) is no
    // longer missing.
    if (p !== null && doc.isSuccess) missing.current.delete(p);
  }, [p, doc.isSuccess]);

  // Coming back to Docs with no page in the URL reopens the last tab, so a
  // reload lands where the reader left off rather than on an empty screen.
  // Tabs known to be missing are closed instead, whichever page is active.
  useEffect(() => {
    let reopened = p !== null;
    for (const path of tabs.paths) {
      if (missing.current.has(path) && path !== p) {
        tabs.close(path);
        continue;
      }
      if (!reopened) {
        onSelect(path);
        reopened = true;
      }
    }
  }, [p, tabs, onSelect]);

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

  // ⌘S / Mod+Enter: commit the exact bytes the editor just serialized (or
  // the buffer, when the shortcut came from outside the editor), without
  // waiting out the pause.
  const saveNow = (markdown?: string) => {
    if (p === null || put.isPending) return;
    const body = markdown ?? draft;
    if (body === undefined) return;
    const saved = client.getQueryData<DocBodyDto>(queryKeys.doc(p))?.body;
    if (saved !== undefined && body === saved) return;
    save(p, body);
  };
  const saveNowRef = useRef(saveNow);
  saveNowRef.current = saveNow;

  // ⌘S anywhere on the page commits now — the browser's "save page" dialog
  // is never what someone editing a wiki means. The rich editor handles the
  // shortcut itself with the bytes it holds, so it is left alone.
  useEffect(() => {
    if (p === null) return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "s")
        return;
      event.preventDefault();
      const target = event.target instanceof Element ? event.target : null;
      if (target?.closest(".ProseMirror")) return;
      saveNowRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [p]);

  // "Close others" flushes the neighbours' unsaved text first: autosave
  // would have committed it a second later anyway, and dropping it would be
  // the one place the always-on editor loses work.
  const closeOthers = (path: string) => {
    for (const other of tabs.paths) {
      if (other === path) continue;
      const body = tabs.drafts[other];
      if (body !== undefined && tabs.isDirty(other)) save(other, body);
    }
    tabs.closeOthers(path);
    if (p !== path) onSelect(path);
  };

  return (
    <>
      {/* The strip only exists while something is open: an empty bar above
          the empty state would be a border with nothing to hold. */}
      {tabs.paths.length > 0 ? (
        <div className="tabs" role="tablist" aria-label="Open pages">
          {tabs.paths.map((path) => (
            <Tab
              key={path}
              path={path}
              active={path === p}
              pinned={tabs.pinned.has(path)}
              dirty={tabs.isDirty(path)}
              onActivate={onSelect}
              onPin={tabs.pin}
              onUnpin={tabs.unpin}
              onClose={onCloseTab}
              onCloseOthers={closeOthers}
            />
          ))}
          <span style={{ flex: 1 }} />
        </div>
      ) : null}

      <div style={{ overflow: "auto", flex: 1 }} className="min-h-0">
        {p === null ? (
          <div className="flex h-full items-center justify-center">
            <Empty
              className="empty"
              title="No page open"
              hint="Pick a page from the sidebar — single click previews it, double click pins it as a tab. Every page is a Markdown file in the repo."
            />
          </div>
        ) : doc.isPending ? (
          <Loading label="Opening page…" />
        ) : doc.isError ? (
          <div className="empty" style={{ padding: 30 }}>
            <p>
              No page at {p}.{" "}
              <button
                type="button"
                className="underline"
                style={{ color: "var(--accent-ink)" }}
                onClick={() => {
                  missing.current.add(p);
                  onCloseTab(p);
                }}
              >
                Back to docs
              </button>
            </p>
            <p className="mono" style={{ fontSize: 11.5, marginTop: 6 }}>
              {doc.error instanceof Error
                ? doc.error.message
                : String(doc.error)}
            </p>
          </div>
        ) : doc.data === undefined ? null : (
          <article className="doc">
            <div className="path">
              {p.split("/").join(" / ")}
              <span className="sp" style={{ flex: 1 }} />
              <span>
                <Pen className="i inline-block" aria-hidden /> always-on editor
                · autosaves 1.5s after you pause
              </span>
            </div>
            {draft === undefined ? (
              <Loading label="Preparing editor…" />
            ) : (
              <Suspense fallback={<Loading label="Loading editor…" />}>
                {docSource ? (
                  // The source recipe draws the box; the editor's own chrome
                  // (fixed height, second border) would double it up.
                  <div className="src [&>div]:h-auto [&>div]:rounded-none [&>div]:border-0 [&>div]:bg-transparent">
                    <CodeMirrorEditor
                      key={p}
                      value={draft}
                      onChange={(next) => tabs.setDraft(p, next)}
                      onSave={saveNow}
                    />
                  </div>
                ) : (
                  // The editor's own 6px inset would shift the body off the
                  // path row's left edge; the article already has margins.
                  <div className="md" style={{ marginInline: -6 }}>
                    <RichEditor
                      key={p}
                      value={draft}
                      onChange={(next) => tabs.setDraft(p, next)}
                      onSave={saveNow}
                      // A document the bridge refuses (conflict markers, a
                      // wasm failure) can still be edited as text.
                      onFallbackToSource={() => setDocSource(true)}
                      className=""
                    />
                  </div>
                )}
              </Suspense>
            )}
            <p
              style={
                {
                  color: "var(--faint)",
                  fontSize: 12,
                  marginTop: 20,
                } as CSSProperties
              }
            >
              Type <kbd>/</kbd> for blocks: heading, list, table, code,
              dit-diagram. <kbd>⌘S</kbd> commits now.
            </p>
          </article>
        )}
      </div>
    </>
  );
}
