// One file in focus, across the whole main area: who imports it on the left,
// the file itself as a card in the middle, what it imports on the right.
// Three fixed columns rather than a force layout, because the question here
// is "what touches this", and a picture that reads the same way every time
// answers it faster than one that moves. Each side shows its twelve most
// depended-on neighbours; the rest are one click away.

import { useEffect, useRef, useState } from "react";
import { AlertTriangle, FolderOpen } from "lucide-react";
import { ApiError } from "../../lib/api";
import {
  apiCountLabel,
  apiSummary,
  capNeighbours,
  charsFor,
  connector,
  FOCUS_GEO,
  focusGeometry,
  proofLabel,
  shortenPath,
} from "../../lib/codemap";
import { useCodeNode } from "../../lib/queries";
import type { CodeApiCallDto, CodeNeighbourDto, CodeNeighbourhoodDto } from "../../lib/types";
import { Verb } from "../morse/common";
import { Empty, ErrorBox, Loading } from "../../components/states";
import { cn } from "../../lib/cn";

export function FocusView({
  focus,
  onFocus,
  onReveal,
}: {
  focus: string | null;
  onFocus: (name: string) => void;
  /** Open the folder the focused file sits in, in the folder view. */
  onReveal: (node: CodeNeighbourhoodDto) => void;
}) {
  const node = useCodeNode(focus);
  if (focus === null) {
    return <Empty title="Nothing in focus" hint="Click a file in the folder map, or type a path or an exported name above." />;
  }
  if (node.isPending) return <Loading label={`Reading ${focus}…`} />;
  if (node.isError) {
    return node.error instanceof ApiError && node.error.status === 404 ? (
      <Empty title={`Nothing indexed is named “${focus}”`} hint="Try a file path relative to its root, or an exported symbol." />
    ) : (
      <ErrorBox error={node.error} onRetry={() => void node.refetch()} />
    );
  }
  // Keyed by path so a new focus starts with both sides folded again.
  return <Neighbourhood key={node.data.path} node={node.data} onFocus={onFocus} onReveal={onReveal} />;
}

