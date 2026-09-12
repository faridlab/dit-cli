// The selection toolbar: the inline-format buttons that appear when text is
// selected. Same commands as the keyboard shortcuts — it exists so nobody
// has to memorize them.

import { isTextSelection } from "@tiptap/core";
import type { EditorState } from "@tiptap/pm/state";
import type { Editor } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";
import { Bold, Code, Italic, Link2, Strikethrough } from "lucide-react";

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

export function BubbleToolbar({ editor }: { editor: Editor }) {
  const linkHref = () => {
    const existing = editor.getAttributes("link").href;
    const href = window.prompt("Link URL", typeof existing === "string" ? existing : "");
    if (href === null) return; // cancelled — leave the link alone
    if (href === "") {
      editor.chain().focus().unsetLink().run();
    } else {
      editor.chain().focus().setLink({ href, title: "" }).run();
    }
  };

  return (
    <BubbleMenu
      editor={editor}
      shouldShow={shouldShowBubble}
      options={{ placement: "top", offset: 6 }}
    >
      <div className="dit-bubble">
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
        <span className="mx-0.5 h-4 w-px bg-edge" aria-hidden />
        <button
          type="button"
          aria-label="Link"
          aria-pressed={editor.isActive("link")}
          onMouseDown={(event) => event.preventDefault()}
          onClick={linkHref}
          data-active={editor.isActive("link") || undefined}
          className={BUTTON}
        >
          <Link2 className="size-3.5" aria-hidden />
        </button>
      </div>
    </BubbleMenu>
  );
}
