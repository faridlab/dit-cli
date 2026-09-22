// The Flow screen (ADR 0019): an orchestration rendered as a diagram —
// issues as nodes on a computed stage grid, `blocked_by` as orthogonal
// edges, lanes as horizontal bands, the critical path emphasized.
//
// The visual language is archify's, ported from its generated artifacts
// (github.com/tt-a1i/archify), not re-imagined: dotted background grid,
// dashed lane bands labelled "01 / Name" top-left, phase headers as
// mask-chip captions over rules above the columns, nodes as an opaque mask
// rect under a semantic fill/stroke pair (ready=green, in-flight=cyan,
// blocked=rose, done=slate) with a status sigil, centered label and
// sublabel, and orthogonal edges with per-variant arrowheads (default,
// emphasis for the main path, dashed for satisfied, rose-dashed for
// broken). Mono type throughout, like the source.
//
// Everything on screen is derived server-side and never written back; the
// screen is read-only, live on any process's write.

import { useMemo, useState } from "react";
import { Waypoints } from "lucide-react";
import { useFlows, useFlowBoard } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import type { FlowBoardDto, FlowEdgeDto, FlowNodeDto } from "../lib/types";

/** The union pseudo-flow the API understands. */
const ALL = "__all__";

const NODE_W = 148;
const NODE_H = 52;
const GAP_X = 60;
const GAP_Y = 24;
const LANE_PAD_X = 28;
const LANE_HEAD = 26;
const LANE_GAP = 14;
const PHASE_HEAD = 34;
const R = 6;

type Semantic = "ready" | "doing" | "blocked" | "done";

function semanticOf(n: FlowNodeDto): Semantic {
  if (n.category === "done") return "done";
  if (n.readiness === "ready") return "ready";
  if (n.readiness === "blocked") return "blocked";
  return "doing";
}

export function FlowView({ onOpen }: { onOpen: (id: string) => void }) {
  const flows = useFlows();
  const [active, setActive] = useState<string>(ALL);
  const board = useFlowBoard(active);

  useRegisterPeekList(
    useMemo(
      () =>
        (board.data?.nodes ?? [])
          .filter((n) => n.readiness === "ready")
          .map((n) => n.short_ref),
      [board.data],
    ),
  );

  if (flows.isPending || (board.isPending && flows.data)) {
    return <Loading label="Loading flow…" className="flex-1" />;
  }
  if (flows.isError) {
    return (
      <ErrorBox
        error={flows.error}
        title="Could not load the flows"
        onRetry={() => void flows.refetch()}
      />
    );
  }
  if ((flows.data ?? []).length === 0) {
    return (
      <div className="wflow">
        <div className="ftitle">
          <Waypoints className="i" aria-hidden />
          Flows
        </div>
        <p className="empty" style={{ padding: "18px 4px" }}>
          No flows yet. An orchestration exists the moment an issue joins it:
          <br />
          <span className="mono">dit issue set '#12' flows=launch</span>
          <br />
          One issue may join several flows at once; dependencies
          (<span className="mono">blocked_by</span>) become the arrows.
        </p>
      </div>
    );
  }
  if (board.isError) {
    return (
      <ErrorBox
        error={board.error}
        title="Could not load the flow diagram"
        onRetry={() => void board.refetch()}
      />
    );
  }

  const data = board.data;
  const mainHops = Math.max(0, (data?.main_path.length ?? 1) - 1);

  return (
    <div className="wflow">
      <div className="fhead">
        <div className="ftitle">
          <Waypoints className="i" aria-hidden />
          {data?.name ?? "all flows"}
          <span className="sub">
            {data ? `${data.nodes.length} nodes · ${data.stages} stages · ${mainHops}-hop critical path` : ""}
          </span>
        </div>
        <div className="seg" role="group" aria-label="Flow">
          {flows.data?.map((f) => (
            <button
              key={f.name}
              type="button"
              className={cn("actf", active === f.name && "on")}
              aria-pressed={active === f.name}
              onClick={() => setActive(f.name)}
              title={`Show the ${f.name} flow`}
            >
              {f.name}
              <span className="cnt">{f.issues}</span>
            </button>
          ))}
          <button
            type="button"
            className={cn("actf", active === ALL && "on")}
            aria-pressed={active === ALL}
            onClick={() => setActive(ALL)}
            title="Every flow's members together"
          >
            all
          </button>
        </div>
      </div>
      {data ? <Diagram board={data} onOpen={onOpen} /> : null}
      <footer className="wfoot">
        <span className="legend">
          nodes
          <i className="nd ready" /> ready
          <i className="nd doing" /> in flight
          <i className="nd blocked" /> blocked
          <i className="nd done" /> done
          · arrows
          <i className="sw" /> in flight
          <i className="sw dashed" /> through the gate
          <i className="sw broken" /> cancelled/gone
          <i className="sw emph" /> critical path
          · stages derive from blocked_by — the deeper, the later
        </span>
      </footer>
    </div>
  );
}

