// The "/" menu: blocks and inserts, rendered with cmdk in the same visual
// language as the command palette. Human-first affordance — nobody should
// need to know markdown syntax to add a table.

import { forwardRef, useEffect, useImperativeHandle, useState } from "react";
import { Command } from "cmdk";
import type { Editor, Range } from "@tiptap/core";
import { Extension } from "@tiptap/core";
import Suggestion, { exitSuggestion, type SuggestionProps } from "@tiptap/suggestion";
import { ReactRenderer } from "@tiptap/react";
import {
  Code2,
  Heading1,
  Heading2,
  Heading3,
  Image as ImageIcon,
  Link2,
  List,
  ListOrdered,
  ListTodo,
  Minus,
  Quote,
  Table,
  Type,
  Workflow,
} from "lucide-react";

type SlashItem = {
  label: string;
  keywords: string;
  icon: typeof Type;
  command: (args: { editor: Editor; range: Range }) => void;
};

const ask = (prompt: string, fallback = "") =>
  typeof window === "undefined" ? fallback : window.prompt(prompt) ?? fallback;

const ITEMS: SlashItem[] = [
  {
    label: "Text",
    keywords: "paragraph plain body text",
    icon: Type,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).setParagraph().run(),
  },
  {
    label: "Heading 1",
    keywords: "title h1 big heading",
    icon: Heading1,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleHeading({ level: 1 }).run(),
  },
  {
    label: "Heading 2",
    keywords: "subtitle h2 section heading",
    icon: Heading2,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleHeading({ level: 2 }).run(),
  },
  {
    label: "Heading 3",
    keywords: "h3 subheading small heading",
    icon: Heading3,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleHeading({ level: 3 }).run(),
  },
  {
    label: "Bullet list",
    keywords: "unordered ul points bullets list",
    icon: List,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleBulletList().run(),
  },
  {
    label: "Numbered list",
    keywords: "ordered ol steps numbers list",
    icon: ListOrdered,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleOrderedList().run(),
  },
  {
    label: "To-do list",
    keywords: "task checkbox check items todo",
    icon: ListTodo,
    command: ({ editor, range }) =>
      editor
        .chain()
        .focus()
        .deleteRange(range)
        .toggleBulletList()
        .updateAttributes("listItem", { task: false })
        .run(),
  },
  {
    label: "Quote",
    keywords: "blockquote callout cite quote",
    icon: Quote,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleBlockquote().run(),
  },
  {
    label: "Code block",
    keywords: "code snippet fence pre dit:query",
    icon: Code2,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).toggleCodeBlock().run(),
  },
  {
    label: "Diagram",
    keywords: "diagram drawing svg figure visual architecture flowchart",
    icon: Workflow,
    command: ({ editor, range }) =>
      // A diagram is a fence whose bytes are SVG — paste or generate them,
      // and the block renders the drawing (ADR 0012).
      editor
        .chain()
        .focus()
        .deleteRange(range)
        .insertContent({
          type: "codeBlock",
          attrs: { language: "dit-diagram" },
          content: [],
        })
        .run(),
  },
  {
    label: "Table",
    keywords: "grid columns rows table gfm",
    icon: Table,
    command: ({ editor, range }) =>
      editor
        .chain()
        .focus()
        .deleteRange(range)
        // The shape plugin pins row 0 as the header and pads `alignments`.
        .insertTable({ rows: 3, cols: 3, withHeaderRow: true })
        .updateAttributes("table", { alignments: ["none", "none", "none"] })
        .run(),
  },
  {
    label: "Divider",
    keywords: "hr rule separator line horizontal",
    icon: Minus,
    command: ({ editor, range }) =>
      editor.chain().focus().deleteRange(range).setHorizontalRule().run(),
  },
  {
    label: "Image",
    keywords: "picture photo img upload image",
    icon: ImageIcon,
    command: ({ editor, range }) => {
      const chain = editor.chain().focus().deleteRange(range);
      const src = ask("Image URL");
      if (!src) {
        chain.run();
        return;
      }
      const alt = ask("Alt text (optional)");
      chain
        .insertContent({
          type: "image",
          attrs: { src, title: "" },
          content: alt ? [{ type: "text", text: alt }] : [],
        })
        .run();
    },
  },
  {
    label: "Link",
    keywords: "url href anchor link",
    icon: Link2,
    command: ({ editor, range }) => {
      const href = ask("Link URL");
      if (!href) return;
      const chain = editor.chain().focus().deleteRange(range);
      const { empty } = editor.state.selection;
      if (empty) {
        chain
          .insertContent([
            { type: "text", text: href, marks: [{ type: "link", attrs: { href, title: "" } }] },
          ])
          .run();
      } else {
        chain.setLink({ href }).run();
      }
    },
  },
];

