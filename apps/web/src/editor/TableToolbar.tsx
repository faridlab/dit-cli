// The table toolbar: rows and columns, shown above a table while the caret
// is in it. Before it, a table could be inserted from "/" but never grown
// or shrunk without switching to source mode.

import { useEditorState, type Editor } from "@tiptap/react";
import { BubbleMenu } from "@tiptap/react/menus";
import {
  ArrowDownToLine,
  ArrowLeftToLine,
  ArrowRightToLine,
  ArrowUpToLine,
  Columns3,
  Rows3,
  Trash2,
} from "lucide-react";

import { tableActions, type TableAction } from "./table";

function tableElement(editor: Editor): HTMLElement | null {
  const { node } = editor.view.domAtPos(editor.state.selection.from);
  const element = node instanceof HTMLElement ? node : node.parentElement;
  return element?.closest("table") ?? null;
}

function Action({ action, icon, danger = false }: { action: TableAction; icon: React.ReactNode; danger?: boolean }) {
  return (
    <button
      type="button"
      className="dit-table-btn"
      data-danger={danger || undefined}
      disabled={action.disabled}
      title={action.label}
      aria-label={action.label}
      onMouseDown={(event) => event.preventDefault()}
      onClick={action.run}
    >
      {icon}
      <span>{action.label}</span>
    </button>
  );
}

export function TableToolbar({ editor }: { editor: Editor }) {
  // The editor does not re-render React on every transaction; without this
  // the buttons would keep the state they had when the table was entered.
  useEditorState({ editor, selector: ({ editor: e }) => e.state });
  const actions = tableActions(editor);
  return (
    <BubbleMenu
      editor={editor}
      pluginKey="tableToolbar"
      shouldShow={({ editor: e, view, state }) =>
        view.hasFocus() && e.isEditable && state.selection.empty && e.isActive("table")
      }
      getReferencedVirtualElement={() => {
        const table = tableElement(editor);
        return table ? { getBoundingClientRect: () => table.getBoundingClientRect() } : null;
      }}
      options={{ placement: "top-start", offset: 6 }}
    >
      <div className="dit-bubble dit-table-bar" role="toolbar" aria-label="Table">
        <Rows3 className="dit-table-group size-3.5" aria-hidden />
        <Action action={actions.addRowBefore} icon={<ArrowUpToLine className="size-3.5" aria-hidden />} />
        <Action action={actions.addRowAfter} icon={<ArrowDownToLine className="size-3.5" aria-hidden />} />
        <Action action={actions.deleteRow} icon={<Trash2 className="size-3.5" aria-hidden />} danger />
        <span className="mx-1 h-4 w-px self-center bg-edge" aria-hidden />
        <Columns3 className="dit-table-group size-3.5" aria-hidden />
        <Action action={actions.addColumnBefore} icon={<ArrowLeftToLine className="size-3.5" aria-hidden />} />
        <Action action={actions.addColumnAfter} icon={<ArrowRightToLine className="size-3.5" aria-hidden />} />
        <Action action={actions.deleteColumn} icon={<Trash2 className="size-3.5" aria-hidden />} danger />
        <span className="mx-1 h-4 w-px self-center bg-edge" aria-hidden />
        <Action action={actions.deleteTable} icon={<Trash2 className="size-3.5" aria-hidden />} danger />
      </div>
    </BubbleMenu>
  );
}