function Diagram({ board, onOpen }: { board: FlowBoardDto; onOpen: (id: string) => void }) {
  const layout = useMemo(() => layoutBoard(board), [board]);
  const mainEdges = useMemo(() => {
    const set = new Set<string>();
    for (let i = 0; i + 1 < board.main_path.length; i += 1) {
      set.add(`${board.main_path[i]}>${board.main_path[i + 1]}`);
    }
    return set;
  }, [board.main_path]);

  return (
    <div className="fscroll">
      <svg
        width={layout.width}
        height={layout.height}
        viewBox={`0 0 ${layout.width} ${layout.height}`}
        role="img"
        aria-label={`Flow diagram: ${board.name ?? "all flows"}`}
        style={{ display: "block" }}
      >
        <defs>
          {(Object.keys(MARKERS) as Array<keyof typeof MARKERS>).map((k) => (
            <marker
              key={k}
              id={`wf-arrow-${k}`}
              markerWidth="10"
              markerHeight="7"
              refX="9"
              refY="3.5"
              orient="auto"
            >
              <polygon points="0 0, 10 3.5, 0 7" className={MARKERS[k]} />
            </marker>
          ))}
        </defs>

        {/* dotted ground grid, archify's c-grid */}
        <rect x="0" y="0" width={layout.width} height={layout.height} className="wf-c-grid" />

        {/* lane bands: dashed rounded frames, numbered captions */}
        {layout.lanes.map((lane, i) => (
          <g key={lane.key}>
            <rect
              x={0}
              y={lane.top}
              width={layout.width}
              height={lane.height}
              rx={10}
              className="wf-c-lane"
            />
            <text
              x={LANE_PAD_X}
              y={lane.top + 17}
              className="wf-t-dim"
              fontSize="11"
              fontWeight="600"
              fontFamily="var(--mono)"
            >
              {String(i + 1).padStart(2, "0")} / {lane.label}
            </text>
          </g>
        ))}

        {/* phase headers: a rule above each stage with a mask-chip caption */}
        {Array.from({ length: board.stages }, (_, s) => {
          const x = layout.stageX(s);
          const w = NODE_W;
          return (
            <g key={s}>
              <line x1={x} y1={PHASE_HEAD - 8} x2={x + w} y2={PHASE_HEAD - 8} className="wf-a-default" strokeWidth="1.1" />
              <rect x={x} y={PHASE_HEAD - 16} width={w} height={16} rx={4} className="wf-c-mask" />
              <text
                x={x + w / 2}
                y={PHASE_HEAD - 5}
                className="wf-t-muted"
                fontSize="9"
                fontWeight="600"
                textAnchor="middle"
                fontFamily="var(--mono)"
              >
                stage {s}
              </text>
            </g>
          );
        })}

        {/* edges under the nodes */}
        {board.edges.map((e) => (
          <Edge key={`${e.from}>${e.to}`} e={e} layout={layout} main={mainEdges.has(`${e.from}>${e.to}`)} />
        ))}

        {/* nodes: mask rect + semantic frame, sigil, centered label/sublabel */}
        {board.nodes.map((n) => (
          <Node key={n.id} n={n} layout={layout} onOpen={onOpen} />
        ))}
      </svg>
    </div>
  );
}

const MARKERS = {
  default: "wf-m-default",
  emph: "wf-m-emph",
  dashed: "wf-m-dashed",
  broken: "wf-m-broken",
} as const;

type Pos = { x: number; y: number };

