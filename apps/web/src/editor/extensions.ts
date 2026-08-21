// The editor schema. This file mirrors `crates/dit-parse/src/prosemirror.rs`:
// every node name, mark name and attr here must round-trip through the Rust
// bridge unchanged — the bridge refuses what it does not know instead of
// guessing (DESIGN.md §17), so a mismatch is an error message in the user's
// face, not silent data loss.
//
// Four shapes deliberately deviate from TipTap's defaults:
// - listItem carries `task` (null | false | "x" | "X") instead of using
//   TaskList/TaskItem, because one GFM list may mix task and plain items.
// - tableRow carries `isHeader`, and cell node types must match their row —
//   comrak's header row is exactly the first row.
// - lists carry `tight`; a tight list with two paragraphs in one item
//   reparses merged (lazy continuation), so the shape plugin loosens it.
// - Underline does not exist (no CommonMark equivalent) and stays disabled.

import StarterKit from "@tiptap/starter-kit";
import { Placeholder } from "@tiptap/extensions";
import {
  Table,
  TableCell,
  TableHeader,
  TableRow,
} from "@tiptap/extension-table";
import { BulletList, ListItem, OrderedList } from "@tiptap/extension-list";
import Link from "@tiptap/extension-link";
import CodeBlock from "@tiptap/extension-code-block";
import HardBreak from "@tiptap/extension-hard-break";
import {
  Extension,
  InputRule,
  mergeAttributes,
  Node,
  type Editor,
} from "@tiptap/core";
import type { Node as PmNode } from "@tiptap/pm/model";
import type { NodeView } from "@tiptap/pm/view";
import { Plugin } from "@tiptap/pm/state";

import { SlashMenu } from "./SlashMenu";
import { sanitizeSvg } from "./sanitizeSvg";

// -- extensions of TipTap's own, adjusted to the bridge's attrs ---------------

const DitBulletList = BulletList.extend({
  addAttributes() {
    return { tight: { default: true } };
  },
  renderHTML({ node, HTMLAttributes }) {
    return ["ul", mergeAttributes(HTMLAttributes, node.attrs.tight ? { "data-tight": "" } : {}), 0];
  },
});

const DitOrderedList = OrderedList.extend({
  addAttributes() {
    return {
      tight: { default: true },
      start: {
        default: 1,
        parseHTML: (element) =>
          element.hasAttribute("start") ? Number.parseInt(element.getAttribute("start") ?? "1", 10) : 1,
      },
    };
  },
  renderHTML({ node, HTMLAttributes }) {
    return [
      "ol",
      mergeAttributes(
        HTMLAttributes,
        node.attrs.start !== 1 ? { start: node.attrs.start } : {},
        node.attrs.tight ? { "data-tight": "" } : {},
      ),
      0,
    ];
  },
});

/** `task`: null = plain item, false = `- [ ]`, "x"/"X" = `- [x]`. */
const DitListItem = ListItem.extend({
  addAttributes() {
    return { task: { default: null, keepOnSplit: false } };
  },
  addNodeView() {
    return ({ node, getPos, editor }) => {
      const item = document.createElement("li");
      const content = document.createElement("div");
      content.className = "dit-li-content";
      let checkbox: HTMLInputElement | null = null;
      let current = node;

      const checked = () => current.attrs.task === "x" || current.attrs.task === "X";
      const mountCheckbox = () => {
        if (checkbox !== null || current.attrs.task === null) return;
        checkbox = document.createElement("input");
        checkbox.type = "checkbox";
        checkbox.contentEditable = "false";
        checkbox.checked = checked();
        checkbox.addEventListener("change", () => {
          const pos = typeof getPos === "function" ? getPos() : undefined;
          if (typeof pos !== "number") return;
          editor.view.dispatch(
            editor.view.state.tr.setNodeMarkup(pos, undefined, {
              ...current.attrs,
              task: checkbox?.checked ? "x" : false,
            }),
          );
        });
        item.append(checkbox);
      };

      const sync = () => {
        item.className = current.attrs.task === null ? "" : "dit-task";
        mountCheckbox();
        if (checkbox !== null) checkbox.checked = checked();
        if (current.attrs.task === null && checkbox !== null) {
          checkbox.remove();
          checkbox = null;
        }
      };
      sync();

      return {
        dom: item,
        contentDOM: content,
        update: (updated) => {
          if (updated.type.name !== this.name) return false;
          current = updated;
          sync();
          return true;
        },
      };
    };
  },
  addInputRules() {
    return [
      // Typing `[ ] ` or `[x] ` at the start of a list item's paragraph
      // turns the item into a task item.
      new InputRule({
        find: /\[([ xX])\] $/,
        handler: ({ state, range, match, chain }) => {
          const $from = state.selection.$from;
          if (range.from !== $from.start()) return; // only at paragraph start
          for (let depth = $from.depth; depth > 0; depth -= 1) {
            const ancestor = $from.node(depth);
            if (ancestor.type.name !== "listItem") continue;
            const task = match[1] === " " ? false : "x";
            chain()
              .command(({ tr }) => {
                tr.setNodeMarkup($from.before(depth), undefined, { ...ancestor.attrs, task });
                return true;
              })
              .deleteRange({ from: range.from, to: range.to })
              .run();
            return;
          }
        },
      }),
    ];
  },
});

