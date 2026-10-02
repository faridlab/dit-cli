// Block-level commands shared by the block menu (the grip beside each
// block), the "+" button and the selection toolbar's "Turn into". Every one
// builds a shape the bridge already writes — headings, lists, to-dos,
// quotes, code — so nothing here can author a document the save path would
// refuse.

import type { Editor } from "@tiptap/core";
import type { ResolvedPos } from "@tiptap/pm/model";
import { TextSelection } from "@tiptap/pm/state";
import {
  Code2,
  Heading1,
  Heading2,
  Heading3,
  List,
  ListOrdered,
  ListTodo,
  Quote,
  Type,
} from "lucide-react";

export type BlockKind = "paragraph" | "h1" | "h2" | "h3" | "bullet" | "ordered" | "todo" | "quote" | "code";

export const BLOCK_KINDS: Array<{ kind: BlockKind; label: string; icon: typeof Type }> = [
  { kind: "paragraph", label: "Text", icon: Type },
  { kind: "h1", label: "Heading 1", icon: Heading1 },
  { kind: "h2", label: "Heading 2", icon: Heading2 },
  { kind: "h3", label: "Heading 3", icon: Heading3 },
  { kind: "bullet", label: "Bullet list", icon: List },
  { kind: "ordered", label: "Numbered list", icon: ListOrdered },
  { kind: "todo", label: "To-do list", icon: ListTodo },
  { kind: "quote", label: "Quote", icon: Quote },
  { kind: "code", label: "Code block", icon: Code2 },
];

/** What the block under the caret is, in the menu's terms. */
export function currentBlock(editor: Editor): BlockKind {
  return blockKindAt(editor.state.selection.$from);
}

/** What the block holding `$from` is, in the menu's terms. */
export function blockKindAt($from: ResolvedPos): BlockKind {
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    const node = $from.node(depth);
    switch (node.type.name) {
      case "codeBlock":
        return "code";
      case "heading":
        return node.attrs.level === 1 ? "h1" : node.attrs.level === 2 ? "h2" : "h3";
      case "listItem":
        if (node.attrs.task !== null) return "todo";
        return $from.node(depth - 1).type.name === "orderedList" ? "ordered" : "bullet";
      case "blockquote":
        return "quote";
    }
  }
  return "paragraph";
}

/** Turn the block under the caret — or the block starting at `at` — into
 *  `kind`. Lists and quotes are unwrapped first, so the result is the one
 *  block asked for rather than a heading inside a quote inside a list. */
export function turnInto(editor: Editor, kind: BlockKind, at?: number): boolean {
  let chain = editor.chain().focus();
  if (at !== undefined) {
    chain = chain.command(({ tr }) => {
      tr.setSelection(TextSelection.near(tr.doc.resolve(Math.min(at + 1, tr.doc.content.size))));
      return true;
    });
  }
  if (!chain.run()) return false;

  const from = currentBlock(editor);
  if (from === kind) return true;
  // A bullet and a to-do are the same list: flip the item, keep the list.
  if ((from === "bullet" && kind === "todo") || (from === "todo" && kind === "bullet")) {
    return editor
      .chain()
      .focus()
      .updateAttributes("listItem", { task: kind === "todo" ? false : null })
      .run();
  }

  const base = editor.chain().focus().clearNodes();
  switch (kind) {
    case "paragraph":
      return base.setParagraph().run();
    case "h1":
      return base.setHeading({ level: 1 }).run();
    case "h2":
      return base.setHeading({ level: 2 }).run();
    case "h3":
      return base.setHeading({ level: 3 }).run();
    case "bullet":
      return base.toggleBulletList().run();
    case "ordered":
      return base.toggleOrderedList().run();
    case "todo":
      return base.toggleBulletList().updateAttributes("listItem", { task: false }).run();
    case "quote":
      return base.toggleBlockquote().run();
    case "code":
      return base.setCodeBlock().run();
  }
}

/** Copy the block at `pos` to just below itself. */
export function duplicateBlock(editor: Editor, pos: number): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  editor.view.dispatch(editor.state.tr.insert(pos + node.nodeSize, node.copy(node.content)));
  return true;
}

export function deleteBlock(editor: Editor, pos: number): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  editor.view.dispatch(editor.state.tr.delete(pos, pos + node.nodeSize));
  return true;
}

/** Swap the block at `pos` with its neighbour above (-1) or below (1).
 *  False at the edge of its container. */
export function moveBlock(editor: Editor, pos: number, direction: -1 | 1): boolean {
  const { doc } = editor.state;
  const node = doc.nodeAt(pos);
  if (!node) return false;
  const $pos = doc.resolve(pos);
  const index = $pos.index();
  const parent = $pos.parent;
  if (direction === -1) {
    if (index === 0) return false;
    const above = parent.child(index - 1);
    const tr = editor.state.tr.delete(pos, pos + node.nodeSize).insert(pos - above.nodeSize, node);
    editor.view.dispatch(tr);
    return true;
  }
  if (index + 1 >= parent.childCount) return false;
  const below = parent.child(index + 1);
  const tr = editor.state.tr.delete(pos, pos + node.nodeSize).insert(pos + below.nodeSize, node);
  editor.view.dispatch(tr);
  return true;
}

/** The "+" beside a block: open the "/" menu on a fresh line below it — or
 *  on the block itself when it is already an empty line. */
export function insertBlockAfter(editor: Editor, pos: number): boolean {
  const node = editor.state.doc.nodeAt(pos);
  if (!node) return false;
  if (node.type.name === "paragraph" && node.content.size === 0) {
    return editor.chain().focus().insertContentAt(pos + 1, "/").run();
  }
  const at = pos + node.nodeSize;
  return editor
    .chain()
    .focus()
    .insertContentAt(at, { type: "paragraph", content: [{ type: "text", text: "/" }] })
    .setTextSelection(at + 2)
    .run();
}