function layoutBoard(board: FlowBoardDto): {
  width: number;
  height: number;
  stageX: (s: number) => number;
  lanes: Array<{ key: string; label: string; top: number; height: number }>;
  node: Map<string, Pos>;
  /** Per-edge ports: staggered fractions of the node's side so concurrent
   *  edges never share an exit or entry point (archify spreads its ports). */
  port: Map<string, { fromY: number; toY: number }>;
} {
  const laneRows = new Map<string, number>();
  for (const n of board.nodes) {
    const key = n.lane ?? "";
    laneRows.set(key, Math.max(laneRows.get(key) ?? 0, n.row + 1));
  }
  const lanes: Array<{ key: string; label: string; top: number; height: number }> = [];
  let top = PHASE_HEAD + 8;
  for (const lane of board.lanes) {
    const key = lane.id ?? "";
    const rows = laneRows.get(key) ?? 0;
    if (rows === 0) continue;
    const height = LANE_HEAD + rows * (NODE_H + GAP_Y);
    lanes.push({ key, label: lane.label, top, height });
    top += height + LANE_GAP;
  }
  const laneTop = new Map(lanes.map((l) => [l.key, l.top]));
  const left = LANE_PAD_X;
  const stageX = (s: number) => left + s * (NODE_W + GAP_X);
  const node = new Map<string, Pos>();
  for (const n of board.nodes) {
    const ltop = laneTop.get(n.lane ?? "") ?? PHASE_HEAD + 8;
    node.set(n.id, {
      x: stageX(n.stage),
      y: ltop + LANE_HEAD + n.row * (NODE_H + GAP_Y),
    });
  }
  // Ports: the k-th of n edges on a side sits at (k+1)/(n+1) of its height,
  // so a fan-out (or fan-in) spreads across the node instead of stacking.
  const outCount = new Map<string, number>();
  const inCount = new Map<string, number>();
  for (const e of board.edges) {
    outCount.set(e.from, (outCount.get(e.from) ?? 0) + 1);
    inCount.set(e.to, (inCount.get(e.to) ?? 0) + 1);
  }
  const outSeen = new Map<string, number>();
  const inSeen = new Map<string, number>();
  const port = new Map<string, { fromY: number; toY: number }>();
  for (const e of board.edges) {
    const kOut = outSeen.get(e.from) ?? 0;
    outSeen.set(e.from, kOut + 1);
    const kIn = inSeen.get(e.to) ?? 0;
    inSeen.set(e.to, kIn + 1);
    const nOut = outCount.get(e.from) ?? 1;
    const nIn = inCount.get(e.to) ?? 1;
    port.set(`${e.from}>${e.to}`, {
      fromY: (kOut + 1) / (nOut + 1),
      toY: (kIn + 1) / (nIn + 1),
    });
  }
  return {
    width: left * 2 + board.stages * NODE_W + Math.max(0, board.stages - 1) * GAP_X,
    height: top,
    stageX,
    lanes,
    node,
    port,
  };
}

function Edge({
  e,
  layout,
  main,
}: {
  e: FlowEdgeDto;
  layout: ReturnType<typeof layoutBoard>;
  main: boolean;
}) {
  const a = layout.node.get(e.from);
  const b = layout.node.get(e.to);
  if (!a || !b) return null;
  const ports = layout.port.get(`${e.from}>${e.to}`);
  const ax = a.x + NODE_W;
  const ay = a.y + NODE_H * (ports?.fromY ?? 0.5);
  const bx = b.x;
  const by = b.y + NODE_H * (ports?.toY ?? 0.5);
  const mid = (ax + bx) / 2;
  const d =
    Math.abs(ay - by) < 1
      ? `M ${ax} ${ay} L ${bx} ${by}`
      : `M ${ax} ${ay} L ${mid} ${ay} L ${mid} ${by} L ${bx} ${by}`;
  // Variant grammar: the main path is emphasized; satisfied mutes to a
  // dashed neutral; broken is dashed rose; anything else is the default.
  const [cls, marker] = main
    ? ["wf-a-emph", "emph"]
    : e.disposition === "broken"
      ? ["wf-a-broken", "broken"]
      : e.disposition === "satisfied"
        ? ["wf-a-dashed", "dashed"]
        : ["wf-a-default", "default"];
  return (
    <path
      d={d}
      className={cls}
      strokeWidth={main ? 1.8 : 1.4}
      markerEnd={`url(#wf-arrow-${marker})`}
    />
  );
}

