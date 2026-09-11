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
  const element = document.createElement("div");
  document.body.append(element);
  new Editor({ element, extensions: ditExtensions(), content });
  return element;
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
});
