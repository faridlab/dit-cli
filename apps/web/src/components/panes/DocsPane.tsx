// The Docs sidebar section: a VS Code-style file explorer over the doc roots
// (ADR 0010). The tree is built client-side from the flat page listing —
// folders exist because pages live under them, plus hand-made folders
// (git has no empty directories, so those live in this browser until a
// page moves in). Clicking a folder selects it and folds it: the new-page
// and new-folder buttons in the heading target the selection. Every other
// action lives in the right-click menu — rename in place, move to another
// folder, delete with a second confirming click — and dragging a page onto
// a folder moves it through one commit.
//
// The markup follows the approved workbench design: a PaneSection heading
// with three small buttons, then `.sb-body.tree` of `.row` buttons indented
// through the `--d` custom property, and a hint paragraph at the bottom.

import {
  useEffect,
  useMemo,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type ReactNode,
  type Ref,
  type RefCallback,
} from "react";
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Copy,
  FileText,
  Folder,
  Pencil,
  Plus,
  Trash2,
} from "lucide-react";
import { toast } from "sonner";
import {
  useDeleteDoc,
  useDocs,
  useMoveDoc,
  usePutDoc,
} from "../../lib/queries";
import { cn } from "../../lib/cn";
import type { DocEntryDto } from "../../lib/types";
import { ErrorBox, Loading } from "../states";
import { ContextMenuFor, MenuButton, type MenuItem } from "../chrome";
import { PaneSection } from "../PaneSection";

const DOC_ROOTS = ["docs", "notes", "epics", "changelogs"] as const;
const FILE_DRAG_PREFIX = "file:";
const DIR_DROP_PREFIX = "dir:";
const EXPANDED_KEY = "dit.docs.expanded";
const FOLDERS_KEY = "dit.docs.folders";

const EMPTY_FOLDER_NOTE =
  "Git cannot hold an empty directory: the folder lives in this browser until a page is created inside it.";

function nameOf(path: string): string {
  return path.split("/").pop() ?? path;
}

function parentOf(path: string): string {
  return path.split("/").slice(0, -1).join("/");
}

/** A typed name becomes a file-system-safe, lowercase segment: the server's
 *  `DocPath` rules only accept lowercase letters, digits, dashes, dots and
 *  underscores. */
