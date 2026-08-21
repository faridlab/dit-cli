// The Docs side pane: a VS Code-style file explorer over the doc roots
// (ADR 0010). The tree is built client-side from the flat page listing —
// folders only exist because pages live under them, so there are no empty
// folders to create or delete. Right-click rename/delete, an inline
// new-file row (also on double-click of the blank space below the tree),
// and drag-a-file-onto-a-folder moves it through one commit.

import { useEffect, useMemo, useState } from "react";
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import * as ContextMenu from "@radix-ui/react-context-menu";
import { ChevronRight, ChevronsDownUp, FilePlus2, FileText, Folder, FolderOpen } from "lucide-react";
import { toast } from "sonner";
import { useDeleteDoc, useDocs, useMoveDoc, usePutDoc } from "../../lib/queries";
import { cn } from "../../lib/cn";
import type { DocEntryDto } from "../../lib/types";
import { ErrorBox, Loading } from "../states";

const DOC_ROOTS = ["docs", "notes", "epics", "changelogs"] as const;
const FILE_DRAG_PREFIX = "file:";
const DIR_DROP_PREFIX = "dir:";
const EXPANDED_KEY = "dit.docs.expanded";

/** Client-side echo of the server's `DocPath` rules, so inline input can
 *  reject a bad path before the round trip. The server remains the
 *  authority — its 400 message lands in the same inline spot. */
function newPathProblem(input: string): string | null {
  const path = input.trim().replace(/\/+$/, "");
  if (path.length === 0) return "give the page a path, e.g. docs/meeting-notes.md";
  const segments = path.split("/");
  const root = segments[0] ?? "";
  if (!(DOC_ROOTS as readonly string[]).includes(root)) {
    return `the first segment must be one of: ${DOC_ROOTS.join(", ")}`;
  }
  for (const folder of segments.slice(0, -1)) {
    if (!/^[a-z0-9-]+$/.test(folder)) {
      return `"${folder}" must be lowercase letters, digits and dashes`;
    }
  }
  const name = segments[segments.length - 1] ?? "";
  if (!/^[a-z0-9][a-z0-9-_.]*\.md$/.test(name)) {
    return `"${name}" must be a .md name of lowercase letters, digits and dashes`;
  }
  return null;
}

/** The first heading a fresh page gets: its file name as a title. */
function titleFor(path: string): string {
  return (path.split("/").pop() ?? "untitled")
    .replace(/\.md$/, "")
    .replace(/[-_]+/g, " ");
}

// -- the tree -----------------------------------------------------------------

type Node =
  | { kind: "dir"; path: string; name: string; children: Node[] }
  | { kind: "file"; path: string; name: string; updatedMs: number };

/** Folders are implied: every intermediate segment of a page's path becomes
 *  one. The four roots always exist so they can be dropped onto, right
 *  clicked, and created into even while empty. */
function buildTree(entries: DocEntryDto[]): Node[] {
  const roots: Extract<Node, { kind: "dir" }>[] = DOC_ROOTS.map((root) => ({
    kind: "dir",
    path: root,
    name: root,
    children: [],
  }));
  const dirs = new Map(roots.map((root) => [root.path, root]));

  const dirAt = (path: string): Extract<Node, { kind: "dir" }> => {
    const existing = dirs.get(path);
    if (existing) return existing;
    const segments = path.split("/");
    const created: Extract<Node, { kind: "dir" }> = {
      kind: "dir",
      path,
      name: segments[segments.length - 1] ?? path,
      children: [],
    };
    dirs.set(path, created);
    const parentPath = segments.slice(0, -1).join("/");
    dirAt(parentPath).children.push(created);
    return created;
  };

  for (const entry of entries) {
    const segments = entry.path.split("/");
    dirAt(segments.slice(0, -1).join("/")).children.push({
      kind: "file",
      path: entry.path,
      name: segments[segments.length - 1] ?? entry.path,
      updatedMs: entry.updated_ms,
    });
  }

  const sortLevel = (nodes: Node[]) => {
    nodes.sort((a, b) => a.name.localeCompare(b.name));
    for (const node of nodes) if (node.kind === "dir") sortLevel(node.children);
  };
  for (const root of roots) sortLevel(root.children);
  return roots;
}

function loadExpanded(): Set<string> {
  try {
    const raw = window.localStorage.getItem(EXPANDED_KEY);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === "string")) {
      return new Set(parsed as string[]);
    }
  } catch {
    // Fall through to the default: roots open.
  }
  return new Set(DOC_ROOTS as readonly string[]);
}

