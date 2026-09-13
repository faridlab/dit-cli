// The board: one column per group, cards move by drag. The grouping is a
// view option (status, assignee, epic or @context) shared with the sidebar
// section and the header's Display menu, so this file owns the column model
// — which issues land where, in what order, and what a drop or a quick add
// has to write — and both surfaces read it from here.
//
// Cards come from the plain issue list rather than /api/board: the board
// endpoint has no due date, epic or start, and the non-status groupings
// need every field anyway. A move is optimistic — the card stays in its new
// column until the list has refetched, and snaps back if the PATCH fails.

import { memo, useEffect, useMemo, useState } from "react";
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
import { Copy, EyeOff, MoreHorizontal, Plus, Search, X } from "lucide-react";
import { toast } from "sonner";
import {
  useBulkPatchIssue,
  useCreateIssue,
  useIssues,
  useMoveIssue,
  useSchema,
  useStatus,
} from "../lib/queries";
import { useViewOptions, type BoardGroupBy, type BoardOptions, type CardSort } from "../lib/viewopts";
import { contextsOf, doneIds, filtersToDql, isDone, matchesFilters, POOL_LIMIT, type ListFilters } from "../lib/lists";
import { contextOf } from "../lib/format";
import { navigate, routeToHash, useRoute } from "../lib/router";
import { useRegisterPeekList } from "../lib/peeklist";
import type { FieldPatch, IssueDto, NewIssueInput, StatusCategory, StatusDto } from "../lib/types";
import { cn } from "../lib/cn";
import { AssigneeCircles, Chip, Due, IssueHandle, PriorityDot, StatusPill, TypeBadge } from "../components/badges";
import { IBtn, MenuButton, Sp, type MenuItem } from "../components/chrome";
import { Empty, ErrorBox, Loading } from "../components/states";

const CARD_DRAG_PREFIX = "card:";
const COLUMN_DRAG_PREFIX = "col:";

/** One column of the board, whatever the grouping. */
export interface BoardColumn {
  key: string;
  label: string;
  /** Paints the header pill: the workflow category, or todo/doing for the
   *  catch-all and regular columns of the other groupings. */
  cat: StatusCategory;
  wip: number | null;
  /** The workflow status behind the column, when grouped by status. */
  status: StatusDto | null;
  /** Every issue the column holds, before the header filters. */
  all: IssueDto[];
  /** What the column shows: filtered, then ordered. */
  cards: IssueDto[];
  /** The fields a title typed into this column starts with. */
  create: Partial<NewIssueInput>;
  /** Said in the toast when the quick add could not put the issue where
   *  it was typed. */
  createNote: string | null;
  /** The patch that lands `issue` in this column; null when the API has no
   *  field for it, which disables dragging into the column. */
  landing: ((issue: IssueDto) => FieldPatch) | null;
}

const EPIC_NOTE = "the API cannot set an issue's epic yet, so it landed under “No epic”";

function cardOrder(sort: CardSort): (a: IssueDto, b: IssueDto) => number {
  if (sort === "updated") return (a, b) => Date.parse(b.updated) - Date.parse(a.updated);
  if (sort === "due") return (a, b) => (a.due ?? "9").localeCompare(b.due ?? "9");
  return (a, b) => (a.priority ?? "p9").localeCompare(b.priority ?? "p9");
}

/** Every `context:*` label replaced by the one context (or none). */
function withContext(labels: readonly string[], context: string | null): string[] {
  const kept = labels.filter((label) => !label.startsWith("context:"));
  return context === null ? kept : [...kept, `context:${context}`];
}

