// The editor affordances that replaced syntax: the code block's language
// field and the "/" menu's Link and Image dialogs.
import { beforeAll, describe, expect, it } from "vitest";
import { Editor } from "@tiptap/core";
import { cleanLanguage, ditExtensions } from "./extensions";
import { insertImage, insertLink } from "./InsertDialog";

beforeAll(() => {
  const empty = () => Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getClientRects = empty;
  Range.prototype.getBoundingClientRect = () => new DOMRect(0, 0, 0, 0);
  Element.prototype.getClientRects = empty;
});

function mount(content: Array<Record<string, unknown>>): { editor: Editor; element: HTMLElement } {
  const element = document.createElement("div");
  document.body.append(element);
  const editor = new Editor({ element, extensions: ditExtensions(), content: { type: "doc", content } });
  return { editor, element };
}

describe("code block language", () => {
  it("shows the fence's language in an editable field", () => {
    const { element } = mount([
      { type: "codeBlock", attrs: { language: "rust" }, content: [{ type: "text", text: "fn main() {}" }] },
    ]);
    const field = element.querySelector<HTMLInputElement>(".dit-code-lang");
    expect(field?.value).toBe("rust");
    expect(element.querySelector("code")?.textContent).toBe("fn main() {}");
  });

  it("writes a changed language back to the block", () => {
    const { editor, element } = mount([{ type: "codeBlock", attrs: { language: "" }, content: [{ type: "text", text: "x" }] }]);
    const field = element.querySelector<HTMLInputElement>(".dit-code-lang");
    if (!field) throw new Error("no language field");
    field.value = " yaml ";
    field.dispatchEvent(new Event("change"));
    expect(editor.state.doc.child(0).attrs.language).toBe("yaml");
    expect(editor.state.doc.child(0).textContent).toBe("x");
  });

  it.each([
    ["dit:query", "dit:query"],
    ["c++", "c++"],
    ["  py  ", "py"],
    ["js`evil", "jsevil"],
    ["two words", "twowords"],
  ])("keeps %j as %j", (typed, kept) => expect(cleanLanguage(typed)).toBe(kept));
});

describe("insert dialogs", () => {
  const typedSlash = (editor: Editor) => {
    editor.commands.setContent({ type: "doc", content: [{ type: "paragraph", content: [{ type: "text", text: "see /" }] }] });
    return { from: 5, to: 6 };
  };

  it("replaces the typed '/' with a link, labelled by the address when no text is given", () => {
    const { editor } = mount([{ type: "paragraph" }]);
    insertLink(editor, typedSlash(editor), "https://example.com", "");
    const json = JSON.stringify(editor.getJSON());
    expect(editor.state.doc.textContent).toBe("see https://example.com");
    expect(json).toContain('"href":"https://example.com"');
  });

  it("uses the given text as the link's label", () => {
    const { editor } = mount([{ type: "paragraph" }]);
    insertLink(editor, typedSlash(editor), "docs/plan.md", "the plan");
    expect(editor.state.doc.textContent).toBe("see the plan");
  });

  it("inserts an image with its description as alt text", () => {
    const { editor } = mount([{ type: "paragraph" }]);
    insertImage(editor, typedSlash(editor), "docs/assets/flow.png", "the flow");
    let image: Record<string, unknown> | null = null;
    editor.state.doc.descendants((node) => {
      if (node.type.name === "image") image = { src: node.attrs.src, alt: node.textContent };
    });
    expect(image).toEqual({ src: "docs/assets/flow.png", alt: "the flow" });
  });
});
