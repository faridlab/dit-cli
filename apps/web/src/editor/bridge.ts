// The only module that touches the WASM bridge. Everything else in the app
// talks to `markdownToDoc` / `docToMarkdown` and gets a value-or-error back —
// never a throw — so a bridge failure degrades the editor to source mode
// instead of blanking the pane.
//
// The wasm side is `crates/dit-wasm` (built by `just wasm-build`); the
// semantics it enforces — canonical bytes in, canonical bytes out, conflict
// markers refused — are pinned by tests there and in `dit-parse`.

import init, { doc_to_markdown, markdown_to_doc } from "./wasm/dit_wasm";

/** A ProseMirror document as plain JSON (what `editor.getJSON()` returns). */
export type PmDoc = Record<string, unknown>;

export type BridgeResult<T> = { ok: true; value: T } | { ok: false; error: string };

// One init per page, shared by every editor instance. A failed init resets
// so the next editor mount retries instead of caching the failure forever.
let ready: Promise<void> | null = null;

function ensure(): Promise<void> {
  if (ready === null) {
    const attempt: Promise<void> = init().then(
      () => {},
      (cause: unknown) => {
        ready = null; // a failed init must not be cached as the forever-state
        throw cause;
      },
    );
    ready = attempt;
    return attempt;
  }
  return ready;
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

/** Markdown bytes → a ProseMirror document, or the reason they refuse. */
export async function markdownToDoc(markdown: string): Promise<BridgeResult<PmDoc>> {
  try {
    await ensure();
    const json = markdown_to_doc(markdown);
    return { ok: true, value: JSON.parse(json) as PmDoc };
  } catch (cause) {
    return { ok: false, error: message(cause) };
  }
}

/** A ProseMirror document → canonical markdown (byte-identical to dit fmt). */
export async function docToMarkdown(doc: PmDoc): Promise<BridgeResult<string>> {
  try {
    await ensure();
    return { ok: true, value: doc_to_markdown(JSON.stringify(doc)) };
  } catch (cause) {
    return { ok: false, error: message(cause) };
  }
}
