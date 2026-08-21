// The Board side pane plus the column-visibility state it owns. The board
// (main area) and the pane sit far apart in the tree, so the hidden-column
// set travels through a context the shell provides above both — visible
// columns are view state shared by two surfaces, not a property of either.

import { createContext, useContext, useMemo, useState, type ReactNode } from "react";
import { useBoard } from "../../lib/queries";
import { cn } from "../../lib/cn";
import { CheckSquare, SectionHeading } from "../chrome";
import { ErrorBox, Loading } from "../states";

interface BoardColumnsValue {
  hidden: ReadonlySet<string>;
  toggle: (id: string) => void;
  showAll: () => void;
}

const BoardColumnsContext = createContext<BoardColumnsValue | null>(null);

export function BoardColumnsProvider({ children }: { children: ReactNode }) {
  const [hidden, setHidden] = useState<ReadonlySet<string>>(new Set());
  const value = useMemo<BoardColumnsValue>(
    () => ({
      hidden,
      toggle: (id) =>
        setHidden((previous) => {
          const next = new Set(previous);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        }),
      showAll: () => setHidden(new Set()),
    }),
    [hidden],
  );
  return <BoardColumnsContext.Provider value={value}>{children}</BoardColumnsContext.Provider>;
}

export function useBoardColumns(): BoardColumnsValue {
  const value = useContext(BoardColumnsContext);
  if (value === null) {
    throw new Error("useBoardColumns must be used inside BoardColumnsProvider");
  }
  return value;
}

export function BoardPane() {
  const board = useBoard();
  const { hidden, toggle, showAll } = useBoardColumns();

  if (board.isPending) {
    return <Loading label="Loading board…" className="p-4" />;
  }
  if (board.isError) {
    return (
      <div className="p-2">
        <ErrorBox
          error={board.error}
          onRetry={() => void board.refetch()}
          title="Could not load the board"
        />
      </div>
    );
  }

  const columns = board.data.columns;
  const total = columns.reduce((sum, column) => sum + column.issues.length, 0);

  return (
    <div className="flex flex-col gap-4 p-3">
      <section>
        <div className="flex items-center gap-2 px-1 pb-2">
          <SectionHeading size="sm">Columns</SectionHeading>
          {hidden.size > 0 ? (
            <button
              type="button"
              onClick={showAll}
              className="ml-auto text-[11px] text-zinc-500 transition-colors hover:text-zinc-200"
            >
              Show all
            </button>
          ) : null}
        </div>
        {columns.map((column) => {
          const on = !hidden.has(column.id);
          const count = column.issues.length;
          const limit = column.wip_limit;
          const overLimit = limit !== null && count > limit;
          return (
            <button
              key={column.id}
              type="button"
              onClick={() => toggle(column.id)}
              title={on ? `Hide ${column.label}` : `Show ${column.label}`}
              className={cn(
                "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left transition-colors hover:bg-card",
                !on && "opacity-45",
              )}
            >
              <CheckSquare on={on} />
              <span className="truncate text-xs uppercase tracking-[0.05em] text-zinc-300">
                {column.label}
              </span>
              <span
                className={cn(
                  "ml-auto shrink-0 rounded px-1.5 py-px font-mono text-[10.5px] tabular-nums",
                  overLimit ? "bg-amber-950/60 text-amber-400" : "bg-edge text-zinc-500",
                )}
                title={
                  overLimit ? `WIP limit ${limit} exceeded` : limit !== null ? `WIP limit ${limit}` : undefined
                }
              >
                {count}
                {limit !== null ? `/${limit}` : ""}
              </span>
            </button>
          );
        })}
      </section>

      <p className="px-1 font-mono text-[10.5px] text-zinc-600">
        {total} issues · {columns.length - hidden.size} of {columns.length} columns shown
      </p>
    </div>
  );
}