function slugify(input: string): string {
  return input
    .trim()
    .replace(/\.md$/, "")
    .replace(/[^\w.-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .toLowerCase();
}

/** The first heading a fresh page gets: its file name as a title. */
function titleFor(path: string): string {
  const name = nameOf(path).replace(/\.md$/, "").replace(/[-_]+/g, " ");
  return name.charAt(0).toUpperCase() + name.slice(1);
}

async function copyText(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(label);
  } catch {
    // Clipboard access needs a secure context; the toast still shows the
    // value so it can be copied by hand.
    toast(`${label}: ${text}`);
  }
}

function loadStringSet(key: string, fallback: readonly string[]): Set<string> {
  try {
    const raw = window.localStorage.getItem(key);
    const parsed: unknown = raw === null ? null : JSON.parse(raw);
    if (Array.isArray(parsed) && parsed.every((v) => typeof v === "string")) {
      return new Set(parsed as string[]);
    }
  } catch {
    // Fall through to the default.
  }
  return new Set(fallback);
}

function persistStringSet(key: string, value: ReadonlySet<string>) {
  try {
    window.localStorage.setItem(key, JSON.stringify([...value]));
  } catch {
    // A blocked or full localStorage only loses the remembered state.
  }
}

// -- the tree -----------------------------------------------------------------

interface TreeRow {
  depth: number;
  path: string;
  label: string;
  folder: boolean;
  root: boolean;
  /** A hand-made folder with no page under it yet — client-only. */
  empty: boolean;
}

/** Every folder that exists: the roots, each intermediate segment of a
 *  page's path, and the hand-made ones. */
function allFolders(
  entries: DocEntryDto[],
  explicit: ReadonlySet<string>,
): Set<string> {
  const folders = new Set<string>(DOC_ROOTS);
  for (const entry of entries) {
    const parts = entry.path.split("/");
    for (let k = 1; k < parts.length; k++)
      folders.add(parts.slice(0, k).join("/"));
  }
  for (const folder of explicit) {
    if (!DOC_ROOTS.some((root) => folder.startsWith(`${root}/`))) continue;
    const parts = folder.split("/");
    for (let k = 1; k <= parts.length; k++)
      folders.add(parts.slice(0, k).join("/"));
  }
  return folders;
}

/** The visible rows, top to bottom: folders first at each level (sorted),
 *  then pages, descending only into folders that are not folded. */
function flattenTree(
  entries: DocEntryDto[],
  folders: ReadonlySet<string>,
  expanded: ReadonlySet<string>,
): TreeRow[] {
  const rows: TreeRow[] = [];
  const directChild = (candidate: string, dir: string) =>
    candidate.startsWith(`${dir}/`) &&
    !candidate.slice(dir.length + 1).includes("/");
  const walk = (dir: string, depth: number) => {
    const subs = [...folders]
      .filter((folder) => directChild(folder, dir))
      .sort();
    const files = entries
      .filter((entry) => directChild(entry.path, dir))
      .sort((a, b) => a.path.localeCompare(b.path));
    for (const sub of subs) {
      rows.push({
        depth,
        path: sub,
        label: nameOf(sub),
        folder: true,
        root: false,
        empty: !entries.some((entry) => entry.path.startsWith(`${sub}/`)),
      });
      if (expanded.has(sub)) walk(sub, depth + 1);
    }
    for (const file of files) {
      rows.push({
        depth,
        path: file.path,
        label: nameOf(file.path),
        folder: false,
        root: false,
        empty: false,
      });
    }
  };
  for (const root of DOC_ROOTS) {
    rows.push({
      depth: 0,
      path: root,
      label: root,
      folder: true,
      root: true,
      empty: false,
    });
    if (expanded.has(root)) walk(root, 1);
  }
  return rows;
}

// -- rows ---------------------------------------------------------------------

function depthStyle(depth: number): CSSProperties {
  return { "--d": depth } as CSSProperties;
}

/** The rows are wrapped by Radix `asChild` triggers (context menu, follow-up
 *  menu), which clone their ref and handlers onto the child. A row therefore
 *  forwards everything it does not use to its `<button>`, and merges the
 *  trigger's ref with dnd-kit's. */
type RowButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  ref?: Ref<HTMLButtonElement>;
};

function mergeRefs<T>(...refs: Array<Ref<T> | undefined>): RefCallback<T> {
  return (node) => {
    for (const ref of refs) {
      if (typeof ref === "function") ref(node);
      else if (ref) ref.current = node;
    }
  };
}

/** An inline text entry that commits on Enter, cancels on Escape or blur —
 *  the one interaction every rename row shares. */
