// The selection toolbar: the inline-format buttons that appear when text is
// selected. Same commands as the keyboard shortcuts — it exists so nobody
// has to memorize them.

import { isTextSelection } from "@tiptap/core";
import type { EditorState } from "@tiptap/pm/state";
import { useEditorState, type Editor } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";
import { useState } from "react";
import { Bold, Check, ChevronDown, Code, ExternalLink, Italic, Link2, Strikethrough, Unlink } from "lucide-react";

import { BLOCK_KINDS, currentBlock, turnInto } from "./blocks";

/** What the toolbar needs to know to decide whether to appear. A structural
 *  subset of what TipTap hands `shouldShow`, so the rule can be exercised
 *  without a live editor view. */
export interface BubbleVisibility {
  editor: Pick<Editor, "isEditable" | "isActive">;
  /** The toolbar's own element — clicking a button moves focus into it. */
  element: HTMLElement;
  view: { hasFocus: () => boolean };
  state: EditorState;
  from: number;
  to: number;
}

/** Whether the selection toolbar belongs on screen right now.
 *
 *  "There is a range" is not the same as "someone selected text": replacing
 *  the document (a server refresh, or opening another issue in the panel)
 *  leaves the whole doc selected in an editor nobody has touched. Focus is
 *  what makes a selection a person's. */
export function shouldShowBubble({
  editor,
  element,
  view,
  state,
  from,
  to,
}: BubbleVisibility): boolean {
  const insideMenu = element.contains(document.activeElement);
  if (!view.hasFocus() && !insideMenu) return false;
  if (!editor.isEditable || state.selection.empty) return false;
  // An empty text block has a range but nothing to format.
  if (isTextSelection(state.selection) && state.doc.textBetween(from, to).length === 0) {
    return false;
  }
  // Code is edited as bytes: inline formatting there would be a lie.
  return !editor.isActive("codeBlock") && !editor.isActive("htmlBlock");
}

const BUTTON =
  "flex size-7 items-center justify-center rounded text-ink-2 transition-colors " +
  "hover:bg-edge hover:text-ink data-active:bg-edge data-active:text-accent";

function Toggle({
  label,
  active,
  onClick,
  children,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      // mousedown, not click: the editor must not lose selection focus
      // before the command runs.
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
      data-active={active || undefined}
      className={BUTTON}
    >
      {children}
    </button>
  );
}

/** Only these open from the toolbar: a `javascript:` href in a pulled
 *  document must not run because someone clicked "open". */
export function isOpenableHref(href: string): boolean {
  return /^(https?:|mailto:)/i.test(href.trim());
}

export function BubbleToolbar({ editor }: { editor: Editor }) {
  // Re-render with the selection, so the active marks and the block kind
  // shown are the ones under the caret now.
  useEditorState({ editor, selector: ({ editor: e }) => e.state });
  // The toolbar has three faces: the format buttons, the block-kind list
  // ("Turn into"), and the link editor. Each replaces the buttons in place
  // so focus never leaves the toolbar and the selection stays put.
  const [face, setFace] = useState<"format" | "blocks" | "link">("format");
  const [href, setHref] = useState("");

  const openLink = () => {
    const existing = editor.getAttributes("link").href;
    setHref(typeof existing === "string" ? existing : "");
    setFace("link");
  };
  const applyLink = () => {
    const value = href.trim();
    const chain = editor.chain().focus().extendMarkRange("link");
    if (value === "") chain.unsetLink().run();
    else chain.setLink({ href: value, title: "" }).run();
    setFace("format");
  };
  const kind = currentBlock(editor);
  const kindLabel = BLOCK_KINDS.find((block) => block.kind === kind)?.label ?? "Text";

  return (
    <BubbleMenu
      editor={editor}
      shouldShow={shouldShowBubble}
      options={{ placement: "top", offset: 6, onHide: () => setFace("format") }}
    >
      {face === "link" ? (
        <div className="dit-bubble dit-bubble-link">
          <input
            autoFocus
            className="dit-bubble-input"
            placeholder="Paste or type a link"
            aria-label="Link address"
            value={href}
            onChange={(event) => setHref(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                applyLink();
              } else if (event.key === "Escape") {
                event.preventDefault();
                setFace("format");
                editor.commands.focus();
              }
            }}
          />
          <Toggle label="Apply link" active={false} onClick={applyLink}>
            <Check className="size-3.5" aria-hidden />
          </Toggle>
          {isOpenableHref(href) ? (
            <Toggle label="Open link in a new tab" active={false} onClick={() => window.open(href.trim(), "_blank", "noopener,noreferrer")}>
              <ExternalLink className="size-3.5" aria-hidden />
            </Toggle>
          ) : null}
          {editor.isActive("link") ? (
            <Toggle
              label="Remove link"
              active={false}
              onClick={() => {
                editor.chain().focus().extendMarkRange("link").unsetLink().run();
                setFace("format");
              }}
            >
              <Unlink className="size-3.5" aria-hidden />
            </Toggle>
          ) : null}
        </div>
      ) : face === "blocks" ? (
        <div className="dit-bubble dit-bubble-blocks" role="menu" aria-label="Turn into">
          {BLOCK_KINDS.map((block) => {
            const Icon = block.icon;
            return (
              <button
                key={block.kind}
                type="button"
                role="menuitemradio"
                aria-checked={block.kind === kind}
                className="dit-bubble-row"
                data-active={block.kind === kind || undefined}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  turnInto(editor, block.kind);
                  setFace("format");
                }}
              >
                <Icon className="size-3.5" aria-hidden />
                {block.label}
              </button>
            );
          })}
        </div>
      ) : (
        <div className="dit-bubble">
          <button
            type="button"
            className="dit-bubble-kind"
            aria-label={`Turn into — now ${kindLabel}`}
            aria-haspopup="menu"
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => setFace("blocks")}
          >
            {kindLabel}
            <ChevronDown className="size-3" aria-hidden />
          </button>
          <span className="mx-0.5 h-4 w-px self-center bg-edge" aria-hidden />
          <Toggle
            label="Bold"
            active={editor.isActive("bold")}
            onClick={() => editor.chain().focus().toggleBold().run()}
          >
            <Bold className="size-3.5" aria-hidden />
          </Toggle>
          <Toggle
            label="Italic"
            active={editor.isActive("italic")}
            onClick={() => editor.chain().focus().toggleItalic().run()}
          >
            <Italic className="size-3.5" aria-hidden />
          </Toggle>
          <Toggle
            label="Strikethrough"
            active={editor.isActive("strike")}
            onClick={() => editor.chain().focus().toggleStrike().run()}
          >
            <Strikethrough className="size-3.5" aria-hidden />
          </Toggle>
          <Toggle
            label="Code"
            active={editor.isActive("code")}
            onClick={() => editor.chain().focus().toggleCode().run()}
          >
            <Code className="size-3.5" aria-hidden />
          </Toggle>
          <span className="mx-0.5 h-4 w-px self-center bg-edge" aria-hidden />
          <Toggle label="Link" active={editor.isActive("link")} onClick={openLink}>
            <Link2 className="size-3.5" aria-hidden />
          </Toggle>
        </div>
      )}
    </BubbleMenu>
  );
}