// -- rows ---------------------------------------------------------------------

const menuContent =
  "min-w-[168px] rounded-md border border-ctl bg-card p-1 text-xs text-zinc-300 shadow-xl";
const menuItem =
  "flex cursor-default select-none items-center gap-2 rounded px-2 py-1.5 outline-none data-highlighted:bg-edge data-highlighted:text-zinc-100";

/** An inline text entry that commits on Enter, cancels on Escape or blur —
 *  the one interaction every editor row (new file, rename) shares. */
function InlineInput({
  initial,
  placeholder,
  onCommit,
  onCancel,
}: {
  initial: string;
  placeholder: string;
  onCommit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <input
      autoFocus
      value={value}
      placeholder={placeholder}
      onChange={(event) => setValue(event.target.value)}
      onBlur={() => onCancel()}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          onCommit(value);
        } else if (event.key === "Escape") {
          event.preventDefault();
          onCancel();
        }
      }}
      onClick={(event) => event.stopPropagation()}
      className="h-[24px] w-full min-w-0 rounded border border-accent bg-card px-1.5 font-mono text-xs text-zinc-100 outline-none"
    />
  );
}

function FileRow({
  node,
  depth,
  active,
  renaming,
  dirty,
  onActivate,
  onOpen,
  onRenameCommit,
  onRenameCancel,
  onDelete,
  onRenameStart,
}: {
  node: Extract<Node, { kind: "file" }>;
  depth: number;
  active: boolean;
  renaming: boolean;
  /** The open tab's buffer differs from the saved body — the same dot the
   *  tab shows, visible from the tree before you switch. */
  dirty: boolean;
  onActivate: (path: string) => void;
  onOpen: (path: string) => void;
  onRenameCommit: (from: string, name: string) => void;
  onRenameCancel: () => void;
  onDelete: (path: string) => void;
  onRenameStart: (path: string) => void;
}) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: `${FILE_DRAG_PREFIX}${node.path}`,
  });

  if (renaming) {
    return (
      <div
        className="flex items-center gap-1.5 py-0.5 pr-2"
        style={{ paddingLeft: 8 + depth * 12 }}
      >
        <FileText className="size-3.5 shrink-0 text-zinc-500" aria-hidden />
        <InlineInput
          initial={node.name.replace(/\.md$/, "")}
          placeholder="page name"
          onCommit={(value) => onRenameCommit(node.path, value)}
          onCancel={onRenameCancel}
        />
      </div>
    );
  }

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger asChild>
        <button
          type="button"
          ref={setNodeRef}
          {...attributes}
          onPointerDown={(event) => listeners?.onPointerDown?.(event)}
          onClick={() => onActivate(node.path)}
          onDoubleClick={() => onOpen(node.path)}
          onKeyDown={(event) => {
            if (event.key === "F2") {
              event.preventDefault();
              onRenameStart(node.path);
            } else if (event.key === "Delete") {
              event.preventDefault();
              onDelete(node.path);
            }
          }}
          title={node.path}
          className={cn(
            "flex h-[26px] w-full items-center gap-1.5 rounded-md pr-2 text-left font-mono text-xs",
            active
              ? "bg-edge text-zinc-100"
              : "text-zinc-400 hover:bg-card hover:text-zinc-200",
            isDragging && "opacity-30",
          )}
          style={{ paddingLeft: 8 + depth * 12 }}
        >
          <FileText className="size-3.5 shrink-0 text-zinc-500" aria-hidden />
          <span className="truncate">{node.name}</span>
          {dirty ? (
            <span
              className="ml-auto size-1.5 shrink-0 rounded-full bg-accent"
              aria-label="unsaved changes"
            />
          ) : null}
        </button>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className={menuContent}>
          <ContextMenu.Item className={menuItem} onSelect={() => onRenameStart(node.path)}>
            Rename <span className="ml-auto text-zinc-600">F2</span>
          </ContextMenu.Item>
          <ContextMenu.Item
            className={cn(menuItem, "text-red-300 data-highlighted:bg-red-950/40 data-highlighted:text-red-200")}
            onSelect={() => onDelete(node.path)}
          >
            Delete
          </ContextMenu.Item>
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function DirRow({
  node,
  depth,
  expanded,
  onToggle,
  onCreateStart,
  children,
}: {
  node: Extract<Node, { kind: "dir" }>;
  depth: number;
  expanded: boolean;
  onToggle: (path: string) => void;
  onCreateStart: (dir: string) => void;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `${DIR_DROP_PREFIX}${node.path}` });

  return (
    <ContextMenu.Root>
      <div ref={setNodeRef} className="select-none">
        <ContextMenu.Trigger asChild>
          <button
            type="button"
            onClick={() => onToggle(node.path)}
            title={node.path}
            className={cn(
              "flex h-[26px] w-full items-center gap-1.5 rounded-md pr-2 text-left font-mono text-xs",
              "text-zinc-300 hover:bg-card hover:text-zinc-100",
              isOver && "bg-card ring-1 ring-inset ring-accent",
            )}
            style={{ paddingLeft: 8 + depth * 12 }}
          >
            <ChevronRight
              className={cn(
                "size-3.5 shrink-0 text-zinc-500 transition-transform",
                expanded && "rotate-90",
              )}
              aria-hidden
            />
            {expanded ? (
              <FolderOpen className="size-3.5 shrink-0 text-accent/80" aria-hidden />
            ) : (
              <Folder className="size-3.5 shrink-0 text-accent/80" aria-hidden />
            )}
            <span className="truncate">{node.name}</span>
          </button>
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Content className={menuContent}>
            <ContextMenu.Item className={menuItem} onSelect={() => onCreateStart(node.path)}>
              <FilePlus2 className="size-3.5" aria-hidden /> New File
            </ContextMenu.Item>
          </ContextMenu.Content>
        </ContextMenu.Portal>
      </div>
      {expanded ? <div>{children}</div> : null}
    </ContextMenu.Root>
  );
}

