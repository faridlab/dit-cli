// The Docs side pane: a VS Code-style file explorer over the doc roots
// (ADR 0010). The tree is built client-side from the flat page listing —
// folders exist because pages live under them, plus hand-made folders
// (git has no empty directories, so those live in this browser until a
// page moves in). Clicking a folder selects it: the new-file and
// new-folder buttons in the action row target the selection. Right-click
// rename/delete, drag-a-file-onto-a-folder moves it through one commit.

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
import {
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  FilePlus2,
  FileText,
  Folder,
  FolderOpen,
  FolderPlus,
  RefreshCw,
} from "lucide-react";
import { toast } from "sonner";
import { useDeleteDoc, useDocs, useMoveDoc, usePutDoc } from "../../lib/queries";
import { cn } from "../../lib/cn";
import type { DocEntryDto } from "../../lib/types";
import { ErrorBox, Loading } from "../states";

const DOC_ROOTS = ["docs", "notes", "epics", "changelogs"] as const;
const FILE_DRAG_PREFIX = "file:";
const DIR_DROP_PREFIX = "dir:";
const EXPANDED_KEY = "dit.docs.expanded";
const FOLDERS_KEY = "dit.docs.folders";

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

/** Folders the user created by hand. Git cannot hold an empty directory,
 *  so these live only in this browser until a page moves into them — from
 *  then on the pages themselves imply the folder. */
function loadExplicitFolders(): Set<string> {
  try {
    const raw = window.localStorage.getItem(FOLDERS_KEY);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === "string")) {
      return new Set(parsed as string[]);
    }
  } catch {
    // Fall through to no hand-made folders.
  }
  return new Set();
}

// -- the tree -----------------------------------------------------------------

type Node =
  | { kind: "dir"; path: string; name: string; children: Node[] }
  | { kind: "file"; path: string; name: string; updatedMs: number };

/** Folders are implied: every intermediate segment of a page's path becomes
 *  one. The four roots always exist so they can be dropped onto, right
 *  clicked, and created into even while empty; hand-made folders are merged
 *  in so they show before anything lives under them. */
function buildTree(entries: DocEntryDto[], explicitFolders: ReadonlySet<string>): Node[] {
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
  for (const folder of explicitFolders) {
    if ((DOC_ROOTS as readonly string[]).some((root) => folder === root || folder.startsWith(`${root}/`))) {
      dirAt(folder);
    }
  }

  const sortLevel = (nodes: Node[]) => {
    nodes.sort((a, b) => a.name.localeCompare(b.name));
    for (const node of nodes) if (node.kind === "dir") sortLevel(node.children);
  };
  for (const root of roots) sortLevel(root.children);
  return roots;
}