/** The `dit-diagram` editing surface (ADR 0012): the sanitized drawing on
 *  top, the literal source under it, one toggle between the two. Only the
 *  source is writable — the drawing is always a re-render of those bytes,
 *  never markup the editor keeps. The block stays an ordinary fenced code
 *  block to git and to the bridge; this view is dressing on the same
 *  storage. */
function DiagramView(
  name: string,
  editor: Editor,
  node: PmNode,
  getPos: () => number | undefined,
): NodeView {
  const wrap = document.createElement("div");
  wrap.className = "dit-diagram";

  const state = document.createElement("span");
  state.className = "dit-diagram-state";

  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "dit-diagram-btn";
  toggle.addEventListener("click", () => {
    setMode(wrap.dataset.mode === "preview" ? "source" : "preview");
  });

  const copy = document.createElement("button");
  copy.type = "button";
  copy.className = "dit-diagram-btn";
  copy.textContent = "Copy SVG";
  copy.addEventListener("click", () => {
    void navigator.clipboard
      .writeText(source())
      .then(() => {
        copy.textContent = "Copied";
        window.setTimeout(() => {
          copy.textContent = "Copy SVG";
        }, 1200);
      })
      .catch(() => {
        // No clipboard access — surface the bytes so they can be copied.
        setMode("source");
      });
  });

  const label = document.createElement("span");
  label.className = "dit-diagram-label";
  label.textContent = "Diagram";
  const bar = document.createElement("div");
  bar.className = "dit-diagram-bar";
  bar.append(label, state, toggle, copy);

  const canvas = document.createElement("div");
  canvas.className = "dit-diagram-canvas";

  const pre = document.createElement("pre");
  const code = document.createElement("code");
  code.className = "language-dit-diagram";
  pre.append(code);
  wrap.append(bar, canvas, pre);

  // The node this view was built for goes stale as the doc moves; the doc at
  // our position is the truth. Falls back to the mount-time node if the
  // position cannot be resolved.
  const source = (): string => {
    const pos = getPos();
    if (typeof pos !== "number") return node.textContent;
    return editor.state.doc.nodeAt(pos)?.textContent ?? node.textContent;
  };

  const setMode = (mode: "preview" | "source"): void => {
    wrap.dataset.mode = mode;
    toggle.textContent = mode === "preview" ? "Edit source" : "Show diagram";
    if (mode === "preview") {
      const result = sanitizeSvg(source());
      // A refusal falls back to the source below with the reason next to the
      // label — never a blank card and never the unsanitized drawing.
      state.textContent = result.ok ? "" : `not rendered — ${result.error}`;
      if (result.ok) canvas.innerHTML = result.svg;
    } else {
      state.textContent =
        source().trim().length === 0 ? "paste SVG source — it renders as a diagram" : "";
    }
  };

  // First paint follows the bytes: a valid drawing opens rendered, anything
  // else opens as source.
  setMode(sanitizeSvg(node.textContent).ok ? "preview" : "source");

  return {
    dom: wrap,
    contentDOM: code,
    update: (updated) => {
      if (updated.type.name !== name) return false;
      // A language change flips which surface the block needs; let the view
      // be rebuilt instead of morphing it in place.
      return updated.attrs.language === node.attrs.language;
    },
    // The bar and canvas are ours; only edits inside the code are the doc's.
    ignoreMutation: (mutation) => !code.contains(mutation.target),
    stopEvent: (event) =>
      event.target instanceof Element && (bar.contains(event.target) || canvas.contains(event.target)),
  };
}

