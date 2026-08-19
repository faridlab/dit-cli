// Kanban board: one column per workflow status, cards move by drag. The
// move is optimistic — the card follows the cursor into its new column and
// the PATCH is confirmed behind it; a failure snaps the board back.

import { memo, useState } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  closestCorners,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type CollisionDetection,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { useBoard, useMoveIssue } from "../lib/queries";
import type { BoardColumnDto, BoardIssueDto } from "../lib/types";
import { cn } from "../lib/cn";
import { AssigneeCircles, IssueHandle, LabelChips, PriorityDot, TypeBadge } from "../components/badges";
import { Empty, ErrorBox, Loading } from "../components/states";
import { energyOf, relativeTime } from "../lib/format";

const CARD_DRAG_PREFIX = "card:";
const COLUMN_DRAG_PREFIX = "col:";

// Prefer "what the pointer is inside of" so a card dropped over a column
// body lands in that column; fall back to corner distance when dragging
// fast between columns.
const boardCollision: CollisionDetection = (args) => {
  const within = pointerWithin(args);
  if (within.length > 0) return within;
  return closestCorners(args);
};

// Memoized because any board refresh re-renders every column: cards whose
// issue did not change (stable identity from the query cache) skip that
// work. Effective only because `onOpen` arrives as a useCallback.
const BoardCard = memo(function BoardCard({
  issue,
  onOpen,
}: {
  issue: BoardIssueDto;
  onOpen: (id: string) => void;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: `${CARD_DRAG_PREFIX}${issue.id}`,
  });

  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      onClick={() => onOpen(issue.id)}
      className={cn(
        "cursor-grab rounded-lg border border-edge bg-card p-3 transition-colors hover:border-dim",
        isDragging && "opacity-30",
      )}
    >
      <div className="flex items-center gap-2">
        <IssueHandle shortRef={issue.short_ref} number={issue.number} />
        <TypeBadge type={issue.type} />
        <PriorityDot priority={issue.priority} />
        <span className="ml-auto font-mono text-[10.5px] text-dim">
          {energyOf(issue.labels)}
        </span>
        {issue.estimate !== null ? (
          <span
            className="font-mono text-[10.5px] text-zinc-500"
            title="estimate"
          >
            {issue.estimate}
          </span>
        ) : null}
      </div>
      <p className="mt-2 line-clamp-2 text-sm leading-normal text-zinc-200">{issue.title}</p>
      <div className="mt-2.5 flex items-center justify-between gap-2">
        {/* The board DTO carries no epic or due date, so the footer shows
            what the endpoint actually has — everything else lives on the
            issue screen. */}
        <LabelChips labels={issue.labels} max={2} />
        <span className="flex shrink-0 items-center gap-2">
          <span className="text-[10px] text-zinc-600" title={issue.updated}>
            {relativeTime(issue.updated)}
          </span>
          <AssigneeCircles assignees={issue.assignees} />
        </span>
      </div>
    </div>
  );
});

function Column({
  column,
  onOpen,
}: {
  column: BoardColumnDto;
  onOpen: (id: string) => void;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `${COLUMN_DRAG_PREFIX}${column.id}` });
  const count = column.issues.length;
  const limit = column.wip_limit;
  const overLimit = limit !== null && count > limit;

  return (
    <section className="flex w-[262px] shrink-0 flex-col border-r border-card last:border-r-0">
      <header className="flex items-center gap-2 px-3.5 pb-2.5 pt-3.5">
        <h2 className="text-xs font-semibold uppercase tracking-[0.06em] text-zinc-400">
          {column.label}
        </h2>
        <span
          className={cn(
            "rounded-md px-[7px] py-px font-mono text-[10.5px] tabular-nums",
            overLimit ? "bg-amber-950/60 text-amber-400" : "bg-edge text-zinc-500",
          )}
          title={overLimit ? `WIP limit ${limit} exceeded` : limit !== null ? `WIP limit ${limit}` : undefined}
        >
          {count}
          {limit !== null ? `/${limit}` : ""}
        </span>
        {overLimit ? (
          <span className="text-[10.5px] text-amber-400">over WIP</span>
        ) : null}
      </header>
      <div
        ref={setNodeRef}
        className={cn(
          "flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto px-3.5 pb-3.5",
          isOver && "rounded-lg bg-card/40 ring-1 ring-inset ring-accent",
        )}
      >
        {column.issues.map((issue) => (
          <BoardCard key={issue.id} issue={issue} onOpen={onOpen} />
        ))}
        {column.issues.length === 0 ? (
          <p className="rounded-lg border border-dashed border-edge px-2 py-5 text-center text-[11.5px] text-dim">
            No issues
          </p>
        ) : null}
      </div>
    </section>
  );
}

