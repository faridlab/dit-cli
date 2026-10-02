// The block commands behind the block menu, the "+" button and the
// selection toolbar's "Turn into". They run against the production schema in
// jsdom: a command that builds a shape the bridge cannot write would surface
// as a save error, so the shapes are pinned here.
import { beforeAll, describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { ditExtensions } from "./extensions";
import {
  currentBlock,
  deleteBlock,
  duplicateBlock,
  insertBlockAfter,
  moveBlock,
  turnInto,
  type BlockKind,
} from "./blocks";

// `focus()` scrolls the caret into view, which asks for layout jsdom does
// not have.
beforeAll(() => {
  const empty = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  const rect = () => new DOMRect(0, 0, 0, 0);
  Range.prototype.getClientRects = empty;
  Range.prototype.getBoundingClientRect = rect;
  Element.prototype.getClientRects = empty;
});

const paragraph = (text: string) => ({ type: "paragraph", content: [{ type: "text", text }] });

function mount(content: Array<Record<string, unknown>>): Editor {
  const element = document.createElement("div");
  document.body.append(element);
  return new Editor({ element, extensions: ditExtensions(), content: { type: "doc", content } });
}

/** Top-level blocks as `type:text`, for compact assertions. */
function outline(editor: Editor): string[] {
  const rows: string[] = [];
  editor.state.doc.forEach((node) => rows.push(`${node.type.name}:${node.textContent}`));
  return rows;
}

/** Position of the n-th top-level block. */
function posOf(editor: Editor, index: number): number {
  let pos = 0;
  for (let i = 0; i < index; i += 1) pos += editor.state.doc.child(i).nodeSize;
  return pos;
}

describe("turnInto", () => {
  const cases: Array<[BlockKind, string]> = [
    ["h1", "heading"],
    ["h2", "heading"],
    ["h3", "heading"],
    ["bullet", "bulletList"],
    ["ordered", "orderedList"],
    ["todo", "bulletList"],
    ["quote", "blockquote"],
    ["code", "codeBlock"],
  ];

  it.each(cases)("turns a paragraph into %s", (kind, nodeType) => {
    const editor = mount([paragraph("words")]);
    editor.commands.setTextSelection(2);
    turnInto(editor, kind);
    expect(editor.state.doc.child(0).type.name).toBe(nodeType);
    expect(editor.state.doc.textContent).toBe("words");
    expect(currentBlock(editor)).toBe(kind);
  });

  it("turns a heading back into text", () => {
    const editor = mount([{ type: "heading", attrs: { level: 2 }, content: [{ type: "text", text: "title" }] }]);
    editor.commands.setTextSelection(2);
    turnInto(editor, "paragraph");
    expect(outline(editor)).toEqual(["paragraph:title"]);
  });

  it("switches a bullet to a to-do and back without leaving the list", () => {
    const editor = mount([paragraph("item")]);
    editor.commands.setTextSelection(2);
    turnInto(editor, "bullet");
    turnInto(editor, "todo");
    expect(currentBlock(editor)).toBe("todo");
    turnInto(editor, "bullet");
    expect(currentBlock(editor)).toBe("bullet");
    expect(outline(editor)).toEqual(["bulletList:item"]);
  });

  it("acts on the block at a given position, not where the caret is", () => {
    const editor = mount([paragraph("first"), paragraph("second")]);
    editor.commands.setTextSelection(2);
    turnInto(editor, "h1", posOf(editor, 1));
    expect(outline(editor)).toEqual(["paragraph:first", "heading:second"]);
  });
});

describe("block operations", () => {
  it("duplicates a block right below itself", () => {
    const editor = mount([paragraph("a"), paragraph("b")]);
    duplicateBlock(editor, posOf(editor, 0));
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:a", "paragraph:b"]);
  });

  it("deletes a block", () => {
    const editor = mount([paragraph("a"), paragraph("b"), paragraph("c")]);
    deleteBlock(editor, posOf(editor, 1));
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:c"]);
  });

  it("moves a block up and down, and stops at the edges", () => {
    const editor = mount([paragraph("a"), paragraph("b"), paragraph("c")]);
    expect(moveBlock(editor, posOf(editor, 1), -1)).toBe(true);
    expect(outline(editor)).toEqual(["paragraph:b", "paragraph:a", "paragraph:c"]);
    expect(moveBlock(editor, posOf(editor, 0), 1)).toBe(true);
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:b", "paragraph:c"]);
    expect(moveBlock(editor, posOf(editor, 0), -1)).toBe(false);
    expect(moveBlock(editor, posOf(editor, 2), 1)).toBe(false);
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:b", "paragraph:c"]);
  });

  it("adds a block below with the '/' menu armed", () => {
    const editor = mount([paragraph("a"), paragraph("b")]);
    insertBlockAfter(editor, posOf(editor, 0));
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:/", "paragraph:b"]);
    // The caret sits after the slash, where typing filters the menu.
    const { $from } = editor.state.selection;
    expect($from.parent.textContent).toBe("/");
    expect($from.parentOffset).toBe(1);
  });

  it("reuses an empty line instead of adding another", () => {
    const editor = mount([paragraph("a"), { type: "paragraph" }]);
    insertBlockAfter(editor, posOf(editor, 1));
    expect(outline(editor)).toEqual(["paragraph:a", "paragraph:/"]);
  });
});