function Neighbourhood({
  node,
  onFocus,
  onReveal,
}: {
  node: CodeNeighbourhoodDto;
  onFocus: (name: string) => void;
  onReveal: (node: CodeNeighbourhoodDto) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(1100);
  const [open, setOpen] = useState<{ users: boolean; uses: boolean }>({ users: false, uses: false });

  useEffect(() => {
    const element = host.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry && entry.contentRect.width > 0) setWidth(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const users = capNeighbours(node.users);
  const uses = capNeighbours(node.uses);
  const rows = (side: typeof users) => side.shown.length + (side.hidden > 0 ? 1 : 0);
  const g = focusGeometry(width, rows(users), rows(uses));
  const leftX = g.sideW;
  const cardL = g.sideW + FOCUS_GEO.gap;
  const cardR = cardL + g.centerW;
  const rightX = cardR + FOCUS_GEO.gap;
  const chars = charsFor(g.sideW - 56, 6.6);

  // One connector per pill; an expanded side scrolls inside its column, so
  // it is drawn as one bundle from the column's middle instead.
  const lines: string[] = [];
  const side = (list: typeof users, expanded: boolean, from: "left" | "right") => {
    const count = rows(list);
    if (count === 0) return;
    if (expanded) {
      const y = (from === "left" ? g.leftTop : g.rightTop) + (count * FOCUS_GEO.rowH) / 2;
      lines.push(from === "left" ? connector(leftX, y, cardL, g.anchorY) : connector(cardR, g.anchorY, rightX, y));
      return;
    }
    for (let i = 0; i < count; i += 1) {
      const y = g.pillY(from, i);
      lines.push(from === "left" ? connector(leftX, y, cardL, g.anchorY) : connector(cardR, g.anchorY, rightX, y));
    }
  };
  side(users, open.users, "left");
  side(uses, open.uses, "right");

  return (
    <div ref={host} className="min-h-0 flex-1 overflow-auto px-6 py-5">
      <div className="mx-auto" style={{ width: g.width }}>
        <div
          className="mb-2 grid text-[10.5px] font-medium tracking-wide text-faint uppercase"
          style={{ gridTemplateColumns: `${g.sideW}px ${g.centerW}px ${g.sideW}px`, columnGap: FOCUS_GEO.gap }}
        >
          <span>
            Used by <span className="font-normal">{node.users.length}</span>
          </span>
          <span className="text-center">This file</span>
          <span className="text-right">
            Uses <span className="font-normal">{node.uses.length}</span>
          </span>
        </div>
        <div
          className="relative grid items-start"
          style={{ gridTemplateColumns: `${g.sideW}px ${g.centerW}px ${g.sideW}px`, columnGap: FOCUS_GEO.gap }}
        >
          <svg className="pointer-events-none absolute inset-0 overflow-visible" width={g.width} height="100%" aria-hidden>
            {lines.map((d, i) => (
              // Positional keys: the list is rebuilt whole on every render.
              <path key={i} d={d} fill="none" stroke="var(--dit-ctl)" strokeWidth={1.2} />
            ))}
          </svg>
          <Column
            label="Used by"
            list={users}
            top={g.leftTop}
            expanded={open.users}
            chars={chars}
            align="right"
            onToggle={() => setOpen((o) => ({ ...o, users: !o.users }))}
            onFocus={onFocus}
          />
          <div style={{ marginTop: g.cardTop }}>
            <Card node={node} onFocus={onFocus} onReveal={onReveal} />
          </div>
          <Column
            label="Uses"
            list={uses}
            top={g.rightTop}
            expanded={open.uses}
            chars={chars}
            align="left"
            onToggle={() => setOpen((o) => ({ ...o, uses: !o.uses }))}
            onFocus={onFocus}
          />
        </div>
        {node.users.length === 0 && node.uses.length === 0 ? (
          <p className="mt-4 text-center text-[12px] text-faint">No file in its root imports it, and it imports none.</p>
        ) : null}
        {node.api_calls.length > 0 ? <ApiCalls calls={node.api_calls} /> : null}
      </div>
    </div>
  );
}

/** The API paths the file calls, each against what the registered specs
 *  say: the operations it reaches and where a scenario proves them — or,
 *  for an orphan, that no spec describes it at all. */
function ApiCalls({ calls }: { calls: CodeApiCallDto[] }) {
  const { orphans } = apiSummary(calls);
  return (
    <section className="mt-8" aria-label="API calls">
      <h3 className="mb-2 text-[11px] font-semibold tracking-wide text-muted uppercase">
        API calls <span className="font-normal text-faint">{calls.length}</span>
        {orphans > 0 ? <span className="ml-2 font-normal normal-case text-crit-text">{orphans} orphan{orphans === 1 ? "" : "s"}</span> : null}
      </h3>
      <ul className="divide-y divide-rowline overflow-hidden rounded-lg border border-edge bg-card">
        {calls.map((call) => (
          <li key={`${call.line}:${call.path}`} className="flex flex-wrap items-start gap-x-4 gap-y-1.5 px-3 py-2">
            <div className="flex min-w-0 basis-80 items-baseline gap-2">
              <span className="w-10 shrink-0 text-right font-mono text-[11px] text-faint" title={`line ${call.line}`}>
                :{call.line}
              </span>
              {/* Break after a slash, never inside a segment: `…/complete` stays whole. */}
              <span className="min-w-0 break-words font-mono text-[12px] text-ink">
                {call.path.split("/").map((seg, i, all) => (
                  <span key={`${i}:${seg}`}>
                    {seg}
                    {i < all.length - 1 ? (
                      <>
                        /<wbr />
                      </>
                    ) : null}
                  </span>
                ))}
              </span>
            </div>
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              {call.operations.length === 0 ? (
                <span className="flex items-center gap-1.5 text-[11.5px] text-crit-text">
                  <AlertTriangle className="size-3.5 shrink-0" aria-hidden />
                  no spec describes this — it may answer 404
                </span>
              ) : (
                call.operations.map((op) => (
                  <div key={`${op.spec}/${op.operation_id}`} className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-0.5">
                    <Verb method={op.method} />
                    <span className="min-w-0 break-all font-mono text-[11.5px] text-ink-2">{op.path}</span>
                    <span className="font-mono text-[10.5px] text-faint">
                      {op.spec}/{op.operation_id}
                    </span>
                    <span
                      className={cn(
                        "rounded-full border px-1.5 text-[10.5px]",
                        op.proven.length > 0
                          ? "border-done-line bg-done-bg text-done-text"
                          : "border-warn-line bg-warn-bg text-warn-text",
                      )}
                    >
                      {proofLabel(op.proven)}
                    </span>
                  </div>
                ))
              )}
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

function Column({
  label,
  list,
  top,
  expanded,
  chars,
  align,
  onToggle,
  onFocus,
}: {
  label: string;
  list: ReturnType<typeof capNeighbours<CodeNeighbourDto>>;
  top: number;
  expanded: boolean;
  chars: number;
  align: "left" | "right";
  onToggle: () => void;
  onFocus: (name: string) => void;
}) {
  const count = list.shown.length + (list.hidden > 0 ? 1 : 0);
  if (count === 0) {
    return (
      <p className={cn("text-[11.5px] text-faint", align === "right" && "text-right")} style={{ marginTop: top }}>
        {label === "Uses" ? "imports no file in its root" : "no file in its root imports it"}
      </p>
    );
  }
  const items = expanded ? list.all : list.shown;
  return (
    <ul
      aria-label={label}
      className={cn("relative flex flex-col", expanded && "overflow-y-auto rounded-md border border-edge bg-panel")}
      style={{ marginTop: top, maxHeight: expanded ? count * FOCUS_GEO.rowH : undefined }}
    >
      {items.map((n) => (
        <li key={n.path} className="flex items-center" style={{ height: FOCUS_GEO.rowH }}>
          <Pill n={n} chars={chars} onFocus={onFocus} />
        </li>
      ))}
      {list.hidden > 0 ? (
        <li className="flex items-center" style={{ height: FOCUS_GEO.rowH }}>
          <button
            type="button"
            onClick={onToggle}
            className="flex w-full items-center justify-center rounded-full border border-dashed border-ctl bg-app px-3 text-[11.5px] text-muted hover:border-dim hover:text-ink"
            style={{ height: FOCUS_GEO.pillH }}
          >
            {expanded ? "show the top 12" : `+${list.hidden} more`}
          </button>
        </li>
      ) : null}
    </ul>
  );
}

function Pill({ n, chars, onFocus }: { n: CodeNeighbourDto; chars: number; onFocus: (name: string) => void }) {
  const names = n.names.length > 0 ? `\nimports: ${n.names.join(", ")}` : "";
  const via = n.via ? `\nvia ${n.via}` : "";
  return (
    <button
      type="button"
      onClick={() => onFocus(n.path)}
      title={`${n.path}${n.generated ? " (generated)" : ""}\n${n.users} file${n.users === 1 ? "" : "s"} import it${names}${via}`}
      aria-label={`Focus ${n.path}`}
      className={cn(
        "flex w-full min-w-0 items-center gap-2 rounded-full border px-3 text-left hover:border-accent focus-visible:border-accent focus-visible:outline-none",
        n.generated ? "border-dashed border-ctl bg-sunken opacity-70" : "border-ctl bg-card",
      )}
      style={{ height: FOCUS_GEO.pillH }}
    >
      <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-ink-2">
        {shortenPath(n.path, n.via ? Math.max(8, chars - 8) : chars)}
      </span>
      {n.via ? <span className="shrink-0 text-[10px] text-faint">via {shortenPath(n.via, 10)}</span> : null}
      <span className="shrink-0 rounded-full bg-sunken px-1.5 font-mono text-[10px] text-muted" title="files importing it">
        {n.users}
      </span>
    </button>
  );
}

function Card({
  node,
  onFocus,
  onReveal,
}: {
  node: CodeNeighbourhoodDto;
  onFocus: (name: string) => void;
  onReveal: (node: CodeNeighbourhoodDto) => void;
}) {
  return (
    <div
      className={cn(
        "rounded-lg border-2 bg-card p-3 shadow-sm",
        node.generated ? "border-dashed border-warn-line" : "border-accent",
      )}
    >
      <div className="flex items-start justify-between gap-2" style={{ minHeight: FOCUS_GEO.anchor * 2 - 24 }}>
        <div className="min-w-0">
          <div className="break-all font-mono text-[12.5px] font-semibold text-ink" title={node.path}>
            {node.path}
          </div>
          <div className="mt-0.5 text-[11.5px] text-muted">
            <span className="font-mono">{node.root}</span> · {node.users.length} user{node.users.length === 1 ? "" : "s"} ·{" "}
            {node.uses.length} import{node.uses.length === 1 ? "" : "s"}
            {apiCountLabel(node.api_calls)}
          </div>
        </div>
        <button type="button" className="btn shrink-0" onClick={() => onReveal(node)} title="Show its folder in the map">
          <FolderOpen className="i" aria-hidden />
          Folder
        </button>
      </div>
      {node.generated ? (
        <div className="mt-2 flex items-center gap-2 rounded-md border border-warn-line bg-warn-bg px-2.5 py-1.5 text-[11.5px] text-warn-text">
          <AlertTriangle className="size-3.5 shrink-0" aria-hidden />
          generated — edit its source, not this file
        </div>
      ) : null}
      <Section title="Defines" count={node.defines.length} empty="exports nothing">
        {node.defines.map((name) => (
          <button
            key={name}
            type="button"
            onClick={() => onFocus(name)}
            className="rounded border border-edge bg-app px-1.5 py-0.5 font-mono text-[11px] text-ink-2 hover:border-dim hover:text-ink"
            title={`Focus the file that defines ${name}`}
          >
            {name}
          </button>
        ))}
      </Section>
      <Section title="External" count={node.external.length} empty="imports no packages">
        {node.external.map((name) => (
          <span key={name} className="rounded bg-sunken px-1.5 py-0.5 font-mono text-[11px] text-muted">
            {name}
          </span>
        ))}
      </Section>
    </div>
  );
}

function Section({
  title,
  count,
  empty,
  children,
}: {
  title: string;
  count: number;
  empty: string;
  children: React.ReactNode;
}) {
  return (
    <section className="mt-3">
      <h3 className="mb-1.5 text-[11px] font-semibold tracking-wide text-muted uppercase">
        {title} <span className="font-normal text-faint">{count}</span>
      </h3>
      {count === 0 ? <p className="text-[11.5px] text-faint">{empty}</p> : <div className="flex flex-wrap gap-1">{children}</div>}
    </section>
  );
}