/** The columns for one grouping, over the pool of non-epic issues. */
export function buildColumns(
  issues: readonly IssueDto[],
  statuses: readonly StatusDto[],
  board: BoardOptions,
  filters: ListFilters,
  me: string | null,
): BoardColumn[] {
  const done = doneIds(statuses);
  const pool = issues.filter((issue) => issue.type !== "story");
  const open = (issue: IssueDto) => !isDone(issue, done);
  type Spec = Omit<BoardColumn, "all" | "cards"> & { pick: (issue: IssueDto) => boolean };
  let specs: Spec[] = [];

  if (board.groupBy === "status") {
    specs = statuses.map((status) => ({
      key: status.id,
      label: status.label,
      cat: status.category,
      wip: status.wip_limit,
      status,
      pick: (issue) => issue.status === status.id,
      create: { status: status.id },
      createNote: null,
      landing: () => ({ status: status.id }),
    }));
  } else if (board.groupBy === "assignee") {
    const aliases = [...new Set(pool.flatMap((issue) => issue.assignees))].sort();
    specs = [
      ...aliases.map(
        (alias): Spec => ({
          key: `a:${alias}`,
          label: alias,
          cat: "doing",
          wip: null,
          status: null,
          pick: (issue) => issue.assignees.includes(alias) && open(issue),
          create: { assignees: [alias] },
          createNote: null,
          landing: (issue) => ({ assignees: [alias, ...issue.assignees.filter((a) => a !== alias)] }),
        }),
      ),
      {
        key: "a:none",
        label: "Unassigned",
        cat: "todo",
        wip: null,
        status: null,
        pick: (issue) => issue.assignees.length === 0 && open(issue),
        create: {},
        createNote: null,
        landing: () => ({ assignees: [] }),
      },
    ];
  } else if (board.groupBy === "epic") {
    const epics = issues.filter((issue) => issue.type === "story" && issue.epic === null);
    specs = [
      ...epics.map(
        (epic): Spec => ({
          key: `e:${epic.id}`,
          label: epic.title,
          cat: "doing",
          wip: null,
          status: null,
          pick: (issue) => issue.epic === epic.id && open(issue),
          create: {},
          createNote: EPIC_NOTE,
          landing: null,
        }),
      ),
      {
        key: "e:none",
        label: "No epic",
        cat: "todo",
        wip: null,
        status: null,
        pick: (issue) => issue.epic === null && open(issue),
        create: {},
        createNote: null,
        landing: null,
      },
    ];
  } else {
    specs = [
      ...contextsOf(pool).map(
        (context): Spec => ({
          key: `c:${context}`,
          label: `@${context}`,
          cat: "doing",
          wip: null,
          status: null,
          pick: (issue) => contextOf(issue.labels) === context && open(issue),
          create: { labels: [`context:${context}`] },
          createNote: null,
          landing: (issue) => ({ labels: withContext(issue.labels, context) }),
        }),
      ),
      {
        key: "c:none",
        label: "No context",
        cat: "todo",
        wip: null,
        status: null,
        pick: (issue) => contextOf(issue.labels) === null && open(issue),
        create: {},
        createNote: null,
        landing: (issue) => ({ labels: withContext(issue.labels, null) }),
      },
    ];
  }

  const order = cardOrder(board.colSort);
  return specs.map(({ pick, ...spec }) => {
    const all = pool.filter(pick);
    return { ...spec, all, cards: all.filter((issue) => matchesFilters(issue, filters, me)).sort(order) };
  });
}

/** A move the list has not caught up with yet: the card is drawn where it
 *  was dropped until the refetch after the PATCH confirms it. */
interface PendingMove {
  patch: FieldPatch;
  settled: boolean;
}

const NO_MOVES: ReadonlyMap<string, PendingMove> = new Map();

function applyPending(issue: IssueDto, patch: FieldPatch): IssueDto {
  return {
    ...issue,
    status: patch.status ?? issue.status,
    assignees: patch.assignees ?? issue.assignees,
    labels: patch.labels ?? issue.labels,
  };
}

