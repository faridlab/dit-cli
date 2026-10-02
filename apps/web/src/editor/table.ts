// What the table toolbar can do to the table under the caret. TipTap's
// table commands do the cell surgery; this keeps GFM's per-column
// alignments attached to their columns and refuses the shapes GFM cannot
// hold (a row above the header, a table with no columns).

import type { Editor } from "@tiptap/core";

export type TableAction = { label: string; disabled: boolean; run: () => void };

type Where = { rowIndex: number; columnIndex: number; columns: number; alignments: string[] };

/** Where the caret sits in its table, or null outside one. */
function where(editor: Editor): Where | null {
  const { $from } = editor.state.selection;
  let rowIndex = -1;
  let columnIndex = -1;
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    const node = $from.node(depth);
    if (node.type.name === "tableCell" || node.type.name === "tableHeader") {
      columnIndex = $from.index(depth - 1);
    } else if (node.type.name === "tableRow") {
      rowIndex = $from.index(depth - 1);
    } else if (node.type.name === "table") {
      const columns = node.childCount > 0 ? node.child(0).childCount : 0;
      const alignments = Array.from(
        { length: columns },
        (_, i) => (node.attrs.alignments as string[])[i] ?? "none",
      );
      return rowIndex < 0 || columnIndex < 0 ? null : { rowIndex, columnIndex, columns, alignments };
    }
  }
  return null;
}

/** Run a column command, then lay the alignments out the way the columns
 *  now stand. Runs as one chain, so undo takes back both. */
function withAlignments(editor: Editor, next: string[], command: "addColumnBefore" | "addColumnAfter" | "deleteColumn") {
  editor.chain().focus()[command]().updateAttributes("table", { alignments: next }).run();
}

export function tableActions(editor: Editor): Record<
  "addRowBefore" | "addRowAfter" | "deleteRow" | "addColumnBefore" | "addColumnAfter" | "deleteColumn" | "deleteTable",
  TableAction
> {
  const at = where(editor);
  const inHeader = at?.rowIndex === 0;
  const none = at === null;
  return {
    addRowBefore: {
      label: "Row above",
      disabled: none || inHeader,
      run: () => void editor.chain().focus().addRowBefore().run(),
    },
    addRowAfter: {
      label: "Row below",
      disabled: none,
      run: () => void editor.chain().focus().addRowAfter().run(),
    },
    deleteRow: {
      label: "Delete row",
      disabled: none || inHeader,
      run: () => void editor.chain().focus().deleteRow().run(),
    },
    addColumnBefore: {
      label: "Column left",
      disabled: none,
      run: () => {
        if (!at) return;
        const next = [...at.alignments];
        next.splice(at.columnIndex, 0, "none");
        withAlignments(editor, next, "addColumnBefore");
      },
    },
    addColumnAfter: {
      label: "Column right",
      disabled: none,
      run: () => {
        if (!at) return;
        const next = [...at.alignments];
        next.splice(at.columnIndex + 1, 0, "none");
        withAlignments(editor, next, "addColumnAfter");
      },
    },
    deleteColumn: {
      label: "Delete column",
      disabled: none || (at?.columns ?? 0) <= 1,
      run: () => {
        if (!at) return;
        const next = [...at.alignments];
        next.splice(at.columnIndex, 1);
        withAlignments(editor, next, "deleteColumn");
      },
    },
    deleteTable: {
      label: "Delete table",
      disabled: none,
      run: () => void editor.chain().focus().deleteTable().run(),
    },
  };
}