// -- the pane -----------------------------------------------------------------

export function DocsPane({
  p,
  onSelect,
  onOpen,
  onMoved,
  onDeleted,
  isDirty,
}: {
  p: string | null;
  /** Single click — the view opens this as a preview tab. */
  onSelect: (path: string) => void;
  /** Double click — the view pins this as a permanent tab. */
  onOpen: (path: string) => void;
  /** A drag/rename landed: the view retargets the page's tab and draft. */
  onMoved: (from: string, to: string) => void;
  /** A page was deleted here: the view closes its tab and picks a
   *  neighbor. */
  onDeleted: (path: string) => void;
  /** Marks pages whose open buffer is ahead of what was committed. */
  isDirty: (path: string) => boolean;
}) {
  const docs = useDocs();
  const put = usePutDoc();
  const move = useMoveDoc();
  const remove = useDeleteDoc();

  const [expanded, setExpanded] = useState<Set<string>>(loadExpanded);
  const [creatingIn, setCreatingIn] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [inlineError, setInlineError] = useState<string | null>(null);

  useEffect(() => {
    try {
      window.localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded]));
    } catch {
      // A blocked or full localStorage only loses the remembered folders.
    }
  }, [expanded]);

  // Four pixels of movement before a drag starts, so plain clicks still
  // open the page — same threshold the board uses.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  const tree = useMemo(() => buildTree(docs.data ?? []), [docs.data]);

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const createIn = (dir: string) => {
    setRenaming(null);
    setInlineError(null);
    setCreatingIn(dir);
    // The input appears as the folder's last child; opening the folder
    // must happen first or it would render nowhere.
    setExpanded((prev) => (prev.has(dir) ? prev : new Set(prev).add(dir)));
  };

  const commitCreate = (dir: string, name: string) => {
    setCreatingIn(null);
    // Accepting a bare name is the point of an inline row; the .md the
    // schema requires is appended rather than demanded.
    const file = name.trim().replace(/\/+$/, "");
    if (file.length === 0) return;
    const path = `${dir}/${file.endsWith(".md") ? file : `${file}.md`}`;
    const problem = newPathProblem(path);
    if (problem !== null) {
      setInlineError(problem);
      return;
    }
    put.mutate(
      { path, body: `# ${titleFor(path)}\n\n` },
      {
        onSuccess: (saved) => {
          setInlineError(null);
          onSelect(saved.path);
        },
        onError: (error) => {
          setInlineError(error instanceof Error ? error.message : String(error));
          setCreatingIn(dir);
        },
      },
    );
  };

  const commitRename = (from: string, name: string) => {
    setRenaming(null);
    const file = name.trim().replace(/\/+$/, "");
    if (file.length === 0) return;
    const parent = from.split("/").slice(0, -1).join("/");
    const to = `${parent}/${file.endsWith(".md") ? file : `${file}.md`}`;
    const problem = newPathProblem(to);
    if (problem !== null) {
      setInlineError(problem);
      return;
    }
    if (to === from) return;
    move.mutate(
      { from, to },
      {
        onSuccess: () => {
          setInlineError(null);
          toast.success(`Renamed to ${to}`);
          onMoved(from, to);
          // Renaming implies focus: the page follows its new name.
          onSelect(to);
        },
        onError: (error) => {
          setInlineError(error instanceof Error ? error.message : String(error));
          setRenaming(from);
        },
      },
    );
  };

  const deletePage = (path: string) => {
    const confirmed = window.confirm(
      `Delete ${path}?\n\nThe page is removed in one commit — git history keeps every version, so nothing is lost permanently.`,
    );
    if (!confirmed) return;
    remove.mutate(path, {
      onSuccess: () => {
        toast.success(`Deleted ${path}`);
        if (renaming === path) setRenaming(null);
        if (creatingIn !== null && path.startsWith(`${creatingIn}/`)) setCreatingIn(null);
        onDeleted(path);
      },
    });
  };

  const onDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over) return;
    const from = String(active.id);
    const dir = String(over.id);
    if (!from.startsWith(FILE_DRAG_PREFIX) || !dir.startsWith(DIR_DROP_PREFIX)) return;
    const path = from.slice(FILE_DRAG_PREFIX.length);
    const targetDir = dir.slice(DIR_DROP_PREFIX.length);
    const name = path.split("/").pop() ?? path;
    const to = `${targetDir}/${name}`;
    if (to === path) return;
    move.mutate(
      { from: path, to },
      {
        onSuccess: () => {
          toast.success(`Moved to ${to}`);
          onMoved(path, to);
        },
      },
    );
  };

  const renderNodes = (nodes: Node[], depth: number): React.ReactNode =>
    nodes.map((node) => {
      if (node.kind === "dir") {
        const showCreate = creatingIn === node.path;
        return (
          <DirRow
            key={node.path}
            node={node}
            depth={depth}
            expanded={expanded.has(node.path)}
            onToggle={toggleDir}
            onCreateStart={createIn}
          >
            {renderNodes(node.children, depth + 1)}
            {showCreate ? (
              <div
                className="flex items-center gap-1.5 py-0.5 pr-2"
                style={{ paddingLeft: 8 + (depth + 1) * 12 }}
              >
                <FileText className="size-3.5 shrink-0 text-zinc-500" aria-hidden />
                <InlineInput
                  initial=""
                  placeholder="page-name.md"
                  onCommit={(value) => commitCreate(node.path, value)}
                  onCancel={() => setCreatingIn(null)}
                />
              </div>
            ) : null}
          </DirRow>
        );
      }
      return (
        <FileRow
          key={node.path}
          node={node}
          depth={depth}
          active={node.path === p}
          renaming={renaming === node.path}
          dirty={isDirty(node.path)}
          onActivate={(path) => onSelect(path)}
          onOpen={onOpen}
          onRenameCommit={commitRename}
          onRenameCancel={() => setRenaming(null)}
          onDelete={deletePage}
          onRenameStart={(path) => {
            setCreatingIn(null);
            setRenaming(path);
          }}
        />
      );
    });

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* The explorer's action row — VS Code keeps new-file and collapse
          here, not in a form below the tree. */}
      <div className="flex shrink-0 items-center gap-0.5 px-2 py-1">
        <button
          type="button"
          title="New page in docs/"
          onClick={() => createIn("docs")}
          className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
        >
          <FilePlus2 className="size-4" aria-hidden />
        </button>
        <button
          type="button"
          title="Collapse folders"
          onClick={() => setExpanded(new Set(DOC_ROOTS as readonly string[]))}
          className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
        >
          <ChevronsDownUp className="size-4" aria-hidden />
        </button>
      </div>

      <div
        className="min-h-0 flex-1 overflow-y-auto pb-2"
        onDoubleClick={(event) => {
          // Blank space below the tree — not a row — starts a page in the
          // primary root, the way an empty explorer offers itself.
          if (event.target === event.currentTarget) createIn("docs");
        }}
      >
        {docs.isPending ? (
          <Loading label="Loading pages…" className="p-4" />
        ) : docs.isError ? (
          <div className="p-2">
            <ErrorBox
              error={docs.error}
              onRetry={() => void docs.refetch()}
              title="Could not list pages"
            />
          </div>
        ) : (
          <DndContext sensors={sensors} onDragEnd={onDragEnd}>
            {renderNodes(tree, 0)}
          </DndContext>
        )}
      </div>

      {inlineError ? (
        <p className="shrink-0 border-t border-edge px-3 py-2 text-xs leading-relaxed text-warn-text">
          {inlineError}
        </p>
      ) : null}
    </div>
  );
}
