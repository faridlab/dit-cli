// The "/" menu's fence entries (diagram, mermaid, callouts) used to insert a
// block and leave the caret after it, so the first words typed landed in the
// paragraph below. The caret belongs inside the new block.
import { beforeAll, describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { ditExtensions } from "./extensions";
import { insertFence, SLASH_ITEMS } from "./SlashMenu";

beforeAll(() => {
  const empty = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getClientRects = empty;
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  Element.prototype.getClientRects = empty;
});

function mount(text: string): Editor {
  const element = document.createElement("div");
  document.body.append(element);
  return new Editor({
    element,
    extensions: ditExtensions(),
    content: { type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text }] }] },
  });
}

describe("slash menu fences", () => {
  it.each([
    ["Note", "dit-note"],
    ["Warning", "dit-warning"],
    ["Mermaid", "mermaid"],
    ["Diagram", "dit-diagram"],
  ])("%s on an empty line turns the line into a %s block, caret inside", (label, language) => {
    const editor = mount("/no");
    const item = SLASH_ITEMS.find((i) => i.label === label);
    if (!item) throw new Error(`no ${label} item`);
    item.command({ editor, range: { from: 1, to: 4 } });
    editor.commands.insertContent("typed");
    expect(editor.state.doc.childCount).toBe(1);
    const block = editor.state.doc.child(0);
    expect([block.type.name, block.attrs.language, block.textContent]).toEqual(["codeBlock", language, "typed"]);
  });

  it("keeps the words on a line and puts the block below it", () => {
    // "/note" typed at the start of a line that has text after it: before,
    // the block went above and the next words joined that text.
    const editor = mount("/notenked to the flow");
    insertFence(editor, { from: 1, to: 6 }, "dit-note");
    editor.commands.insertContent("typed");
    const rows: string[] = [];
    editor.state.doc.forEach((node) => rows.push(`${node.type.name}:${node.attrs.language ?? ""}:${node.textContent}`));
    expect(rows).toEqual(["paragraph::nked to the flow", "codeBlock:dit-note:typed"]);
  });
});
