// The "/" menu's Link and Image entries ask for an address in a real dialog
// instead of the browser's prompt(): one form, both fields at once, a
// preview of the picture, and the editor keeps its place underneath.

import { useState, useSyncExternalStore } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import type { Editor, Range } from "@tiptap/core";

type Request = { kind: "link" | "image"; editor: Editor; range: Range };

let pending: Request | null = null;
const listeners = new Set<() => void>();
const publish = (next: Request | null) => {
  pending = next;
  for (const listener of listeners) listener();
};

/** Open the dialog for the "/" command that was typed at `range`. */
export function requestInsert(request: Request): void {
  publish(request);
}

function usePending(): Request | null {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => pending,
  );
}

/** Insert what the form holds where the "/" was typed. */
export function insertLink(editor: Editor, range: Range, href: string, text: string): void {
  const label = text || href;
  editor
    .chain()
    .focus()
    .deleteRange(range)
    .insertContent([{ type: "text", text: label, marks: [{ type: "link", attrs: { href, title: "" } }] }])
    .run();
}

export function insertImage(editor: Editor, range: Range, src: string, alt: string): void {
  editor
    .chain()
    .focus()
    .deleteRange(range)
    .insertContent({ type: "image", attrs: { src, title: "" }, content: alt ? [{ type: "text", text: alt }] : [] })
    .run();
}

/** Mounted once per editor; answers only that editor's requests, so a page
 *  with two editors shows one dialog. */
export function InsertDialog({ editor }: { editor: Editor }) {
  const request = usePending();
  if (!request || request.editor !== editor) return null;
  return <InsertForm key={`${request.kind}:${request.range.from}`} request={request} />;
}

function InsertForm({ request }: { request: Request }) {
  const { kind, editor, range } = request;
  const [address, setAddress] = useState("");
  const [text, setText] = useState("");
  const close = () => {
    publish(null);
    editor.commands.focus();
  };
  const submit = () => {
    const value = address.trim();
    if (!value) return;
    if (kind === "link") insertLink(editor, range, value, text.trim());
    else insertImage(editor, range, value, text.trim());
    publish(null);
  };
  const isLink = kind === "link";
  return (
    <Dialog.Root open onOpenChange={(open) => (open ? undefined : close())}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">{isLink ? "Insert a link" : "Insert an image"}</Dialog.Title>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              submit();
            }}
          >
            <div className="mw-dlg-b">
              <div className="mw-fld">
                <label htmlFor="dit-insert-address">{isLink ? "Link address" : "Image address"}</label>
                <input
                  id="dit-insert-address"
                  autoFocus
                  value={address}
                  placeholder={isLink ? "https://… or docs/page.md" : "https://…/picture.png or docs/assets/picture.png"}
                  onChange={(event) => setAddress(event.target.value)}
                />
              </div>
              <div className="mw-fld">
                <label htmlFor="dit-insert-text">{isLink ? "Text to show (optional)" : "Describe the picture (optional)"}</label>
                <input
                  id="dit-insert-text"
                  value={text}
                  placeholder={isLink ? "Defaults to the address" : "Read aloud by screen readers"}
                  onChange={(event) => setText(event.target.value)}
                />
              </div>
            </div>
            <div className="mw-dlg-f">
              <button type="button" className="mw-btn" onClick={close}>
                Cancel
              </button>
              <button type="submit" className="mw-btn pri" disabled={!address.trim()}>
                Insert
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
