// The workspaces on this machine (ADR 0028): the first-run page, and the
// dialogs the workspace menu opens. A new workspace is a name — the server
// picks the folder — and the browser may add only a folder that already is a
// DIT workspace; registering any other repository is a terminal action.

import { useMemo, useState, type ReactNode } from "react";
import * as Dialog from "@radix-ui/react-dialog";
import { AlertTriangle, Check, FolderOpen, FolderPlus, Plus, Star, X } from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { addWorkspace, chooseFolder, createWorkspace, removeWorkspace, setDefaultWorkspace } from "../lib/api";
import type { FolderPurpose } from "../lib/types";
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

/** "Choose folder…": the system's own dialog, opened by the local server
 *  (ADR 0030). Hands over the folder picked and whether it already is a
 *  DIT workspace; a cancelled dialog hands over nothing. */
function useChooseFolder(purpose: FolderPurpose, onProblem: (text: string) => void) {
  const [choosing, setChoosing] = useState(false);
  const choose = (onChosen: (path: string, isWorkspace: boolean) => void) => {
    setChoosing(true);
    chooseFolder(purpose)
      .then((chosen) => {
        if (chosen.path) onChosen(chosen.path, chosen.is_workspace);
      })
      .catch((e: unknown) => onProblem(message(e)))
      .finally(() => setChoosing(false));
  };
  return { choosing, choose };
}

type FormProps = {
  /** Whether "Choose folder…" can open the system's dialog. */
  canChoose: boolean;
  footer: (busy: boolean, ready: boolean) => ReactNode;
};

function ChooseButton({ choosing, onClick }: { choosing: boolean; onClick: () => void }) {
  return (
    <button type="button" className="mw-btn" disabled={choosing} onClick={onClick}>
      <FolderOpen className="i" aria-hidden />
      {choosing ? "Choosing…" : "Choose folder…"}
    </button>
  );
}

function NewWorkspaceForm({ root, canChoose, footer }: FormProps & { root: string }) {
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /** A chosen folder, or null for the default one. */
  const [at, setAt] = useState<string | null>(null);
  /** The chosen folder already is a workspace: nothing is made inside it. */
  const [atIsWorkspace, setAtIsWorkspace] = useState(false);
  const { choosing, choose } = useChooseFolder("new", setProblem);
  const name = toWorkspaceName(typed);
  const location = at ?? root;
  const submit = () => {
    if (!name || atIsWorkspace) return;
    setBusy(true);
    setProblem(null);
    createWorkspace(name, at ?? undefined)
      .then(() => openWorkspace(name))
      .catch((e: unknown) => {
        setProblem(message(e));
        setBusy(false);
      });
  };
  const addInstead = () => {
    if (!at) return;
    setBusy(true);
    setProblem(null);
    addWorkspace(at)
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
          <label htmlFor="ws-new-name">Name</label>
          <input
            id="ws-new-name"
            value={typed}
            autoFocus
            spellCheck={false}
            placeholder="Acme website"
            onChange={(e) => setTyped(e.target.value)}
          />
        </div>
        <div className="mw-fld">
          <div className="ws-loc-head">
            <span className="ws-loc-label">Location</span>
            {at !== null ? (
              <button
                type="button"
                className="ws-link"
                onClick={() => {
                  setAt(null);
                  setAtIsWorkspace(false);
                }}
              >
                Use the default folder
              </button>
            ) : null}
          </div>
          <div className="ws-loc">
            <code>{location}</code>
            {canChoose ? (
              <ChooseButton
                choosing={choosing}
                onClick={() =>
                  choose((path, isWorkspace) => {
                    setAt(path);
                    setAtIsWorkspace(isWorkspace);
                  })
                }
              />
            ) : null}
          </div>
          {atIsWorkspace ? (
            <div className="ws-note">
              <span>
                This folder already is a DIT workspace. A new one is not made inside another — add this one to
                the list instead, or choose a different folder.
              </span>
              <button type="button" className="mw-btn pri" disabled={busy} onClick={addInstead}>
                Add this workspace
              </button>
            </div>
          ) : (
            <p className="mw-hint" style={{ margin: 0 }}>
              {name ? (
                <>
                  Made as <code>{`${location}/${name}`}</code> — nothing to set up.
                </>
              ) : (
                <>A folder named after the workspace is made here.</>
              )}
            </p>
          )}
        </div>
        <Problem text={problem} />
      </div>
      {footer(busy || choosing, name !== "" && !atIsWorkspace)}
    </form>
  );
}

function AddWorkspaceForm({ canChoose, footer }: FormProps) {
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  /** What the dialog said about the folder it handed over; null once typed. */
  const [isWorkspace, setIsWorkspace] = useState<boolean | null>(null);
  const { choosing, choose } = useChooseFolder("add", setProblem);
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
          <div className="ws-loc">
            <input
              id="ws-add-path"
              value={path}
              autoFocus
              spellCheck={false}
              placeholder="/Users/you/Documents/DIT/acme"
              onChange={(e) => {
                setPath(e.target.value);
                setIsWorkspace(null);
              }}
            />
            {canChoose ? (
              <ChooseButton
                choosing={choosing}
                onClick={() =>
                  choose((chosen, workspace) => {
                    setPath(chosen);
                    setIsWorkspace(workspace);
                  })
                }
              />
            ) : null}
          </div>
          {isWorkspace === false ? (
            <div className="mw-errline">
              <AlertTriangle className="i" aria-hidden />
              <span>
                This folder is not a DIT workspace. Make a new one with New workspace, or add a code repository from a
                terminal with <code>dit workspace add &lt;folder&gt;</code>.
              </span>
            </div>
          ) : (
            <p className="mw-hint" style={{ margin: 0 }}>
              A folder that already is a DIT workspace — one made on another machine, or moved here. To track a code
              repository that is not one yet, run <code>dit workspace add &lt;folder&gt;</code> in a terminal.
            </p>
          )}
        </div>
        <Problem text={problem} />
      </div>
      {footer(busy || choosing, path.trim() !== "" && isWorkspace !== false)}
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
  canChoose,
  onClose,
}: {
  dialog: WorkspaceDialog;
  root: string;
  canChoose: boolean;
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
          {dialog?.kind === "new" ? (
            <NewWorkspaceForm root={root} canChoose={canChoose} footer={footer("Create")} />
          ) : null}
          {dialog?.kind === "add" ? <AddWorkspaceForm canChoose={canChoose} footer={footer("Add")} /> : null}
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
export function FirstRun({ root, canChoose }: { root: string; canChoose: boolean }) {
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
        {mode === "new" ? <NewWorkspaceForm root={root} canChoose={canChoose} footer={footer("Create workspace")} /> : null}
        {mode === "add" ? <AddWorkspaceForm canChoose={canChoose} footer={footer("Add workspace")} /> : null}
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
          // A check on the one this page shows; the rest keep its space so
          // the names line up.
          icon: w.name === current ? <Check className="i" aria-hidden /> : <span className="i" aria-hidden />,
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
    <WorkspaceDialogs
      dialog={dialog}
      root={data.root}
      canChoose={data.can_choose_folder}
      onClose={() => setDialog(null)}
    />
  ) : null;
  return { items, dialogs };
}