/** Every folder path in the tree, roots included — the expand-all target. */
function collectDirs(nodes: Node[], into: Set<string>): Set<string> {
  for (const node of nodes) {
    if (node.kind === "dir") {
      into.add(node.path);
      collectDirs(node.children, into);
    }
  }
  return into;
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
  active,
  onSelect,
  onCreateStart,
  onCreateFolderStart,
  onDelete,
  children,
}: {
  node: Extract<Node, { kind: "dir" }>;
  depth: number;
  expanded: boolean;
  /** The folder the add-file/add-folder buttons target. */
  active: boolean;
  onSelect: (path: string) => void;
  onCreateStart: (dir: string) => void;
  onCreateFolderStart: (dir: string) => void;
  onDelete: (dir: string) => void;
  children: React.ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({ id: `${DIR_DROP_PREFIX}${node.path}` });
  // A root is a fixture of the schema, not a thing the user made — only a
  // hand-made folder can be empty, and only empty folders can be deleted.
  const deletable = node.children.length === 0 && node.path.includes("/");

  return (
    <ContextMenu.Root>
      <div ref={setNodeRef} className="select-none">
        <ContextMenu.Trigger asChild>
          <button
            type="button"
            // Clicking a folder selects it (the target for new files and
            // folders) and opens it — selecting a folder you cannot see
            // into would feel like filing into the dark.
            onClick={() => onSelect(node.path)}
            title={node.path}
            className={cn(
              "flex h-[26px] w-full items-center gap-1.5 rounded-md pr-2 text-left font-mono text-xs",
              active ? "bg-edge text-zinc-100" : "text-zinc-300 hover:bg-card hover:text-zinc-100",
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
            {active ? (
              <FolderOpen className="size-3.5 shrink-0 text-accent" aria-hidden />
            ) : expanded ? (
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
            <ContextMenu.Item className={menuItem} onSelect={() => onCreateFolderStart(node.path)}>
              <FolderPlus className="size-3.5" aria-hidden /> New Folder
            </ContextMenu.Item>
            {deletable ? (
              <ContextMenu.Item
                className={cn(menuItem, "text-red-300 data-highlighted:bg-red-950/40 data-highlighted:text-red-200")}
                onSelect={() => onDelete(node.path)}
              >
                Delete
              </ContextMenu.Item>
            ) : null}
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
  const [folderCreatingIn, setFolderCreatingIn] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [inlineError, setInlineError] = useState<string | null>(null);
  // The folder the two add buttons target. Clicking a folder row selects
  // it; "docs" is the sensible starting point before anything is clicked.
  const [activeDir, setActiveDir] = useState<string>("docs");
  const [explicitFolders, setExplicitFolders] = useState<Set<string>>(loadExplicitFolders);

  useEffect(() => {
    try {
      window.localStorage.setItem(EXPANDED_KEY, JSON.stringify([...expanded]));
    } catch {
      // A blocked or full localStorage only loses the remembered folders.
    }
  }, [expanded]);

  useEffect(() => {
    try {
      window.localStorage.setItem(FOLDERS_KEY, JSON.stringify([...explicitFolders]));
    } catch {
      // A blocked or full localStorage only loses the empty folders.
    }
  }, [explicitFolders]);

  // Four pixels of movement before a drag starts, so plain clicks still
  // open the page — same threshold the board uses.
  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  const tree = useMemo(() => buildTree(docs.data ?? [], explicitFolders), [docs.data, explicitFolders]);
  const allDirs = useMemo(() => collectDirs(tree, new Set<string>()), [tree]);
  // A folder can vanish while selected (its last page moved away, or it was
  // deleted); the buttons always need a real target.
  const selectedDir = allDirs.has(activeDir) ? activeDir : "docs";
  const allExpanded = [...allDirs].every((dir) => expanded.has(dir));

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const selectDir = (path: string) => {
    setActiveDir(path);
    toggleDir(path);
  };

  // One button, both directions: everything open folds back to the roots,
  // anything closed opens the whole tree.
  const toggleExpandAll = () => {
    setExpanded(allExpanded ? new Set(DOC_ROOTS as readonly string[]) : new Set(allDirs));
  };

  const createIn = (dir: string) => {
    setRenaming(null);
    setFolderCreatingIn(null);
    setInlineError(null);
    setCreatingIn(dir);
    setActiveDir(dir);
    // The input appears as the folder's last child; opening the folder
    // must happen first or it would render nowhere.
    setExpanded((prev) => (prev.has(dir) ? prev : new Set(prev).add(dir)));
  };

  const createFolderIn = (dir: string) => {
    setCreatingIn(null);
    setRenaming(null);
    setInlineError(null);
    setFolderCreatingIn(dir);
    setActiveDir(dir);
    setExpanded((prev) => (prev.has(dir) ? prev : new Set(prev).add(dir)));
  };

  const commitFolderCreate = (dir: string, name: string) => {
    setFolderCreatingIn(null);
    const folder = name.trim().replace(/\/+$/, "");
    if (folder.length === 0) return;
    if (!/^[a-z0-9-]+$/.test(folder)) {
      setInlineError(`"${folder}" must be lowercase letters, digits and dashes`);
      return;
    }
    const path = `${dir}/${folder}`;
    if (allDirs.has(path)) {
      setInlineError(`a folder named "${folder}" already exists there`);
      return;
    }
    // A hand-made folder exists only here until a page lives under it —
    // git has no empty directories to commit.
    setExplicitFolders((prev) => new Set(prev).add(path));
    setActiveDir(path);
    setExpanded((prev) => new Set(prev).add(path));
  };

  const deleteFolder = (path: string) => {
    setExplicitFolders((prev) => {
      const next = new Set(prev);
      next.delete(path);
      return next;
    });
    setFolderCreatingIn(null);
    // Filing into a folder that no longer exists would silently pick the
    // wrong parent — climb to the closest surviving ancestor.
    if (activeDir === path || activeDir.startsWith(`${path}/`)) {
      setActiveDir(path.split("/").slice(0, -1).join("/") || "docs");
    }
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
        const showFolderCreate = folderCreatingIn === node.path;
        return (
          <DirRow
            key={node.path}
            node={node}
            depth={depth}
            expanded={expanded.has(node.path)}
            active={node.path === selectedDir}
            onSelect={selectDir}
            onCreateStart={createIn}
            onCreateFolderStart={createFolderIn}
            onDelete={deleteFolder}
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
            {showFolderCreate ? (
              <div
                className="flex items-center gap-1.5 py-0.5 pr-2"
                style={{ paddingLeft: 8 + (depth + 1) * 12 }}
              >
                <Folder className="size-3.5 shrink-0 text-zinc-500" aria-hidden />
                <InlineInput
                  initial=""
                  placeholder="folder-name"
                  onCommit={(value) => commitFolderCreate(node.path, value)}
                  onCancel={() => setFolderCreatingIn(null)}
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
            setFolderCreatingIn(null);
            setRenaming(path);
          }}
        />
      );
    });

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* The explorer's action row — creation on the left (into the selected
          folder), the tree's state on the right. */}
      <div className="flex shrink-0 items-center gap-0.5 px-2 py-1">
        <button
          type="button"
          title={`New page in ${selectedDir}/`}
          onClick={() => createIn(selectedDir)}
          className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
        >
          <FilePlus2 className="size-4" aria-hidden />
        </button>
        <button
          type="button"
          title={`New folder in ${selectedDir}/`}
          onClick={() => createFolderIn(selectedDir)}
          className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
        >
          <FolderPlus className="size-4" aria-hidden />
        </button>
        <div className="ml-auto flex items-center gap-0.5">
          <button
            type="button"
            title="Refresh"
            onClick={() => void docs.refetch()}
            className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
          >
            <RefreshCw className="size-4" aria-hidden />
          </button>
          <button
            type="button"
            title={allExpanded ? "Collapse all folders" : "Expand all folders"}
            onClick={toggleExpandAll}
            className="flex size-6 items-center justify-center rounded text-zinc-500 hover:bg-card hover:text-zinc-200"
          >
            {allExpanded ? (
              <ChevronsDownUp className="size-4" aria-hidden />
            ) : (
              <ChevronsUpDown className="size-4" aria-hidden />
            )}
          </button>
        </div>
      </div>

      <div
        className="min-h-0 flex-1 overflow-y-auto pb-2"
        onDoubleClick={(event) => {
          // Blank space below the tree — not a row — starts a page in the
          // selected folder, the way an empty explorer offers itself.
          if (event.target === event.currentTarget) createIn(selectedDir);
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
