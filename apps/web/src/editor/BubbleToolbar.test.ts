// The selection toolbar appeared over editors nobody had touched — two at
// once on the issue page, floating across the description and the comment
// box. The cause was the visibility rule: it asked "is there a range?" and
// a range is not a selection. Replacing a document (a server refresh, or
// the next issue in the side panel) leaves the whole doc selected in an
// editor that was never focused.
//
// A real editor supplies real selections; only focus is faked, because that
// is the signal the rule was missing.
import { describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { shouldShowBubble } from "./BubbleToolbar";
import { ditExtensions } from "./extensions";

function editorWith(content: Record<string, unknown>): Editor {
  const element = document.createElement("div");
  document.body.append(element);
  return new Editor({ element, extensions: ditExtensions(), content });
}

const paragraph = (text: string) => ({
  type: "doc",
  content: [{ type: "paragraph", content: [{ type: "text", text }] }],
});

/** The toolbar element, detached from the editor — nothing inside it is
 *  focused unless a test says so. */
const menu = () => document.createElement("div");

function visibility(editor: Editor, hasFocus: boolean, element = menu()) {
  const { from, to } = editor.state.selection;
  return shouldShowBubble({ editor, element, view: { hasFocus: () => hasFocus }, state: editor.state, from, to });
}

describe("shouldShowBubble", () => {
  it("stays away from a full-document selection nobody made", () => {
    const editor = editorWith(paragraph("hello world"));
    // What replacing the document leaves behind: everything selected.
    editor.commands.selectAll();
    expect(editor.state.selection.empty).toBe(false);

    expect(visibility(editor, false)).toBe(false);
  });

  it("appears for text a focused writer selected", () => {
    const editor = editorWith(paragraph("hello world"));
    editor.commands.setTextSelection({ from: 1, to: 6 });

    expect(visibility(editor, true)).toBe(true);
  });

  it("stays away from a caret with nothing selected", () => {
    const editor = editorWith(paragraph("hello world"));
    editor.commands.setTextSelection({ from: 3, to: 3 });

    expect(visibility(editor, true)).toBe(false);
  });

  it("stays away from a text selection that spans no text", () => {
    // Dragging across blank lines: a real range, nothing to format.
    const editor = editorWith({
      type: "doc",
      content: [{ type: "paragraph" }, { type: "paragraph" }],
    });
    editor.commands.setTextSelection({ from: 1, to: 3 });
    expect(editor.state.selection.empty).toBe(false);

    expect(visibility(editor, true)).toBe(false);
  });

  it("stays away inside a code block, where inline marks mean nothing", () => {
    const editor = editorWith({
      type: "doc",
      content: [{ type: "codeBlock", content: [{ type: "text", text: "let x = 1;" }] }],
    });
    editor.commands.setTextSelection({ from: 1, to: 5 });

    expect(visibility(editor, true)).toBe(false);
  });

  it("survives clicking its own buttons, which moves focus out of the editor", () => {
    const editor = editorWith(paragraph("hello world"));
    editor.commands.setTextSelection({ from: 1, to: 6 });

    const element = menu();
    const button = document.createElement("button");
    element.append(button);
    document.body.append(element);
    button.focus();

    expect(visibility(editor, false, element)).toBe(true);
  });
});
