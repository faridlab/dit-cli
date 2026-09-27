// The code map screen: one folder of one code root drawn as a graph of who
// imports what, or one file in focus with both of its sides — a toggle
// between the two, with the search box beside it.
//
// Everything worth sharing — the root, the folder, the file in focus, which
// of the two is showing — lives in the route, so a link or a screenshot
// reopens the same reading. What is only this visit's — the back history of
// the focus — stays here.

import { useCallback, useEffect, useState } from "react";
import { AlertTriangle, ArrowLeft, ChevronRight, Search } from "lucide-react";
import { ApiError } from "../lib/api";
import { folderCrumbs, halfOf, parentFolder } from "../lib/codemap";
import { useCodeOverview, useCodeRoots } from "../lib/queries";
import type { Route } from "../lib/router";
import type { CodeRootDto } from "../lib/types";
import type { CodeHalf } from "../lib/codemap";
import { cn } from "../lib/cn";
import { IBtn, INPUT_CLASS } from "../components/chrome";
import { SelectField } from "../components/SelectField";
import { Empty, ErrorBox, Loading } from "../components/states";
import { FocusView } from "./code/FocusView";
import { FolderGraph } from "./code/FolderGraph";

type CodeRoute = Extract<Route, { name: "code" }>;
type Half = CodeHalf;

/** How many files the Back button remembers. */
const BACK_DEPTH = 20;

const CONFIG_SNIPPET = `code:
  - { id: web, include: ["src/**"], generated: ["src/generated/**"] }
  # a linked repository (named under repos:), pinned to a branch:
  # - { id: api, repo: backend, include: ["src/**"], ref: main }`;

function rootLabel(root: CodeRootDto): string {
  const where = root.repo ? `${root.repo}${root.git_ref ? `@${root.git_ref}` : ""}` : root.git_ref ? `@${root.git_ref}` : "this repo";
  return `${root.id} · ${where} · ${root.files} file${root.files === 1 ? "" : "s"}`;
}

