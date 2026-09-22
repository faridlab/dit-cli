// The Flow screen (ADR 0019): an orchestration rendered as a diagram —
// issues as nodes on a computed stage grid, `blocked_by` as orthogonal
// edges, lanes as horizontal bands, the critical path emphasized. The visual
// language follows the archify workflow-diagram tradition (github.com/
// tt-a1i/archify): grid placement, orthogonal routes, semantic stroke
// variants, muted satisfied edges — a diagram, not a board with sections.
//
// Everything on screen is derived server-side (`GET /api/flow/{name}`) and
// never written back; the screen is read-only, live on any process's write.

import { useMemo, useState } from "react";
import { CircleDot, Lock, Waypoints } from "lucide-react";
import { useFlows, useFlowBoard } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import type { FlowBoardDto, FlowEdgeDto, FlowNodeDto } from "../lib/types";

/** The union pseudo-flow the API understands. */
const ALL = "__all__";

const NODE_W = 210;
const NODE_H = 58;
const GAP_X = 64;
const GAP_Y = 20;
const LANE_HEAD = 30;
const PAD = 16;
const R = 8;

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
        <div className="fhead">
          <Waypoints className="i" aria-hidden />
          <b>Flows</b>
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

  return (
    <div className="wflow">
      <div className="fhead">
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
        <span className="fmeta">
          {data
            ? `${data.nodes.length} nodes · ${data.stages} stages · critical path ${Math.max(0, data.main_path.length - 1)} hops`
            : ""}
        </span>
      </div>
      {data ? <Diagram board={data} onOpen={onOpen} /> : null}
      <footer className="wfoot">
        <span className="legend">
          <i className="sw edge unsat" /> in flight <i className="sw edge sat" /> through the gate{" "}
          <i className="sw edge broken" /> cancelled/gone <i className="sw edge main" /> critical
          path · stages are derived from blocked_by — the deeper, the later
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
      >
        <defs>
          <marker id="fa" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto">
            <path d="M0,0 L7,3.5 L0,7 z" className="arr" />
          </marker>
        </defs>
        {/* lane bands */}
        {layout.lanes.map((lane) => (
          <g key={lane.key}>
            <rect
              x={0}
              y={lane.top}
              width={layout.width}
              height={lane.height}
              rx={10}
              className="laneBand"
            />
            <text x={PAD + 2} y={lane.top + 19} className="laneLbl">
              {lane.label}
            </text>
          </g>
        ))}
        {/* stage ticks */}
        {Array.from({ length: board.stages }, (_, s) => (
          <text key={s} x={PAD + s * (NODE_W + GAP_X)} y={10} className="stageLbl">
            stage {s}
          </text>
        ))}
        {/* edges under nodes */}
        {board.edges.map((e) => (
          <Edge key={`${e.from}>${e.to}`} e={e} layout={layout} main={mainEdges.has(`${e.from}>${e.to}`)} />
        ))}
        {/* nodes */}
        {board.nodes.map((n) => (
          <Node key={n.id} n={n} layout={layout} onOpen={onOpen} />
        ))}
      </svg>
    </div>
  );
}

type Pos = { x: number; y: number };

function layoutBoard(board: FlowBoardDto): {
  width: number;
  height: number;
  lanes: Array<{ key: string; label: string; top: number; height: number }>;
  node: Map<string, Pos>;
} {
  const laneRows = new Map<string, number>();
  for (const n of board.nodes) {
    const key = n.lane ?? "";
    laneRows.set(key, Math.max(laneRows.get(key) ?? 0, n.row + 1));
  }
  const lanes: Array<{ key: string; label: string; top: number; height: number }> = [];
  let top = 24;
  for (const lane of board.lanes) {
    const key = lane.id ?? "";
    const rows = laneRows.get(key) ?? 0;
    if (rows === 0) continue;
    const height = LANE_HEAD + rows * (NODE_H + GAP_Y);
    lanes.push({ key, label: lane.label, top, height });
    top += height + 14;
  }
  const laneTop = new Map(lanes.map((l) => [l.key, l.top]));
  const node = new Map<string, Pos>();
  for (const n of board.nodes) {
    const ltop = laneTop.get(n.lane ?? "") ?? 24;
    node.set(n.id, {
      x: PAD + n.stage * (NODE_W + GAP_X),
      y: ltop + LANE_HEAD + n.row * (NODE_H + GAP_Y),
    });
  }
  return {
    width: PAD * 2 + board.stages * NODE_W + Math.max(0, board.stages - 1) * GAP_X,
    height: top,
    lanes,
    node,
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
  const ax = a.x + NODE_W;
  const ay = a.y + NODE_H / 2;
  const bx = b.x;
  const by = b.y + NODE_H / 2;
  // Same row: straight; otherwise one elbow at the midpoint channel.
  const mid = (ax + bx) / 2;
  const d =
    Math.abs(ay - by) < 1
      ? `M${ax},${ay} L${bx},${by}`
      : `M${ax},${ay} L${mid},${ay} L${mid},${by} L${bx},${by}`;
  return (
    <path
      d={d}
      className={cn("edge", e.disposition, main && "main")}
      markerEnd="url(#fa)"
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
  const handle = n.number !== null ? `#${n.number}` : n.short_ref;
  const title = n.title.length > 30 ? `${n.title.slice(0, 29)}…` : n.title;
  const sub = `${handle} · ${n.status}${n.claim ? ` · ${n.claim.claimed_by}${n.claim.stale ? " (stale)" : ""}` : ""}${
    n.outside_blockers > 0 ? ` · ${n.outside_blockers} outside` : ""
  }`;
  const done = n.category === "done";
  return (
    <g
      transform={`translate(${pos.x},${pos.y})`}
      className={cn("fnode", done && "done")}
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
      <rect width={NODE_W} height={NODE_H} rx={R} className="fcard" />
      <rect width={4} height={NODE_H} rx={2} className={cn("fbar", n.category ?? "doing")} />
      <text x={14} y={23} className="ftitle">
        {title}
      </text>
      <text x={14} y={41} className="fsub">
        {sub}
      </text>
      {n.readiness === "ready" ? (
        <CircleDot className="i fready" x={NODE_W - 22} y={12} width={14} height={14} aria-hidden />
      ) : n.readiness === "blocked" ? (
        <Lock className="i fblocked" x={NODE_W - 22} y={12} width={14} height={14} aria-hidden />
      ) : null}
    </g>
  );
}