export function BoardView({ onOpen }: { onOpen: (id: string) => void }) {
  const board = useBoard();
  const move = useMoveIssue();
  const [activeId, setActiveId] = useState<string | null>(null);
  // Four pixels of movement before a drag starts, so plain clicks still
  // open the issue.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  if (board.isPending) return <Loading label="Loading board…" />;
  if (board.isError) {
    return <ErrorBox error={board.error} onRetry={() => void board.refetch()} title="Could not load the board" />;
  }

  const columns = board.data.columns;
  const activeIssue = activeId
    ? columns.flatMap((column) => column.issues).find((issue) => `${CARD_DRAG_PREFIX}${issue.id}` === activeId)
    : undefined;

  const onDragStart = (event: DragStartEvent) => {
    setActiveId(String(event.active.id));
  };

  const onDragEnd = (event: DragEndEvent) => {
    setActiveId(null);
    const { active, over } = event;
    if (!over) return;

    const issueId = String(active.id).slice(CARD_DRAG_PREFIX.length);
    const overId = String(over.id);

    // The drop target is either a column or a card; a card means "where that
    // card lives", which is how every kanban feels intuitive.
    let targetStatus: string | null = null;
    if (overId.startsWith(COLUMN_DRAG_PREFIX)) {
      targetStatus = overId.slice(COLUMN_DRAG_PREFIX.length);
    } else if (overId.startsWith(CARD_DRAG_PREFIX)) {
      const overIssueId = overId.slice(CARD_DRAG_PREFIX.length);
      const home = columns.find((column) =>
        column.issues.some((issue) => issue.id === overIssueId),
      );
      targetStatus = home?.id ?? null;
    }

    if (!targetStatus) return;
    const currentColumn = columns.find((column) =>
      column.issues.some((issue) => issue.id === issueId),
    );
    if (currentColumn?.id === targetStatus) return;

    move.mutate({ id: issueId, status: targetStatus });
  };

  if (columns.length === 0) {
    return (
      <Empty
        title="This workspace has no workflow statuses"
        hint="The board needs at least one status in the workflow schema."
        className="flex-1 justify-center"
      />
    );
  }

  return (
    <DndContext
      collisionDetection={boardCollision}
      sensors={sensors}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      onDragCancel={() => setActiveId(null)}
    >
      <div className="flex min-h-0 flex-1 flex-col">
        <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
          <h1 className="text-lg font-semibold text-zinc-100">Board</h1>
          <span className="ml-auto text-[11.5px] text-dim">
            open an issue and change its status — the card moves here
          </span>
        </header>
        <div className="flex min-h-0 flex-1 items-stretch overflow-x-auto">
          {columns.map((column) => (
            <Column key={column.id} column={column} onOpen={onOpen} />
          ))}
        </div>
      </div>
      <DragOverlay>
        {activeIssue ? (
          <div className="w-[262px] rotate-1 rounded-lg border border-ctl bg-card p-3 opacity-90 shadow-2xl">
            <div className="flex items-center gap-2">
              <IssueHandle shortRef={activeIssue.short_ref} number={activeIssue.number} />
              <TypeBadge type={activeIssue.type} />
              <PriorityDot priority={activeIssue.priority} />
            </div>
            <p className="mt-2 text-sm leading-normal text-zinc-100">{activeIssue.title}</p>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}
