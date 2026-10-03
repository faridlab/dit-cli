// The workspaces on this machine (ADR 0028): the first-run page, and the
// dialogs the workspace menu opens. A new workspace is a name — the server
// picks the folder — and the browser may add only a folder that already is a
// DIT workspace; registering any other repository is a terminal action.

import { useMemo, useState, type ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, FolderPlus, Plus, Star, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { addWorkspace, createWorkspace, removeWorkspace, setDefaultWorkspace } from "../lib/api";
import { useWorkspaces } from "../lib/queries";
import { workspaceHref, workspaceName } from "../lib/workspace";
import type { MenuItem } from "./chrome";

/** A workspace name from what someone typed: `My Project` → `my-project`.
 *  The server's rule: a lowercase word, starting with a letter. */
export function toWorkspaceName(typed: string): string {
  const word = typed
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
  if (word === "") return "";
  return /^[a-z]/.test(word) ? word : `ws-${word}`.slice(0, 48);
}

/** Leave this page for workspace `name`'s. A full load: everything the page
 *  holds — queries, live updates, remembered tabs — belongs to one workspace. */
export function openWorkspace(name: string): void {
  window.location.assign(workspaceHref(name));
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function Problem({ text }: { text: string | null }) {
  if (!text) return null;
  return (
    <div className="mw-errline">
      <AlertTriangle className="i" aria-hidden />
      <span>{text}</span>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The forms, shared by the first-run page and the dialogs
// ---------------------------------------------------------------------------

function NewWorkspaceForm({ root, footer }: { root: string; footer: (busy: boolean, ready: boolean) => ReactNode }) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const name = toWorkspaceName(typed);
  const submit = () => {
    if (!name) return;
    setBusy(true);
    setProblem(null);
    createWorkspace(name)
      .then(() => openWorkspace(name))
      .catch((e: unknown) => {
        setProblem(message(e));
        setBusy(false);
      });
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="mw-dlg-b">
        <div className="mw-fld">
          <label htmlFor="ws-new-name">Name</label>
          <input
            id="ws-new-name"
            value={typed}
            autoFocus
            spellCheck={false}
            placeholder="Acme website"
            onChange={(e) => setTyped(e.target.value)}
          />
          <p className="mw-hint" style={{ margin: 0 }}>
            {name ? (
              <>
                Saved as <code>{name}</code> in <code>{`${root}/${name}`}</code>
              </>
            ) : (
              <>A folder for it is made in <code>{root}</code> — nothing to set up.</>
            )}
          </p>
        </div>
        <Problem text={problem} />
      </div>
      {footer(busy, name !== "")}
    </form>
  );
}

function AddWorkspaceForm({ footer }: { footer: (busy: boolean, ready: boolean) => ReactNode }) {
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const submit = () => {
    if (!path.trim()) return;
    setBusy(true);
    setProblem(null);
    addWorkspace(path.trim())
      .then((added) => openWorkspace(added.name))
      .catch((e: unknown) => {
        setProblem(message(e));
        setBusy(false);
      });
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="mw-dlg-b">
        <div className="mw-fld">
          <label htmlFor="ws-add-path">Folder</label>
          <input
            id="ws-add-path"
            value={path}
            autoFocus
            spellCheck={false}
            placeholder="/Users/you/Documents/DIT/acme"
            onChange={(e) => setPath(e.target.value)}
          />
          <p className="mw-hint" style={{ margin: 0 }}>
            A folder that already is a DIT workspace — one made on another machine, or moved here. To track a code
            repository that is not one yet, run <code>dit workspace add &lt;folder&gt;</code> in a terminal.
          </p>
        </div>
        <Problem text={problem} />
      </div>
      {footer(busy, path.trim() !== "")}
    </form>
  );
}

// ---------------------------------------------------------------------------
// Dialogs
// ---------------------------------------------------------------------------

export type WorkspaceDialog = { kind: "new" } | { kind: "add" } | { kind: "remove"; name: string } | null;

export function WorkspaceDialogs({
  dialog,
  root,
  onClose,
}: {
  dialog: WorkspaceDialog;
  root: string;
  onClose: () => void;
}) {
  const footer = (label: string) => (busy: boolean, ready: boolean) => (
    <div className="mw-dlg-f">
      <button type="button" className="mw-btn" onClick={onClose}>
        Cancel
      </button>
      <button type="submit" className="mw-btn pri" disabled={!ready || busy}>
        {label}
      </button>
    </div>
  );
  const title =
    dialog?.kind === "new" ? "New workspace" : dialog?.kind === "add" ? "Add an existing workspace" : "Remove from the list";
  return (
    <Dialog.Root open={dialog !== null} onOpenChange={(open) => (open ? undefined : onClose())}>
      <Dialog.Portal>
        <Dialog.Overlay className="mw-scrim" />
        <Dialog.Content className="mw-dlg" aria-describedby={undefined}>
          <Dialog.Title className="mw-dlg-h">{title}</Dialog.Title>
          {dialog?.kind === "new" ? <NewWorkspaceForm root={root} footer={footer("Create")} /> : null}
          {dialog?.kind === "add" ? <AddWorkspaceForm footer={footer("Add")} /> : null}
          {dialog?.kind === "remove" ? <RemoveWorkspace name={dialog.name} onClose={onClose} /> : null}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function RemoveWorkspace({ name, onClose }: { name: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const current = workspaceName() === name;
  const submit = () => {
    setBusy(true);
    setProblem(null);
    removeWorkspace(name)
      .then(() => {
        if (current) {
          // This page's workspace is gone from the list: `/` goes to the default.
          window.location.assign("/");
          return;
        }
        void queryClient.invalidateQueries({ queryKey: ["workspaces"] });
        onClose();
      })
      .catch((e: unknown) => {
        setProblem(message(e));
        setBusy(false);
      });
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <div className="mw-dlg-b">
        <p style={{ margin: 0 }}>
          <code>{name}</code> leaves this machine's list of workspaces. Its folder, its issues and its history stay
          exactly where they are; add the folder again at any time.
        </p>
        <Problem text={problem} />
      </div>
      <div className="mw-dlg-f">
        <button type="button" className="mw-btn" onClick={onClose}>
          Cancel
        </button>
        <button type="submit" className="mw-btn pri" disabled={busy}>
          Remove from list
        </button>
      </div>
    </form>
  );
}

// ---------------------------------------------------------------------------
// First run
// ---------------------------------------------------------------------------

/** What `/` shows when this machine has no workspace yet. */
export function FirstRun({ root }: { root: string }) {
  const [mode, setMode] = useState<"new" | "add">("new");
  const footer = (label: string) => (busy: boolean, ready: boolean) => (
    <div className="mw-dlg-f">
      <button
        type="button"
        className="mw-btn"
        onClick={() => setMode(mode === "new" ? "add" : "new")}
      >
        {mode === "new" ? "Add an existing one instead" : "Make a new one instead"}
      </button>
      <button type="submit" className="mw-btn pri" disabled={!ready || busy}>
        {label}
      </button>
    </div>
  );
  return (
    <main className="flex min-h-dvh items-center justify-center bg-app p-6">
      <div className="mw-dlg ws-first">
        <h1 className="mw-dlg-h">Welcome to DIT</h1>
        <p className="ws-first-lede">
          A workspace holds one project's issues, documents and API scenarios.{" "}
          {mode === "new" ? "Give your first one a name." : "Point DIT at a workspace folder you already have."}
        </p>
        {mode === "new" ? <NewWorkspaceForm root={root} footer={footer("Create workspace")} /> : null}
        {mode === "add" ? <AddWorkspaceForm footer={footer("Add workspace")} /> : null}
      </div>
    </main>
  );
}

// ---------------------------------------------------------------------------
// The switcher, as items of the workspace menu
// ---------------------------------------------------------------------------

/** Menu items for switching and managing workspaces, and the dialogs they
 *  open. Empty on a server started for a single workspace, which has no list. */
export function useWorkspaceSwitcher(): { items: MenuItem[]; dialogs: ReactNode } {
  const list = useWorkspaces();
  const queryClient = useQueryClient();
  const [dialog, setDialog] = useState<WorkspaceDialog>(null);
  const current = workspaceName();
  const data = list.data;

  const items = useMemo<MenuItem[]>(() => {
    if (!data || current === null) return [];
    const isDefault = data.default === current;
    return [
      { kind: "head", label: "Workspaces" },
      ...data.workspaces.map(
        (w): MenuItem => ({
          label: w.name,
          on: w.name === current,
          meta: w.name === data.default ? "default" : undefined,
          run: () => {
            if (w.name !== current) openWorkspace(w.name);
          },
        }),
      ),
      { label: "New workspace…", icon: <Plus className="i" aria-hidden />, run: () => setDialog({ kind: "new" }) },
      {
        label: "Add existing workspace…",
        icon: <FolderPlus className="i" aria-hidden />,
        run: () => setDialog({ kind: "add" }),
      },
      ...(isDefault
        ? []
        : [
            {
              label: "Open this one first",
              icon: <Star className="i" aria-hidden />,
              run: () => {
                setDefaultWorkspace(current)
                  .then(() => {
                    toast.success(`${current} now opens first`);
                    return queryClient.invalidateQueries({ queryKey: ["workspaces"] });
                  })
                  .catch((e: unknown) => toast.error(message(e)));
              },
            } satisfies MenuItem,
          ]),
      {
        label: "Remove from list…",
        icon: <X className="i" aria-hidden />,
        run: () => setDialog({ kind: "remove", name: current }),
      },
      { kind: "sep" },
    ];
  }, [data, current, queryClient]);

  const dialogs = data ? (
    <WorkspaceDialogs dialog={dialog} root={data.root} onClose={() => setDialog(null)} />
  ) : null;
  return { items, dialogs };
}