export function CodeView({
  root,
  folder,
  focus,
  view,
  onGo,
}: {
  root: string | null;
  folder: string | null;
  focus: string | null;
  view: Half | null;
  /** Move to another reading of the map. */
  onGo: (route: CodeRoute) => void;
}) {
  const roots = useCodeRoots();
  const list = roots.data?.roots ?? [];
  const active = list.find((r) => r.id === root) ?? list[0] ?? null;
  const at = folder ?? "";
  const half = halfOf(view, focus);
  const overview = useCodeOverview(half === "folder" ? (active?.id ?? null) : null, at);
  const [back, setBack] = useState<string[]>([]);
  const [text, setText] = useState(focus ?? "");
  useEffect(() => setText(focus ?? ""), [focus]);

  const go = useCallback(
    (patch: Partial<Omit<CodeRoute, "name">>) =>
      onGo({ name: "code", root: active?.id ?? root, folder: at, focus, view: half, ...patch }),
    [active?.id, at, focus, half, onGo, root],
  );

  const focusOn = useCallback(
    (name: string) => {
      if (name === focus) {
        go({ view: "focus" });
        return;
      }
      if (focus !== null) setBack((stack) => [...stack, focus].slice(-BACK_DEPTH));
      go({ focus: name, view: "focus" });
    },
    [focus, go],
  );

  const goBack = useCallback(() => {
    const previous = back[back.length - 1];
    if (previous === undefined) return;
    setBack((stack) => stack.slice(0, -1));
    go({ focus: previous, view: "focus" });
  }, [back, go]);

  if (roots.isPending) return <Loading label="Bringing the map up to HEAD…" className="flex-1" />;
  if (roots.isError) return <ErrorBox error={roots.error} onRetry={() => void roots.refetch()} title="Could not read the code map" />;
  if (active === null) return <NoRoots problems={roots.data.problems} />;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-3 border-b border-edge px-4 py-2">
        <SelectField
          ariaLabel="Code root"
          value={active.id}
          options={list.map((r) => ({ value: r.id, label: rootLabel(r) }))}
          onChange={(id) => {
            setBack([]);
            onGo({ name: "code", root: id, folder: null, focus: null, view: "folder" });
          }}
          className="w-auto min-w-56 font-mono"
        />
        <div className="seg" style={{ marginLeft: 0 }} role="tablist" aria-label="Show">
          {(["folder", "focus"] as const).map((h) => (
            <button
              key={h}
              type="button"
              role="tab"
              aria-selected={half === h}
              className={cn(half === h && "on")}
              onClick={() => go({ view: h })}
            >
              {h === "folder" ? "Folder" : "Focus"}
            </button>
          ))}
        </div>
        <nav aria-label="Folder" className="flex min-w-0 flex-wrap items-center gap-0.5 font-mono text-[12.5px]">
          {folderCrumbs(active.id, at).map((crumb, index, all) => {
            const last = index === all.length - 1;
            return (
              <span key={crumb.folder || "/"} className="flex items-center gap-0.5">
                {index > 0 ? <ChevronRight className="size-3.5 text-faint" aria-hidden /> : null}
                <button
                  type="button"
                  onClick={() => go({ folder: crumb.folder, view: "folder" })}
                  aria-current={last && half === "folder" ? "location" : undefined}
                  className={cn(
                    "rounded px-1 py-0.5 hover:bg-hover hover:text-ink",
                    last && half === "folder" ? "font-semibold text-ink" : "text-muted",
                  )}
                >
                  {crumb.label}
                </button>
              </span>
            );
          })}
        </nav>
        <span className="flex-1" />
        <form
          className="flex items-center gap-1.5"
          onSubmit={(ev) => {
            ev.preventDefault();
            const name = text.trim();
            if (name.length > 0) focusOn(name);
          }}
        >
          <IBtn onClick={goBack} disabled={back.length === 0} title="Back to the previous file" aria-label="Back to the previous file">
            <ArrowLeft className="i" aria-hidden />
          </IBtn>
          <label className="relative">
            <Search className="pointer-events-none absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-faint" aria-hidden />
            <input
              className={cn(INPUT_CLASS, "w-80 pl-7 font-mono")}
              value={text}
              onChange={(ev) => setText(ev.target.value)}
              placeholder="File path or symbol, then Enter"
              aria-label="Focus a file path or symbol"
              spellCheck={false}
            />
          </label>
        </form>
      </div>

      {roots.data.problems.length > 0 ? <Problems problems={roots.data.problems} /> : null}

      <section className="flex min-h-0 flex-1 flex-col bg-app" aria-label={half === "folder" ? "Folder map" : "Focus"}>
        {half === "focus" ? (
          <FocusView
            focus={focus}
            onFocus={focusOn}
            onReveal={(node) =>
              onGo({ name: "code", root: node.root, folder: parentFolder(node.path), focus: node.path, view: "folder" })
            }
          />
        ) : overview.isPending ? (
          <Loading label={`Reading ${at || active.id}…`} />
        ) : overview.isError ? (
          <div>
            <ErrorBox
              error={overview.error}
              title={overview.error instanceof ApiError && overview.error.status === 404 ? "No such folder in this root" : "Could not read the folder"}
              onRetry={() => void overview.refetch()}
            />
            {at !== "" ? (
              <button type="button" className="btn mx-4" onClick={() => go({ folder: "" })}>
                Back to the top of {active.id}
              </button>
            ) : null}
          </div>
        ) : overview.data.units.length === 0 ? (
          <Empty title="Nothing indexed in this folder" hint="The root's include rules may leave it out." />
        ) : (
          <FolderGraph
            overview={overview.data}
            focus={focus}
            onFolder={(path) => go({ folder: path, view: "folder" })}
            onFile={focusOn}
          />
        )}
      </section>
    </div>
  );
}

function Problems({ problems }: { problems: string[] }) {
  return (
    <div role="alert" className="flex items-start gap-2 border-b border-warn-line bg-warn-bg px-4 py-2 text-[12px] text-warn-text">
      <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <ul className="min-w-0 space-y-0.5">
        {problems.map((p) => (
          <li key={p} className="font-mono text-[11.5px] text-warn-text-dim">
            {p}
          </li>
        ))}
      </ul>
    </div>
  );
}

function NoRoots({ problems }: { problems: string[] }) {
  return (
    <div className="mx-auto flex max-w-xl flex-col gap-3 p-10">
      {problems.length > 0 ? <Problems problems={problems} /> : null}
      <h2 className="text-[15px] font-semibold text-ink">No code roots are registered</h2>
      <p className="text-[13px] text-ink-2">
        The code map reads the roots listed under <code className="font-mono">code:</code> in{" "}
        <code className="font-mono">.dit/config.yaml</code>. Add one, commit, and reopen this screen:
      </p>
      <pre className="overflow-x-auto rounded-md border border-edge bg-sunken p-3 font-mono text-[12px] text-ink">
        {CONFIG_SNIPPET}
      </pre>
      <p className="text-[12px] text-muted">
        <code className="font-mono">include</code> picks the files to map, <code className="font-mono">generated</code> marks the
        ones that are generated (indexed, but never the place to edit), and <code className="font-mono">ref</code> pins a branch or
        tag.
      </p>
    </div>
  );
}