/** The column model both the board and its sidebar section draw from. */
export function useBoardModel(pending: ReadonlyMap<string, PendingMove> = NO_MOVES) {
  const issues = useIssues({ limit: POOL_LIMIT });
  const schema = useSchema();
  const status = useStatus();
  const { board, filters } = useViewOptions();
  const me = status.data?.me ?? null;
  const statuses = schema.data?.workflow.statuses;

  const items = useMemo(() => {
    const all = issues.data?.items ?? [];
    if (pending.size === 0) return all;
    return all.map((issue) => {
      const move = pending.get(issue.id);
      return move ? applyPending(issue, move.patch) : issue;
    });
  }, [issues.data, pending]);

  const columns = useMemo(
    () => buildColumns(items, statuses ?? [], board, filters, me),
    [items, statuses, board, filters, me],
  );
  const statusById = useMemo(() => new Map((statuses ?? []).map((s) => [s.id, s])), [statuses]);
  const titleById = useMemo(() => new Map(items.map((issue) => [issue.id, issue.title])), [items]);

  return { issues, schema, columns, statusById, titleById, me, items };
}

// Prefer "what the pointer is inside of" so a card dropped over a column
// body lands in that column; fall back to corner distance when dragging
// fast between columns.
const boardCollision: CollisionDetection = (args) => {
  const within = pointerWithin(args);
  if (within.length > 0) return within;
  return closestCorners(args);
};

const EPIC_DRAG_TITLE = "Cards stay put while grouped by epic — the API cannot set an issue's epic yet";

function CardBody({
  issue,
  groupBy,
  cards,
  status,
  epicTitle,
}: {
  issue: IssueDto;
  groupBy: BoardGroupBy;
  cards: BoardOptions["cards"];
  status: StatusDto | undefined;
  epicTitle: string | null;
}) {
  return (
    <>
      <div className="top">
        <IssueHandle shortRef={issue.short_ref} number={issue.number} />
        <TypeBadge type={issue.type} />
        <PriorityDot priority={issue.priority} />
        {groupBy !== "status" ? <StatusPill status={status} label={issue.status} /> : null}
        <Sp />
        {cards.due ? <Due iso={issue.due} /> : null}
      </div>
      <div className="title">{issue.title}</div>
      {cards.epic && epicTitle !== null ? (
        <div style={{ fontSize: 11.5, color: "var(--muted)", display: "flex", gap: 5, alignItems: "center" }}>
          <TypeBadge type="story" />
          {epicTitle}
        </div>
      ) : null}
      <div className="bot">
        {cards.labels
          ? issue.labels
              .filter((label) => !label.startsWith("energy:"))
              .slice(0, 2)
              .map((label) => <Chip key={label} label={label} />)
          : null}
        <Sp />
        {issue.estimate ? (
          <span className="est" title="estimate">
            {issue.estimate}pt
          </span>
        ) : null}
        <AssigneeCircles assignees={issue.assignees} />
      </div>
    </>
  );
}

// Memoized because any board refresh re-renders every column: cards whose
// issue did not change (stable identity from the query cache) skip that
// work. Effective only because `onOpen` arrives as a useCallback.
const Card = memo(function Card({
  issue,
  groupBy,
  cards,
  status,
  epicTitle,
  selected,
  draggable,
  onOpen,
}: {
  issue: IssueDto;
  groupBy: BoardGroupBy;
  cards: BoardOptions["cards"];
  status: StatusDto | undefined;
  epicTitle: string | null;
  selected: boolean;
  draggable: boolean;
  onOpen: (id: string) => void;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: `${CARD_DRAG_PREFIX}${issue.id}`,
    disabled: !draggable,
  });
  return (
    <button
      type="button"
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      title={draggable ? undefined : EPIC_DRAG_TITLE}
      onClick={() => onOpen(issue.short_ref)}
      className={cn("card", selected && "sel", isDragging && "opacity-30")}
      data-id={issue.short_ref}
    >
      <CardBody issue={issue} groupBy={groupBy} cards={cards} status={status} epicTitle={epicTitle} />
    </button>
  );
});

/** The row at the end of every column, open: a title in, an issue in this
 *  column out. Stays open after a create so capturing a burst of issues
 *  never re-opens anything; Escape or leaving it empty closes it. */