/** A code block's `language` is the full info string (`` `dit:query` `` included). */
const DitCodeBlock = CodeBlock.extend({
  addAttributes() {
    return {
      language: {
        default: "",
        parseHTML: (element) => /language-([\w:.-]+)/.exec(element.className)?.[1] ?? "",
      },
    };
  },
  addNodeView() {
    return ({ node, editor, getPos }) => {
      if (node.attrs.language !== "dit-diagram") {
        // Every other fence renders exactly what the renderHTML path
        // produces — a node view is taken only so diagrams can differ.
        const pre = document.createElement("pre");
      const code = document.createElement("code");
      if (node.attrs.language) code.className = `language-${node.attrs.language}`;
      pre.append(code);
      return {
        dom: pre,
        contentDOM: code,
        update: (updated) => {
          if (updated.type.name !== this.name) return false;
          return updated.attrs.language === node.attrs.language;
        },
      };
    };
  },
});

/** One node, two comrak states: `soft` is a single newline inside a paragraph. */
const DitHardBreak = HardBreak.extend({
  addAttributes() {
    return { soft: { default: false, keepOnSplit: false } };
  },
  renderHTML({ node }) {
    // Rendered like DIT's server-side preview renders it — no line break —
    // with a faint marker so the byte is visible while editing.
    return node.attrs.soft ? ["span", { class: "dit-softbreak" }] : ["br"];
  },
});

/** Exactly the attrs the bridge reads: no target/rel/class to drift in. */
const DitLink = Link.extend({
  addAttributes() {
    return {
      href: {
        default: "",
        parseHTML: (element) => element.getAttribute("href") ?? "",
      },
      title: {
        default: "",
        parseHTML: (element) => element.getAttribute("title") ?? "",
      },
    };
  },
}).configure({ openOnClick: false, autolink: true, linkOnPaste: true });

/** A GFM table: column alignments live on the table, not per cell. */
const DitTable = Table.extend({
  addAttributes() {
    return { alignments: { default: [] } };
  },
});

const DitTableRow = TableRow.extend({
  addAttributes() {
    return { isHeader: { default: false, keepOnSplit: false } };
  },
});

/** A cell is exactly one paragraph — the bridge refuses anything richer. */
const DitTableCell = TableCell.extend({ content: "paragraph" });
const DitTableHeader = TableHeader.extend({ content: "paragraph" });

// -- nodes the bridge owns outright -------------------------------------------

/** Raw HTML blocks: verbatim bytes, never interpreted as markup. */
const HtmlBlock = Node.create({
  name: "htmlBlock",
  group: "block",
  atom: true,
  defining: true,
  addAttributes() {
    return { literal: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "div[data-html-block]" }];
  },
  renderHTML({ node }) {
    return ["div", { "data-html-block": node.attrs.literal }];
  },
  addNodeView() {
    return ({ node }) => {
      const wrap = document.createElement("div");
      wrap.className = "dit-htmlblock";
      const code = document.createElement("code");
      code.textContent = node.attrs.literal;
      const caption = document.createElement("span");
      caption.className = "dit-htmlblock-caption";
      caption.textContent = "raw HTML — kept as-is, never executed";
      wrap.append(code, caption);
      return {
        dom: wrap,
        update: (updated) => {
          if (updated.type.name !== this.name) return false;
          code.textContent = updated.attrs.literal;
          return true;
        },
      };
    };
  },
});

/** Inline raw HTML (`<br>` and friends): a muted pill of its own bytes. */
const HtmlInline = Node.create({
  name: "htmlInline",
  group: "inline",
  inline: true,
  atom: true,
  addAttributes() {
    return { literal: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "span[data-html-inline]" }];
  },
  renderHTML({ node }) {
    return ["span", { "data-html-inline": node.attrs.literal, class: "dit-htmlinline" }];
  },
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement("span");
      dom.className = "dit-htmlinline";
      dom.textContent = node.attrs.literal;
      return {
        dom,
        update: (updated) => {
          if (updated.type.name !== this.name) return false;
          dom.textContent = updated.attrs.literal;
          return true;
        },
      };
    };
  },
});

/** `[[target|label]]` — the label is inline content, like comrak has it. */
const WikiLink = Node.create({
  name: "wikiLink",
  group: "inline",
  inline: true,
  content: "inline*",
  addAttributes() {
    return { target: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "span[data-wikilink]" }];
  },
  renderHTML({ HTMLAttributes, node }) {
    return ["span", mergeAttributes(HTMLAttributes, { "data-wikilink": node.attrs.target, class: "dit-wikilink" }), 0];
  },
  addInputRules() {
    // Typing the closing `]]` of `[[target]]` or `[[target|label]]` turns
    // the whole thing into one wiki link node.
    return [
      new InputRule({
        find: /\[\[([^|\]]+)(?:\|([^\]]*))?\]\]$/,
        handler: ({ state, range, match, chain }) => {
          const target = match[1] ?? "";
          if (!target || state.selection.$from.depth === 0) return;
          const label = match[2] ?? target;
          chain()
            .deleteRange({ from: range.from, to: range.to })
            .insertContentAt(range.from, {
              type: this.name,
              attrs: { target },
              content: [{ type: "text", text: label }],
            })
            // Cursor after the node, not inside its label.
            .setTextSelection(range.from + target.length + 2)
            .run();
        },
      }),
    ];
  },
});