function Node({
  n,
  layout,
  onOpen,
}: {
  n: FlowNodeDto;
  layout: ReturnType<typeof layoutBoard>;
  onOpen: (id: string) => void;
}) {
  const pos = layout.node.get(n.id);
  if (!pos) return null;
  const sem = semanticOf(n);
  const handle = n.number !== null ? `#${n.number}` : n.short_ref;
  // Two lines of centered label, clipped the way archify clips: hard caps.
  const t1 = n.title.length > 24 ? `${n.title.slice(0, 23)}…` : n.title;
  const claim = n.claim
    ? `@${n.claim.claimed_by}${n.claim.stale ? " · stale" : ""}`
    : n.status_label;
  const t2 = `${handle} · ${claim}${n.outside_blockers > 0 ? ` · +${n.outside_blockers} outside` : ""}`;
  return (
    <g
      transform={`translate(${pos.x},${pos.y})`}
      className="fnode"
      onClick={() => onOpen(n.short_ref)}
      role="button"
      tabIndex={0}
      onKeyDown={(ev) => {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          onOpen(n.short_ref);
        }
      }}
      aria-label={`${handle} ${n.title}`}
    >
      <title>{`${handle} · ${n.title} · ${n.status_label}${n.claim ? ` · claim ${n.claim.claimed_by}` : ""}`}</title>
      {/* the mask first: opaque, so edges underneath are hidden */}
      <rect width={NODE_W} height={NODE_H} rx={R} className="wf-c-mask" />
      <rect
        width={NODE_W}
        height={NODE_H}
        rx={R}
        className={cn("wf-node-frame", `wf-c-${sem}`)}
        strokeWidth="1.5"
      />
      {/* sigil: a tiny status glyph in the frame's stroke color */}
      <Sigil sem={sem} x={8} y={8} />
      <text
        x={NODE_W / 2}
        y={22}
        className="wf-t-primary"
        fontSize="11"
        fontWeight="600"
        textAnchor="middle"
        fontFamily="var(--mono)"
      >
        {t1}
      </text>
      <text
        x={NODE_W / 2}
        y={38}
        className={cn(sem === "done" ? "wf-t-dim" : "wf-t-muted")}
        fontSize="8.5"
        textAnchor="middle"
        fontFamily="var(--mono)"
      >
        {t2}
      </text>
    </g>
  );
}

/** The per-semantic mini glyph, archify's semantic sigils in miniature:
 *  ready = a check-forward tick, doing = two moving bars, blocked = an
 *  octagon stop hint, done = a settled square. */
function Sigil({ sem, x, y }: { sem: Semantic; x: number; y: number }) {
  const stroke =
    sem === "ready"
      ? "var(--wf-ready-stroke)"
      : sem === "doing"
        ? "var(--wf-doing-stroke)"
        : sem === "blocked"
          ? "var(--wf-blocked-stroke)"
          : "var(--wf-done-stroke)";
  return (
    <g transform={`translate(${x},${y})`} stroke={stroke} fill="none" strokeWidth="1.4" aria-hidden>
      {sem === "ready" && <path d="M1 4 L3.4 6.4 L7.5 1.6" />}
      {sem === "doing" && (
        <>
          <path d="M1.2 1.2 V6.8" />
          <path d="M4.2 1.2 V6.8" />
          <path d="M7.2 1.2 V6.8" />
        </>
      )}
      {sem === "blocked" && (
        <>
          <path d="M2.4 0.8 H5.6 L7.2 2.4 V5.6 L5.6 7.2 H2.4 L0.8 5.6 V2.4 Z" />
          <path d="M2.6 2.6 L5.4 5.4 M5.4 2.6 L2.6 5.4" />
        </>
      )}
      {sem === "done" && <rect x="1.2" y="1.2" width="5.6" height="5.6" rx="1" />}
    </g>
  );
}

// keep FlowEdgeDto referenced for the edge typing even when tree-shaken
export type { FlowEdgeDto as __FlowEdgeDto };