function QuickAdd({ column, onClose }: { column: BoardColumn; onClose: () => void }) {
  const [title, setTitle] = useState("");
  const create = useCreateIssue();

  const submit = () => {
    const trimmed = title.trim();
    if (trimmed.length === 0 || create.isPending) return;
    create.mutate(
      { title: trimmed, type: "task", body: "", ...column.create },
      {
        onSuccess: (created) => {
          setTitle("");
          const handle = created.number !== null ? `#${created.number}` : created.short_ref;
          toast(
            column.createNote
              ? `Created ${handle} — ${column.createNote}`
              : `Created ${handle} in ${column.label}`,
          );
        },
      },
    );
  };

  return (
    <div className="qadd open">
      <Plus className="i" aria-hidden />
      <input
        autoFocus
        value={title}
        onChange={(event) => setTitle(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            submit();
          } else if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
        // Blur with words stays put — a stray click never discards a title;
        // blur empty collapses back to the affordance.
        onBlur={() => {
          if (title.trim().length === 0) onClose();
        }}
        placeholder="Title, then Enter — stays open for the next one"
        aria-label={`New issue in ${column.label}`}
        disabled={create.isPending}
      />
    </div>
  );
}

function Column({
  column,
  groupBy,
  cards,
  statusById,
  titleById,
  selectedRef,
  quickAddOpen,
  onQuickAdd,
  onCloseQuickAdd,
  onOpen,
}: {
  column: BoardColumn;
  groupBy: BoardGroupBy;
  cards: BoardOptions["cards"];
  statusById: ReadonlyMap<string, StatusDto>;
  titleById: ReadonlyMap<string, string>;
  selectedRef: string | null;
  quickAddOpen: boolean;
  onQuickAdd: (key: string) => void;
  onCloseQuickAdd: () => void;
  onOpen: (id: string) => void;
}) {
  const droppable = column.landing !== null;
  const { setNodeRef, isOver } = useDroppable({
    id: `${COLUMN_DRAG_PREFIX}${column.key}`,
    disabled: !droppable,
  });
  const { hideColumn } = useViewOptions();
  const count = column.cards.length;
  const wip = column.wip;
  const atLimit = wip !== null && count >= wip;

  const copyCards = () => {
    const text = column.cards
      .map((issue) => `- [ ] ${issue.number !== null ? `#${issue.number}` : issue.short_ref} ${issue.title}`)
      .join("\n");
    navigator.clipboard.writeText(text).then(
      () => toast("List copied"),
      () => toast.error("Could not reach the clipboard"),
    );
  };

  const statusItems: MenuItem[] =
    column.status !== null
      ? [
          { kind: "sep" },
          {
            label: `Run: status = ${column.status.id}`,
            icon: <Search className="i" aria-hidden />,
            run: () => navigate({ name: "search", q: `status = ${column.status?.id ?? ""}` }),
          },
        ]
      : [];
  const menu: MenuItem[] = [
    { kind: "head", label: column.label },
    { label: "Hide column", icon: <EyeOff className="i" aria-hidden />, run: () => hideColumn(column.key) },
    { label: "Add issue here", icon: <Plus className="i" aria-hidden />, run: () => onQuickAdd(column.key) },
    ...statusItems,
    { kind: "sep" },
    { label: `Copy ${count} cards as markdown list`, icon: <Copy className="i" aria-hidden />, run: copyCards },
  ];

  return (
    <section className="col" data-col={column.key}>
      <div className="col-h">
        {column.status !== null ? (
          <StatusPill status={column.status} />
        ) : (
          <StatusPill category={column.cat} label={column.label} />
        )}
        <span className="cnt">{count}</span>
        {wip !== null ? (
          <span
            className="wip"
            title={`WIP limit ${wip}${atLimit ? " — at the limit" : ""}`}
            style={atLimit ? undefined : { color: "var(--muted)", background: "var(--sunken)" }}
          >
            {count}/{wip}
          </span>
        ) : null}
        <Sp />
        <IBtn title={`Add an issue to ${column.label}`} onClick={() => onQuickAdd(column.key)}>
          <Plus className="i" aria-hidden />
        </IBtn>
        <MenuButton items={menu} align="end">
          <IBtn title="Column menu">
            <MoreHorizontal className="i" aria-hidden />
          </IBtn>
        </MenuButton>
      </div>
      <div
        ref={setNodeRef}
        className="cards"
        style={isOver ? { background: "var(--hover)", borderRadius: 8 } : undefined}
      >
        {column.cards.map((issue) => (
          <Card
            key={issue.id}
            issue={issue}
            groupBy={groupBy}
            cards={cards}
            status={statusById.get(issue.status)}
            epicTitle={issue.epic !== null ? (titleById.get(issue.epic) ?? null) : null}
            selected={selectedRef === issue.short_ref}
            draggable={groupBy !== "epic"}
            onOpen={onOpen}
          />
        ))}
      </div>
      {quickAddOpen ? (
        <QuickAdd column={column} onClose={onCloseQuickAdd} />
      ) : (
        <button type="button" className="qadd" onClick={() => onQuickAdd(column.key)}>
          <Plus className="i" aria-hidden />
          New issue
        </button>
      )}
    </section>
  );
}

/** The header filters as chips, each removable on its own, plus the DQL
 *  they compose as a link to the Search page. Nothing when no filter is on. */
function FiltersBar() {
  const { filters, toggleMine, toggleContext, toggleType } = useViewOptions();
  const chips: Array<{ label: string; remove: () => void }> = [];
  if (filters.mine) chips.push({ label: "assignee = @me", remove: toggleMine });
  for (const context of [...filters.contexts].sort()) {
    chips.push({ label: `label = context:${context}`, remove: () => toggleContext(context) });
  }
  for (const type of [...filters.types].sort()) {
    chips.push({ label: `type = ${type}`, remove: () => toggleType(type) });
  }
  if (chips.length === 0) return null;
  const q = filtersToDql(filters).join(" and ");
  return (
    <div className="filters">
      {chips.map((chip) => (
        <span className="fchip" key={chip.label}>
          {chip.label}
          <button type="button" title="Remove this filter" onClick={chip.remove}>
            <X className="i" aria-hidden />
          </button>
        </span>
      ))}
      <a className="dq" href={routeToHash({ name: "search", q })} title="Run this exact query on the Search page">
        {q}
      </a>
    </div>
  );
}

export function BoardView({ onOpen }: { onOpen: (id: string) => void }) {
  const route = useRoute();
  const { board, showAllColumns } = useViewOptions();
  const [pending, setPending] = useState<ReadonlyMap<string, PendingMove>>(NO_MOVES);
  const { issues, schema, columns, statusById, titleById, items } = useBoardModel(pending);
  const move = useMoveIssue();
  const patch = useBulkPatchIssue();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [quickAdd, setQuickAdd] = useState<string | null>(null);
  // Four pixels of movement before a drag starts, so plain clicks still
  // open the issue.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  // Once the list has refetched after a confirmed move, the server's row
  // says the same thing as the pending patch; drop it so the two never
  // disagree later. A move still in flight keeps its card where it was
  // dropped even if an unrelated refresh lands first.
  useEffect(() => {
    setPending((current) => {
      if (current.size === 0) return current;
      const next = new Map(current);
      for (const [id, entry] of current) if (entry.settled) next.delete(id);
      return next.size === current.size ? current : next;
    });
  }, [issues.dataUpdatedAt]);

  const selectedRef = route.name === "board" ? (route.issue ?? null) : null;
  const visible = useMemo(() => columns.filter((column) => !board.hidden.has(column.key)), [columns, board.hidden]);

  // Column by column, top to bottom: the order the issue panel walks with
  // J/K. Hidden columns are not on screen, so they are not in it.
  useRegisterPeekList(
    useMemo(() => visible.flatMap((column) => column.cards.map((issue) => issue.short_ref)), [visible]),
  );

  if (issues.isPending || schema.isPending) return <Loading label="Loading board…" />;
  if (issues.isError) {
    return <ErrorBox error={issues.error} onRetry={() => void issues.refetch()} title="Could not load the board" />;
  }
  if (schema.isError) {
    return <ErrorBox error={schema.error} onRetry={() => void schema.refetch()} title="Could not load the workflow" />;
  }

  if (board.groupBy === "status" && columns.length === 0) {
    return (
      <Empty
        title="This workspace has no workflow statuses"
        hint="The board needs at least one status in the workflow schema."
        className="empty flex-1 justify-center"
      />
    );
  }

  const activeIssue =
    activeId !== null ? items.find((issue) => `${CARD_DRAG_PREFIX}${issue.id}` === activeId) : undefined;

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
    let target: BoardColumn | undefined;
    if (overId.startsWith(COLUMN_DRAG_PREFIX)) {
      const key = overId.slice(COLUMN_DRAG_PREFIX.length);
      target = visible.find((column) => column.key === key);
    } else if (overId.startsWith(CARD_DRAG_PREFIX)) {
      const overIssueId = overId.slice(CARD_DRAG_PREFIX.length);
      target = visible.find((column) => column.cards.some((issue) => issue.id === overIssueId));
    }
    if (!target || target.landing === null) return;

    const issue = items.find((candidate) => candidate.id === issueId);
    if (!issue) return;
    const from = visible.find((column) => column.cards.some((candidate) => candidate.id === issueId));
    if (from?.key === target.key) return;

    const set = target.landing(issue);
    setPending((current) => new Map(current).set(issueId, { patch: set, settled: false }));
    const settle = (failed: boolean) =>
      setPending((current) => {
        const next = new Map(current);
        if (failed) next.delete(issueId);
        else next.set(issueId, { patch: set, settled: true });
        return next;
      });
    if (set.status !== undefined) {
      move.mutate(
        { id: issueId, status: set.status },
        { onSuccess: () => settle(false), onError: () => settle(true) },
      );
    } else {
      patch.mutate([{ id: issueId, set }], { onSuccess: () => settle(false), onError: () => settle(true) });
    }
  };

  const hiddenCount = board.hidden.size;

  return (
    <>
      <FiltersBar />
      <DndContext
        collisionDetection={boardCollision}
        sensors={sensors}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
        onDragCancel={() => setActiveId(null)}
      >
        <div className="board">
          {visible.map((column) => (
            <Column
              key={column.key}
              column={column}
              groupBy={board.groupBy}
              cards={board.cards}
              statusById={statusById}
              titleById={titleById}
              selectedRef={selectedRef}
              quickAddOpen={quickAdd === column.key}
              onQuickAdd={setQuickAdd}
              onCloseQuickAdd={() => setQuickAdd(null)}
              onOpen={onOpen}
            />
          ))}
          {hiddenCount > 0 ? (
            <div style={{ flex: "none", width: 200, padding: 10, color: "var(--faint)", fontSize: 12 }}>
              {hiddenCount} hidden {hiddenCount === 1 ? "column" : "columns"} —{" "}
              <button type="button" style={{ color: "var(--accent-ink)" }} onClick={showAllColumns}>
                show all
              </button>
            </div>
          ) : null}
        </div>
        <DragOverlay>
          {activeIssue ? (
            <div className="card" style={{ width: 268, boxShadow: "var(--shadow-lg)" }}>
              <CardBody
                issue={activeIssue}
                groupBy={board.groupBy}
                cards={board.cards}
                status={statusById.get(activeIssue.status)}
                epicTitle={activeIssue.epic !== null ? (titleById.get(activeIssue.epic) ?? null) : null}
              />
            </div>
          ) : null}
        </DragOverlay>
      </DndContext>
    </>
  );
}
