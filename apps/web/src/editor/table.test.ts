// Table commands behind the table toolbar. GFM keeps column alignments on
// the table, one per column; TipTap's column commands know nothing of them,
// and the shape plugin only pads or trims at the end — so inserting a column
// in the middle would hand every later column its neighbour's alignment.
import { beforeAll, describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { ditExtensions } from "./extensions";
import { tableActions } from "./table";

beforeAll(() => {
  const empty = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getClientRects = empty;
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  Element.prototype.getClientRects = empty;
});

const cell = (type: string, text: string) => ({
  type,
  content: [{ type: "paragraph", content: text ? [{ type: "text", text }] : [] }],
});
const row = (header: boolean, ...texts: string[]) => ({
  type: "tableRow",
  attrs: { isHeader: header },
  content: texts.map((t) => cell(header ? "tableHeader" : "tableCell", t)),
});

function mount(): Editor {
  const element = document.createElement("div");
  document.body.append(element);
  return new Editor({
    element,
    extensions: ditExtensions(),
    content: {
      type: "doc",
      content: [
        {
          type: "table",
          attrs: { alignments: ["left", "center", "right"] },
          content: [row(true, "A", "B", "C"), row(false, "a1", "b1", "c1")],
        },
      ],
    },
  });
}

/** Put the caret in the cell whose text is `text`. */
function caretIn(editor: Editor, text: string): void {
  let at = -1;
  editor.state.doc.descendants((node, pos) => {
    if (at === -1 && node.isText && node.text === text) at = pos;
  });
  editor.commands.setTextSelection(at + 1);
}

const alignments = (editor: Editor) => editor.state.doc.child(0).attrs.alignments as string[];
const firstRow = (editor: Editor) => {
  const cells: string[] = [];
  editor.state.doc.child(0).child(0).forEach((c) => cells.push(c.textContent));
  return cells;
};
const rows = (editor: Editor) => editor.state.doc.child(0).childCount;

describe("table actions", () => {
  it("inserts a column on the left with no alignment, shifting the rest", () => {
    const editor = mount();
    caretIn(editor, "b1");
    tableActions(editor).addColumnBefore.run();
    expect(firstRow(editor)).toEqual(["A", "", "B", "C"]);
    expect(alignments(editor)).toEqual(["left", "none", "center", "right"]);
  });

  it("inserts a column on the right", () => {
    const editor = mount();
    caretIn(editor, "a1");
    tableActions(editor).addColumnAfter.run();
    expect(firstRow(editor)).toEqual(["A", "", "B", "C"]);
    expect(alignments(editor)).toEqual(["left", "none", "center", "right"]);
  });

  it("deletes a column together with its alignment", () => {
    const editor = mount();
    caretIn(editor, "b1");
    tableActions(editor).deleteColumn.run();
    expect(firstRow(editor)).toEqual(["A", "C"]);
    expect(alignments(editor)).toEqual(["left", "right"]);
  });

  it("adds and deletes body rows", () => {
    const editor = mount();
    caretIn(editor, "a1");
    tableActions(editor).addRowAfter.run();
    expect(rows(editor)).toBe(3);
    tableActions(editor).deleteRow.run();
    expect(rows(editor)).toBe(2);
  });

  it("offers no row above the header and no deleting it — GFM's header is the first row", () => {
    const editor = mount();
    caretIn(editor, "A");
    const actions = tableActions(editor);
    expect(actions.addRowBefore.disabled).toBe(true);
    expect(actions.deleteRow.disabled).toBe(true);
    expect(actions.addRowAfter.disabled).toBe(false);
  });

  it("does not delete the last column", () => {
    const editor = mount();
    caretIn(editor, "a1");
    tableActions(editor).deleteColumn.run();
    caretIn(editor, "b1");
    tableActions(editor).deleteColumn.run();
    caretIn(editor, "c1");
    expect(tableActions(editor).deleteColumn.disabled).toBe(true);
  });

  it("deletes the whole table", () => {
    const editor = mount();
    caretIn(editor, "a1");
    tableActions(editor).deleteTable.run();
    expect(editor.state.doc.child(0).type.name).not.toBe("table");
  });
});