function RenameRow({
  row,
  onCommit,
  onCancel,
}: {
  row: TreeRow;
  onCommit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(
    row.folder ? row.label : row.label.replace(/\.md$/, ""),
  );
  return (
    <div className="row" style={depthStyle(row.depth)}>
      {row.folder ? (
        <Folder className="i" aria-hidden />
      ) : (
        <FileText className="i" aria-hidden />
      )}
      <input
        autoFocus
        value={value}
        placeholder={row.folder ? "folder-name" : "page-name"}
        aria-label={`Rename ${row.path}`}
        onChange={(event) => setValue(event.target.value)}
        onFocus={(event) => event.target.select()}
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
      />
    </div>
  );
}

/** A second menu opened from a context-menu item ("New page here", "Move
 *  to…") anchors to the row it came from. A row has either its context
 *  menu or its follow-up menu attached — both wrap the same element. */
function RowMenus({
  items,
  followUp,
  onFollowUpClose,
  children,
}: {
  items: MenuItem[];
  followUp: MenuItem[] | null;
  onFollowUpClose: () => void;
  children: ReactNode;
}) {
  if (followUp !== null) {
    return (
      <MenuButton
        items={followUp}
        open
        onOpenChange={(open) => {
          if (!open) onFollowUpClose();
        }}
      >
        {children}
      </MenuButton>
    );
  }
  return <ContextMenuFor items={items}>{children}</ContextMenuFor>;
}

function FolderRow({
  row,
  expanded,
  selected,
  onFold,
  onRename,
  onDelete,
  ref,
  className,
  onClick,
  onKeyDown,
  ...rest
}: {
  row: TreeRow;
  expanded: boolean;
  selected: boolean;
  /** Click: select the folder as the filing target and fold/unfold it. */
  onFold: (path: string) => void;
  onRename: (path: string) => void;
  onDelete: (path: string) => void;
} & RowButtonProps) {
  const { setNodeRef, isOver } = useDroppable({
    id: `${DIR_DROP_PREFIX}${row.path}`,
  });
  return (
    <button
      type="button"
      {...rest}
      ref={mergeRefs(setNodeRef, ref)}
      className={cn("row fold", (selected || isOver) && "selfold", className)}
      style={depthStyle(row.depth)}
      title={row.path}
      onClick={(event) => {
        onClick?.(event);
        onFold(row.path);
      }}
      onKeyDown={(event) => {
        onKeyDown?.(event);
        if (event.key === "F2" && !row.root) {
          event.preventDefault();
          onRename(row.path);
        } else if (event.key === "Delete" && row.empty) {
          event.preventDefault();
          onDelete(row.path);
        }
      }}
    >
      {expanded ? (
        <ChevronDown className="i" aria-hidden />
      ) : (
        <ChevronRight className="i" aria-hidden />
      )}
      <Folder className="i" aria-hidden />
      <span className="lbl">{row.label}</span>
      {row.empty && !row.root ? (
        <span className="cnt" title="client-only until a page lives here">
          empty
        </span>
      ) : null}
    </button>
  );
}

function FileRow({
  row,
  active,
  dirty,
  onActivate,
  onOpen,
  onRename,
  onDelete,
  ref,
  className,
  onClick,
  onPointerDown,
  onKeyDown,
  ...rest
}: {
  row: TreeRow;
  active: boolean;
  /** The open tab's buffer differs from the saved body — the same dot the
   *  tab shows, visible from the tree before you switch. */
  dirty: boolean;
  onActivate: (path: string) => void;
  onOpen: (path: string) => void;
  onRename: (path: string) => void;
  onDelete: (path: string) => void;
} & RowButtonProps) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: `${FILE_DRAG_PREFIX}${row.path}`,
  });
  return (
    <button
      type="button"
      {...rest}
      {...attributes}
      ref={mergeRefs(setNodeRef, ref)}
      onPointerDown={(event) => {
        onPointerDown?.(event);
        listeners?.onPointerDown?.(event);
      }}
      className={cn(
        "row file",
        active && "on",
        isDragging && "opacity-30",
        className,
      )}
      style={depthStyle(row.depth)}
      title={row.path}
      onClick={(event) => {
        onClick?.(event);
        onActivate(row.path);
      }}
      onDoubleClick={() => onOpen(row.path)}
      onKeyDown={(event) => {
        onKeyDown?.(event);
        if (event.key === "F2") {
          event.preventDefault();
          onRename(row.path);
        } else if (event.key === "Delete") {
          event.preventDefault();
          onDelete(row.path);
        }
      }}
    >
      <FileText className="i" aria-hidden />
      <span className="lbl">{row.label}</span>
      {dirty ? (
        <span
          className="dot"
          title="buffer ahead of the committed body — autosaves after a pause"
        />
      ) : null}
    </button>
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

  const [expanded, setExpanded] = useState<Set<string>>(() =>
    loadStringSet(EXPANDED_KEY, DOC_ROOTS),
  );
  const [explicitFolders, setExplicitFolders] = useState<Set<string>>(() =>
    loadStringSet(FOLDERS_KEY, []),
  );
  // The folder the heading buttons file into. Clicking a folder selects it;
  // clicking a page clears the selection, and "docs" is the fallback.
  const [selectedDir, setSelectedDir] = useState<string | null>("docs");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [followUp, setFollowUp] = useState<{
    path: string;
    items: MenuItem[];
  } | null>(null);

  useEffect(() => persistStringSet(EXPANDED_KEY, expanded), [expanded]);
  useEffect(
    () => persistStringSet(FOLDERS_KEY, explicitFolders),
    [explicitFolders],
  );

  // Four pixels of movement before a drag starts, so plain clicks still
  // open the page — same threshold the board uses.
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
  );

  const entries = docs.data ?? [];
  const folders = useMemo(
    () => allFolders(entries, explicitFolders),
    [entries, explicitFolders],
  );
  const rows = useMemo(
    () => flattenTree(entries, folders, expanded),
    [entries, folders, expanded],
  );
  // A folder can vanish while selected (its last page moved away, or it
  // was deleted); the buttons always need a real target.
  const targetDir =
    selectedDir !== null && folders.has(selectedDir) ? selectedDir : "docs";
  const anyFolded = [...folders].some((folder) => !expanded.has(folder));

  const selectFolder = (path: string) => {
    setSelectedDir(path);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const reveal = (path: string) =>
    setExpanded((prev) => (prev.has(path) ? prev : new Set(prev).add(path)));

  // One button, both directions: anything folded opens the whole tree;
  // otherwise every folder folds, roots included, leaving one row per root.
  const toggleFoldAll = () => {
    setExpanded(anyFolded ? new Set(folders) : new Set());
  };

  const createDoc = (dir: string, name: string) => {
    const base = slugify(name) || "untitled";
    let path = `${dir}/${base}.md`;
    let k = 2;
    while (entries.some((entry) => entry.path === path))
      path = `${dir}/${base}-${k++}.md`;
    put.mutate(
      { path, body: `# ${titleFor(path)}\n\n` },
      {
        onSuccess: (saved) => {
          toast(`Created ${saved.path} · committed`);
          reveal(dir);
          onSelect(saved.path);
        },
      },
    );
  };

  const createFolder = (dir: string, name: string) => {
    const slug = slugify(name);
    if (slug.length === 0) return;
    const path = `${dir}/${slug}`;
    if (folders.has(path)) {
      toast(`A folder named ${slug} already exists there`);
      return;
    }
    // A hand-made folder exists only here until a page lives under it —
    // git has no empty directories to commit.
    setExplicitFolders((prev) => new Set(prev).add(path));
    setSelectedDir(path);
    reveal(dir);
    reveal(path);
    toast(`Folder ${path} (client-only until it has a page)`);
  };

  const deleteEmptyFolder = (path: string) => {
    // Hand-made subfolders go with their parent; nothing under it is a page.
    setExplicitFolders(
      (prev) =>
        new Set(
          [...prev].filter(
            (folder) => folder !== path && !folder.startsWith(`${path}/`),
          ),
        ),
    );
    // Filing into a folder that no longer exists would silently pick the
    // wrong parent — climb to the closest surviving ancestor.
    if (
      selectedDir !== null &&
      (selectedDir === path || selectedDir.startsWith(`${path}/`))
    ) {
      setSelectedDir(parentOf(path) || "docs");
    }
  };

  const moveDoc = (from: string, to: string) => {
    if (from === to) return;
    if (entries.some((entry) => entry.path === to)) {
      toast("A page with that name already exists (409 Conflict)");
      return;
    }
    move.mutate(
      { from, to },
      {
        onSuccess: () => {
          toast(`Moved · git records R100 ${nameOf(from)} → ${nameOf(to)}`);
          onMoved(from, to);
        },
      },
    );
  };

  const renamePage = (from: string, name: string) => {
    const slug = slugify(name);
    if (slug.length === 0) return;
    moveDoc(from, `${parentOf(from)}/${slug}.md`);
  };

  // Renaming a folder is a move of every page under it — one commit each,
  // the only shape the write path has. Hand-made folders just change name.
  const renameFolder = async (from: string, name: string) => {
    const slug = slugify(name);
    if (slug.length === 0) return;
    const to = `${parentOf(from)}/${slug}`;
    if (to === from) return;
    if (folders.has(to)) {
      toast(`A folder named ${slug} already exists there`);
      return;
    }
    const pages = entries.filter((entry) => entry.path.startsWith(`${from}/`));
    for (const page of pages) {
      const target = `${to}${page.path.slice(from.length)}`;
      try {
        await move.mutateAsync({ from: page.path, to: target });
        onMoved(page.path, target);
      } catch {
        // The hook already surfaced the server's message; stop so the
        // folder is not left half-moved without anyone noticing.
        return;
      }
    }
    setExplicitFolders((prev) => {
      const next = new Set(
        [...prev].filter(
          (folder) => folder !== from && !folder.startsWith(`${from}/`),
        ),
      );
      next.add(to);
      return next;
    });
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.delete(from)) next.add(to);
      return next;
    });
    if (selectedDir === from) setSelectedDir(to);
    toast(
      pages.length > 0
        ? `Moved ${pages.length} page${pages.length === 1 ? "" : "s"} to ${to}/`
        : `Folder renamed to ${to}`,
    );
  };

  const deletePage = (path: string) => {
    remove.mutate(path, {
      onSuccess: () => {
        toast(`Deleted ${path} · committed`);
        onDeleted(path);
      },
    });
  };

  const onDragEnd = (event: DragEndEvent) => {
    const { active, over } = event;
    if (!over) return;
    const from = String(active.id);
    const dir = String(over.id);
    if (!from.startsWith(FILE_DRAG_PREFIX) || !dir.startsWith(DIR_DROP_PREFIX))
      return;
    const path = from.slice(FILE_DRAG_PREFIX.length);
    const targetDir = dir.slice(DIR_DROP_PREFIX.length);
    moveDoc(path, `${targetDir}/${nameOf(path)}`);
  };

  // -- menus ------------------------------------------------------------------

  const newPageItems = (dir: string): MenuItem[] => [
    { kind: "head", label: `New page in ${dir}/` },
    {
      kind: "input",
      placeholder: "page-name",
      button: "Create",
      run: (value) => createDoc(dir, value),
    },
  ];

  const newFolderItems = (dir: string): MenuItem[] => [
    { kind: "head", label: `New folder in ${dir}/` },
    { kind: "text", node: EMPTY_FOLDER_NOTE },
    {
      kind: "input",
      placeholder: "folder-name",
      button: "Create",
      run: (value) => createFolder(dir, value),
    },
  ];

  const moveToItems = (path: string): MenuItem[] => [
    { kind: "head", label: "Move to folder" },
    ...[...folders].sort().map((folder) => ({
      label: `${folder}/`,
      icon: <Folder className="i" aria-hidden />,
      disabled: folder === parentOf(path),
      run: () => moveDoc(path, `${folder}/${nameOf(path)}`),
    })),
  ];

  const deletePageItem = (path: string): MenuItem => ({
    label: "Delete page…",
    icon: <Trash2 className="i" aria-hidden />,
    danger: true,
    confirm: "Click again to confirm",
    run: () => deletePage(path),
  });

  const rowItems = (row: TreeRow): MenuItem[] => {
    const items: MenuItem[] = [{ kind: "head", label: row.path }];
    if (row.folder) {
      items.push(
        {
          label: "New page here",
          icon: <Plus className="i" aria-hidden />,
          run: () =>
            setFollowUp({ path: row.path, items: newPageItems(row.path) }),
        },
        {
          label: "New folder here",
          icon: <Folder className="i" aria-hidden />,
          run: () =>
            setFollowUp({ path: row.path, items: newFolderItems(row.path) }),
        },
      );
    } else {
      items.push({
        label: "Open in a pinned tab",
        icon: <FileText className="i" aria-hidden />,
        run: () => onOpen(row.path),
      });
    }
    items.push({
      label: "Copy path",
      icon: <Copy className="i" aria-hidden />,
      run: () => void copyText(row.path, "Path copied"),
    });
    // The four roots are fixtures of the schema, not names anyone chose.
    if (!row.root) {
      items.push({
        label: "Rename",
        icon: <Pencil className="i" aria-hidden />,
        kbd: "F2",
        run: () => setRenaming(row.path),
      });
    }
    if (row.folder) {
      if (row.empty && !row.root) {
        items.push(
          { kind: "sep" },
          {
            label: "Delete empty folder",
            icon: <Trash2 className="i" aria-hidden />,
            danger: true,
            run: () => deleteEmptyFolder(row.path),
          },
        );
      }
    } else {
      items.push(
        {
          label: "Move to…",
          icon: <Folder className="i" aria-hidden />,
          run: () =>
            setFollowUp({ path: row.path, items: moveToItems(row.path) }),
        },
        { kind: "sep" },
        deletePageItem(row.path),
      );
    }
    return items;
  };

  const renderRow = (row: TreeRow) => {
    if (renaming === row.path) {
      return (
        <RenameRow
          key={`rename:${row.path}`}
          row={row}
          onCancel={() => setRenaming(null)}
          onCommit={(value) => {
            setRenaming(null);
            if (row.folder) void renameFolder(row.path, value);
            else renamePage(row.path, value);
          }}
        />
      );
    }
    return (
      <RowMenus
        key={row.path}
        items={rowItems(row)}
        followUp={followUp?.path === row.path ? followUp.items : null}
        onFollowUpClose={() => setFollowUp(null)}
      >
        {row.folder ? (
          <FolderRow
            row={row}
            expanded={expanded.has(row.path)}
            selected={selectedDir === row.path}
            onFold={selectFolder}
            onRename={setRenaming}
            onDelete={deleteEmptyFolder}
          />
        ) : (
          <FileRow
            row={row}
            active={row.path === p}
            dirty={isDirty(row.path)}
            onActivate={(path) => {
              setSelectedDir(null);
              onSelect(path);
            }}
            onOpen={onOpen}
            onRename={setRenaming}
            // The Delete key asks the same way the menu does: a second click.
            onDelete={(path) =>
              setFollowUp({
                path,
                items: [{ kind: "head", label: path }, deletePageItem(path)],
              })
            }
          />
        )}
      </RowMenus>
    );
  };

  return (
    <PaneSection
      id="docs.pages"
      title="Pages"
      fill
      actions={
        <>
        <MenuButton items={newPageItems(targetDir)} align="end">
          <button type="button" title={`New page in ${targetDir}/`}>
            <Plus className="i" aria-hidden />
          </button>
        </MenuButton>
        <MenuButton items={newFolderItems(targetDir)} align="end">
          <button type="button" title={`New folder in ${targetDir}/`}>
            <Folder className="i" aria-hidden />
          </button>
        </MenuButton>
        <button
          type="button"
          title={anyFolded ? "Expand all" : "Collapse all"}
          onClick={toggleFoldAll}
        >
          <ChevronsUpDown className="i" aria-hidden />
        </button>
        </>
      }
    >

      <div
        className="sb-body tree"
        onDoubleClick={(event) => {
          // Blank space — not a row — starts a page in the selected folder,
          // the way an empty explorer offers itself.
          if (event.target instanceof Element && event.target.closest(".row"))
            return;
          createDoc(targetDir, "untitled");
        }}
      >
        {docs.isPending ? (
          <Loading label="Loading pages…" className="p-2" />
        ) : docs.isError ? (
          <ErrorBox
            error={docs.error}
            onRetry={() => void docs.refetch()}
            title="Could not list pages"
          />
        ) : (
          <DndContext sensors={sensors} onDragEnd={onDragEnd}>
            {rows.map(renderRow)}
          </DndContext>
        )}
        <p
          className="empty"
          style={{
            padding: "12px 10px 0",
            fontSize: 11.5,
            color: "var(--faint)",
          }}
        >
          Right-click a row for rename, delete, new page. Click a folder to
          select it and fold it. Double-click blank space for a new page.
        </p>
      </div>
    </PaneSection>
  );
}