function matching(query: string): SlashItem[] {
  const q = query.toLowerCase().trim();
  if (!q) return ITEMS;
  return ITEMS.filter(
    (item) =>
      item.label.toLowerCase().includes(q) ||
      item.keywords.split(" ").some((k) => k.startsWith(q)),
  );
}

export type SlashListHandle = { onKeyDown: (event: KeyboardEvent) => boolean };

const SlashList = forwardRef<SlashListHandle, SuggestionProps<SlashItem>>(
  function SlashList(props, ref) {
    const [items, setItems] = useState<SlashItem[]>(() => matching(props.query));
    const [selected, setSelected] = useState(0);

    useEffect(() => {
      setItems(matching(props.query));
      setSelected(0);
    }, [props.query]);

    const run = (item: SlashItem) => props.command(item);

    useImperativeHandle(ref, () => ({
      // The editor still owns the keyboard; the menu only claims the keys
      // that move within it.
      onKeyDown: (event) => {
        if (event.key === "ArrowDown") {
          setSelected((s) => (items.length === 0 ? 0 : (s + 1) % items.length));
          return true;
        }
        if (event.key === "ArrowUp") {
          setSelected((s) => (items.length === 0 ? 0 : (s - 1 + items.length) % items.length));
          return true;
        }
        if (event.key === "Enter") {
          if (items[selected]) run(items[selected]);
          return true;
        }
        return false;
      },
    }));

    return (
      <div className="dit-slash">
        <Command shouldFilter={false} loop>
          <Command.List className="dit-slash-list">
            <Command.Empty className="dit-slash-empty">No blocks match.</Command.Empty>
            {items.map((item, index) => {
              const Icon = item.icon;
              return (
                <Command.Item
                  key={item.label}
                  value={item.label}
                  // Mouse clicks run the command; Enter goes through onKeyDown.
                  onSelect={() => run(item)}
                  data-selected={index === selected || undefined}
                  className="dit-slash-item"
                  onMouseEnter={() => setSelected(index)}
                >
                  <Icon className="size-4 shrink-0 text-zinc-500" aria-hidden />
                  {item.label}
                </Command.Item>
              );
            })}
          </Command.List>
        </Command>
      </div>
    );
  },
);

/** The '/' suggestion plugin. Positioning is handled by @tiptap/suggestion. */
export const SlashMenu = Extension.create({
  name: "slashMenu",
  addProseMirrorPlugins() {
    return [
      Suggestion({
        editor: this.editor,
        char: "/",
        startOfLine: false,
        items: ({ query }) => matching(query),
        command: ({ editor, range, props }) => props.command({ editor, range }),
        placement: "bottom-start",
        render: () => {
          let component: ReactRenderer<SlashListHandle> | null = null;
          let unmount: (() => void) | undefined;
          return {
            onStart: (props) => {
              component = new ReactRenderer(SlashList, { props, editor: props.editor });
              unmount = props.mount?.(component.element);
            },
            onUpdate: (props) => component?.updateProps(props),
            onKeyDown: (props) => {
              if (props.event.key === "Escape") {
                // Exit the suggestion itself, so onExit cleans up and the
                // '/' text stays as plain text instead of re-arming a menu.
                exitSuggestion(props.view);
                return true;
              }
              return component?.ref?.onKeyDown(props.event) ?? false;
            },
            onExit: () => {
              unmount?.();
              unmount = undefined;
              component?.destroy();
              component = null;
            },
          };
        },
      }),
    ];
  },
});
