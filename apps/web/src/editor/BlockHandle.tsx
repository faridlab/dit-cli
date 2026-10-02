// The handle beside the block under the pointer: "+" opens the "/" menu on
// a new line below, the grip drags the block or — clicked — opens its menu
// (turn into, duplicate, move, delete). It exists so nobody has to know that
// "# " makes a heading or that a block can be dragged.

import { useRef, useState } from "react";
import type { Editor } from "@tiptap/react";
import { DragHandle } from "@tiptap/extension-drag-handle-react";
import type { Node as PmNode } from "@tiptap/pm/model";
import { ArrowDown, ArrowUp, Copy, GripVertical, Plus, Trash2 } from "lucide-react";

import { MenuButton, type MenuItem } from "../components/chrome";
import { TextSelection } from "@tiptap/pm/state";

import { BLOCK_KINDS, blockKindAt, deleteBlock, duplicateBlock, insertBlockAfter, moveBlock, turnInto } from "./blocks";

/** Blocks whose text can become another kind of block. Tables, images and
 *  raw HTML keep their own shape. */
const TURNABLE = new Set(["paragraph", "heading", "codeBlock", "bulletList", "orderedList", "blockquote"]);

type Target = { node: PmNode; pos: number };

export function BlockHandle({ editor }: { editor: Editor }) {
  const hovered = useRef<Target | null>(null);
  // The block the open menu acts on — fixed when the menu opens, so moving
  // the pointer over other blocks while choosing does not retarget it.
  const [menuTarget, setMenuTarget] = useState<Target | null>(null);

  const openMenu = (open: boolean) => {
    // The React drag handle registers no lock commands; its plugin reads
    // this meta, which keeps the handle beside the block while choosing.
    setMenuTarget(open ? hovered.current : null);
    editor.commands.setMeta("lockDragHandle", open);
  };

  const items = (): MenuItem[] => {
    if (!menuTarget) return [];
    const { node, pos } = menuTarget;
    const list: MenuItem[] = [];
    if (TURNABLE.has(node.type.name)) {
      // Which kind the block is now, read where a caret inside it would sit.
      const $inside = editor.state.doc.resolve(Math.min(pos + 1, editor.state.doc.content.size));
      const now = blockKindAt(TextSelection.near($inside).$from);
      list.push({ kind: "head", label: "Turn into" });
      for (const block of BLOCK_KINDS) {
        const Icon = block.icon;
        list.push({
          label: block.label,
          icon: <Icon className="i" aria-hidden />,
          on: block.kind === now,
          run: () => void turnInto(editor, block.kind, pos),
        });
      }
      list.push({ kind: "sep" });
    }
    list.push(
      {
        label: "Duplicate",
        icon: <Copy className="i" aria-hidden />,
        run: () => void duplicateBlock(editor, pos),
      },
      {
        label: "Move up",
        icon: <ArrowUp className="i" aria-hidden />,
        disabled: editor.state.doc.resolve(pos).index() === 0,
        run: () => void moveBlock(editor, pos, -1),
      },
      {
        label: "Move down",
        icon: <ArrowDown className="i" aria-hidden />,
        disabled: (() => {
          const $pos = editor.state.doc.resolve(pos);
          return $pos.index() + 1 >= $pos.parent.childCount;
        })(),
        run: () => void moveBlock(editor, pos, 1),
      },
      { kind: "sep" },
      {
        label: "Delete",
        icon: <Trash2 className="i" aria-hidden />,
        danger: true,
        run: () => void deleteBlock(editor, pos),
      },
    );
    return list;
  };

  return (
    <DragHandle
      editor={editor}
      onNodeChange={({ node, pos }) => {
        hovered.current = node ? { node, pos } : null;
      }}
    >
      <span className="dit-block-handle">
        <button
          type="button"
          className="dit-handle-btn"
          aria-label="Add a block below"
          title="Add a block below"
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => {
            const target = hovered.current;
            if (target) insertBlockAfter(editor, target.pos);
          }}
        >
          <Plus className="size-3.5" aria-hidden />
        </button>
        <MenuButton items={items()} open={menuTarget !== null} onOpenChange={openMenu}>
          <button
            type="button"
            className="dit-handle-btn dit-drag-handle"
            aria-label="Drag to move, click for options"
            title="Drag to move · click for options"
          >
            <GripVertical className="size-3.5" aria-hidden />
          </button>
        </MenuButton>
      </span>
    </DragHandle>
  );
}
