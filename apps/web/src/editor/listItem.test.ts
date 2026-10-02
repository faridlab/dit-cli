// The list item NodeView owns its own DOM (an <li> with an optional task
// checkbox). ProseMirror only renders a node's children into `contentDOM`
// when that element is attached under `dom` — a detached container makes
// every list item render as an empty bullet while the source still holds
// the text. This mounts the production extension set in jsdom and pins
// that the text of a list item is actually visible.
import { describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { ditExtensions } from "./extensions";

function mount(content: Record<string, unknown>): HTMLElement {
  return mountEditor(content).element;
}

function mountEditor(content: Record<string, unknown>): { editor: Editor; element: HTMLElement } {
  const element = document.createElement("div");
  document.body.append(element);
  const editor = new Editor({ element, extensions: ditExtensions(), content });
  return { editor, element };
}

/** The `task` attr of every list item, in document order. */
function tasks(editor: Editor): unknown[] {
  const found: unknown[] = [];
  editor.state.doc.descendants((node) => {
    if (node.type.name === "listItem") found.push(node.attrs.task);
  });
  return found;
}

const list = (...items: Array<{ task: unknown; text: string }>) => ({
  type: "doc",
  content: [
    {
      type: "bulletList",
      attrs: { tight: true },
      content: items.map(({ task, text }) => ({ type: "listItem", attrs: { task }, content: [paragraph(text)] })),
    },
  ],
});

/** Type text the way a keystroke arrives: through the view's text-input
 *  handler, which is where input rules run. */
function type(editor: Editor, text: string): void {
  const view = editor.view;
  for (const ch of text) {
    const { from, to } = view.state.selection;
    const insert = () => view.state.tr.insertText(ch, from, to);
    const handled = view.someProp("handleTextInput", (f) => f(view, from, to, ch, insert));
    if (handled !== true) view.dispatch(insert());
  }
}

/** Put the caret at the end of the document's text and press Enter. */
function enterAtEnd(editor: Editor): void {
  editor.commands.setTextSelection(editor.state.doc.content.size - 3);
  editor.commands.keyboardShortcut("Enter");
}

const paragraph = (text: string) => ({ type: "paragraph", content: [{ type: "text", text }] });

describe("DitListItem NodeView", () => {
  it("renders the item's paragraphs inside the <li>", () => {
    const element = mount({
      type: "doc",
      content: [
        {
          type: "bulletList",
          attrs: { tight: true },
          content: [
            { type: "listItem", attrs: { task: null }, content: [paragraph("first item")] },
            {
              type: "listItem",
              attrs: { task: null },
              content: [paragraph("second item"), paragraph("with a continuation")],
            },
          ],
        },
      ],
    });
    const items = Array.from(element.querySelectorAll("li"));
    expect(items.map((li) => li.textContent)).toEqual(["first item", "second itemwith a continuation"]);
    // The wrapper must be a descendant of the <li>, not a free-floating div.
    for (const li of items) expect(li.querySelector(".dit-li-content")).not.toBeNull();
  });

  it("puts a task item's checkbox before its text", () => {
    const element = mount({
      type: "doc",
      content: [
        {
          type: "bulletList",
          attrs: { tight: true },
          content: [{ type: "listItem", attrs: { task: "x" }, content: [paragraph("done thing")] }],
        },
      ],
    });
    const li = element.querySelector("li");
    expect(li?.textContent).toBe("done thing");
    const checkbox = li?.querySelector<HTMLInputElement>('input[type="checkbox"]');
    expect(checkbox?.checked).toBe(true);
    expect(checkbox?.nextElementSibling?.classList.contains("dit-li-content")).toBe(true);
  });

  it("marks a done item so the stylesheet can strike it through", () => {
    const element = mount(list({ task: "x", text: "done" }, { task: false, text: "open" }));
    const [done, open] = Array.from(element.querySelectorAll("li"));
    expect(done?.hasAttribute("data-checked")).toBe(true);
    expect(open?.hasAttribute("data-checked")).toBe(false);
  });

  it("starts another open to-do when Enter is pressed in a to-do", () => {
    const { editor } = mountEditor(list({ task: false, text: "buy milk" }));
    enterAtEnd(editor);
    expect(tasks(editor)).toEqual([false, false]);
  });

  it("starts an open to-do, not a done one, after a done to-do", () => {
    const { editor } = mountEditor(list({ task: "x", text: "paid rent" }));
    enterAtEnd(editor);
    expect(tasks(editor)).toEqual(["x", false]);
  });

  it("keeps a plain bullet plain when Enter is pressed", () => {
    const { editor } = mountEditor(list({ task: null, text: "a point" }));
    enterAtEnd(editor);
    expect(tasks(editor)).toEqual([null, null]);
  });

  it.each(["[] ", "[ ] ", "[x] "])("turns %j typed on an empty line into a to-do", (typed) => {
    const { editor } = mountEditor({ type: "doc", content: [{ type: "paragraph" }] });
    editor.commands.setTextSelection(1);
    type(editor, typed);
    expect(tasks(editor)).toEqual([typed === "[x] " ? "x" : false]);
    expect(editor.state.doc.textContent).toBe("");
  });
});

describe("soft line breaks", () => {
  it("show as a line break, not as a marker glyph on the same line", () => {
    const { element } = mountEditor({
      type: "doc",
      content: [
        {
          type: "paragraph",
          content: [
            { type: "text", text: "first line" },
            { type: "hardBreak", attrs: { soft: true } },
            { type: "text", text: "second line" },
          ],
        },
      ],
    });
    const br = element.querySelector("p br");
    expect(br).not.toBeNull();
    expect(element.querySelector(".dit-softbreak")?.tagName).toBe("BR");
  });

  it("stay soft when the editor parses its own HTML back", () => {
    const { editor } = mountEditor({ type: "doc", content: [{ type: "paragraph" }] });
    editor.commands.setContent('<p>a<br class="dit-softbreak">b<br>c</p>');
    const breaks: unknown[] = [];
    editor.state.doc.descendants((node) => {
      if (node.type.name === "hardBreak") breaks.push(node.attrs.soft);
    });
    expect(breaks).toEqual([true, false]);
  });
});
