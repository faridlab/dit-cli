// The selection toolbar: the inline-format buttons that appear when text is
// selected. Same commands as the keyboard shortcuts — it exists so nobody
// has to memorize them.

import type { Editor } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";
import { Bold, Code, Italic, Link2, Strikethrough } from "lucide-react";

const BUTTON =
  "flex size-7 items-center justify-center rounded text-zinc-400 transition-colors " +
  "hover:bg-edge hover:text-zinc-100 data-active:bg-edge data-active:text-teal-300";

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
      shouldShow={({ editor: e, from, to }) =>
        from !== to && !e.isActive("codeBlock") && !e.isActive("htmlBlock")
      }
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