/** `![alt](src "title")` — alt text is inline content, like comrak has it. */
const DitImage = Node.create({
  name: "image",
  group: "inline",
  inline: true,
  content: "inline*",
  draggable: true,
  addAttributes() {
    return { src: { default: "" }, title: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "img[data-dit-image]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["img", mergeAttributes(HTMLAttributes, { "data-dit-image": "" })];
  },
  addNodeView() {
    return ({ node }) => {
      const dom = document.createElement("span");
      dom.className = "dit-image";
      const img = document.createElement("img");
      const sync = () => {
        img.src = node.attrs.src;
        img.alt = node.textContent;
        if (node.attrs.title) img.title = node.attrs.title;
      };
      sync();
      dom.append(img);
      // No contentDOM on purpose: the alt-text children ride along in the
      // document (and in the bytes) but are edited via source mode.
      return {
        dom,
        update: (updated) => {
          if (updated.type.name !== this.name) return false;
          node = updated;
          sync();
          return true;
        },
      };
    };
  },
});

// -- the shape normalizer ------------------------------------------------------

/**
 * Normalizes editor-authored documents into the shapes the markdown parser
 * can round-trip — the editor must not be able to author a document its own
 * save path will refuse or silently change on reload:
 *
 * - GFM's header row is exactly the first row: `isHeader` is pinned to the
 *   row index, and cell node types are converted to match. TipTap's
 *   toggle-header commands work on any row; GFM does not.
 * - A tight list with two paragraphs in one item serializes to lines that
 *   reparse as ONE paragraph (lazy continuation) — those lists go loose.
 * - Column alignments must cover every column: TipTap's own add-column
 *   commands know nothing of the attr.
 */
const DitShape = Extension.create({
  name: "ditShape",
  addProseMirrorPlugins() {
    const schema = this.editor.schema;
    return [
      new Plugin({
        appendTransaction: (transactions, _old, state) => {
          if (!transactions.some((t) => t.docChanged)) return null;
          const tr = state.tr;
          let fixed = false;

          state.doc.descendants((node, pos) => {
            switch (node.type.name) {
              case "bulletList":
              case "orderedList": {
                if (!node.attrs.tight) break;
                let merges = false;
                node.forEach((item) => {
                  if (merges || item.type.name !== "listItem") return;
                  let paragraphs = 0;
                  item.forEach((child) => {
                    if (child.type.name === "paragraph") paragraphs += 1;
                  });
                  if (paragraphs > 1) merges = true;
                });
                if (merges) {
                  tr.setNodeMarkup(pos, undefined, { ...node.attrs, tight: false });
                  fixed = true;
                }
                break;
              }
              case "table": {
                const width = node.childCount > 0 ? node.child(0).childCount : 0;
                if (node.attrs.alignments.length !== width) {
                  const alignments = Array.from(
                    { length: width },
                    (_, i) => node.attrs.alignments[i] ?? "none",
                  );
                  tr.setNodeMarkup(pos, undefined, { ...node.attrs, alignments });
                  fixed = true;
                }
                node.forEach((row, rowOffset, index) => {
                  const wantedHeader = index === 0;
                  if (row.attrs.isHeader !== wantedHeader) {
                    tr.setNodeMarkup(pos + rowOffset + 1, undefined, {
                      ...row.attrs,
                      isHeader: wantedHeader,
                    });
                    fixed = true;
                  }
                  const wantedCell =
                    wantedHeader ? schema.nodes.tableHeader : schema.nodes.tableCell;
                  row.forEach((cell, cellOffset) => {
                    if (cell.type !== wantedCell) {
                      tr.setNodeMarkup(pos + rowOffset + cellOffset + 2, wantedCell);
                      fixed = true;
                    }
                  });
                });
                break;
              }
            }
            return true;
          });

          return fixed ? tr : null;
        },
      }),
    ];
  },
});

export function ditExtensions() {
  return [
    DitShape,
    StarterKit.configure({
      // No CommonMark equivalent: allowing it would write bytes that lie.
      underline: false,
      // Appends paragraphs the user did not author.
      trailingNode: false,
      // Replaced below with the exact attrs the bridge reads.
      link: false,
      codeBlock: false,
      listItem: false,
      bulletList: false,
      orderedList: false,
      hardBreak: false,
      heading: { levels: [1, 2, 3, 4, 5, 6] },
    }),
    DitBulletList,
    DitOrderedList,
    DitListItem,
    DitCodeBlock,
    DitHardBreak,
    DitLink,
    DitTable,
    DitTableRow,
    DitTableCell,
    DitTableHeader,
    HtmlBlock,
    HtmlInline,
    WikiLink,
    DitImage,
    SlashMenu,
    Placeholder.configure({
      placeholder: ({ node }) =>
        node.type.name === "heading"
          ? "Heading"
          : node.type.name === "codeBlock"
            ? "code"
            : "Write something, or type '/' for commands",
    }),
  ];
}
