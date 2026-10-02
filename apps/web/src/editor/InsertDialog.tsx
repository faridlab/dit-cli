// The "/" menu's Link and Image entries ask for an address in a real dialog
// instead of the browser's prompt(): one form, both fields at once, a
// preview of the picture, and the editor keeps its place underneath.

import { useState, useSyncExternalStore } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import type { Editor, Range } from "@tiptap/core";

import { uploadAttachment } from "../lib/api";
import { altFrom, type AttachContext } from "../lib/attachments";

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
export function InsertDialog({ editor, attach }: { editor: Editor; attach?: AttachContext }) {
  const request = usePending();
  if (!request || request.editor !== editor) return null;
  return <InsertForm key={`${request.kind}:${request.range.from}`} request={request} attach={attach} />;
}

function InsertForm({ request, attach }: { request: Request; attach?: AttachContext }) {
  const { kind, editor, range } = request;
  const [address, setAddress] = useState("");
  const [text, setText] = useState("");
  const [uploading, setUploading] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const upload = (file: File | undefined) => {
    if (!file || !attach) return;
    setUploading(true);
    setProblem(null);
    uploadAttachment(attach.target, file, file.name)
      .then((done) => {
        insertImage(editor, range, done.link, text.trim() || altFrom(file.name));
        publish(null);
      })
      .catch((error: unknown) => {
        setUploading(false);
        setProblem(error instanceof Error ? error.message : "The picture could not be added");
      });
  };
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
              {!isLink && attach ? (
                <div className="mw-fld">
                  <label htmlFor="dit-insert-file">From this computer</label>
                  <input
                    id="dit-insert-file"
                    type="file"
                    accept="image/png,image/jpeg,image/gif,image/webp"
                    disabled={uploading}
                    onChange={(event) => upload(event.target.files?.[0])}
                  />
                  <p className="mw-hint" style={{ margin: 0 }}>
                    {uploading
                      ? "Adding the picture…"
                      : "PNG, JPEG, GIF or WebP up to 1 MB — committed next to this page. You can also paste or drop a picture straight into the text."}
                  </p>
                  {problem ? <p className="mw-errline">{problem}</p> : null}
                </div>
              ) : null}
              <div className="mw-fld">
                <label htmlFor="dit-insert-address">{isLink ? "Link address" : attach ? "…or an address" : "Image address"}</label>
                <input
                  id="dit-insert-address"
                  autoFocus={isLink || !attach}
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
