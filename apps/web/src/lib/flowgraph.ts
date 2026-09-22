// The Flow diagram's pure layer: the graph the board describes, the grid it
// draws on, and the questions the reading tools ask of it — what does this
// node depend on, what waits on it, what route connects these two, which
// node am I searching for.
//
// It is kept out of the view on purpose. Every one of these answers is a
// derivation over `nodes` + `edges` and nothing here touches React, the DOM
// or the network, so the behaviour that makes the diagram readable can be
// pinned by a test instead of by looking at pixels.

import type { FlowBoardDto, FlowEdgeDto, FlowNodeDto } from "./types";

/** The board as an adjacency list, indexed by issue id. */
export interface FlowGraph {
  node: Map<string, FlowNodeDto>;
  /** id -> the issues it blocks (arrows leaving it). */
  out: Map<string, string[]>;
  /** id -> the issues blocking it (arrows arriving). */
  in: Map<string, string[]>;
}

export type Semantic = "ready" | "doing" | "blocked" | "done";

/** The four node colours, derived from status and readiness — never stored. */
export function semanticOf(n: FlowNodeDto): Semantic {
  if (n.category === "done") return "done";
  if (n.readiness === "ready") return "ready";
  if (n.readiness === "blocked") return "blocked";
  return "doing";
}

export function buildGraph(board: FlowBoardDto): FlowGraph {
  const node = new Map(board.nodes.map((n) => [n.id, n]));
  const out = new Map<string, string[]>();
  const incoming = new Map<string, string[]>();
  const push = (map: Map<string, string[]>, key: string, value: string) => {
    const list = map.get(key);
    if (list) list.push(value);
    else map.set(key, [value]);
  };
  for (const e of board.edges) {
    if (!node.has(e.from) || !node.has(e.to)) continue;
    push(out, e.from, e.to);
    push(incoming, e.to, e.from);
  }
  return { node, out, in: incoming };
}

/** Everything reachable from `id`, exclusive of `id` itself. `up` walks the
 *  blockers (what has to happen first), `down` walks the dependents. */
export function reach(graph: FlowGraph, id: string, dir: "up" | "down"): Set<string> {
  const edges = dir === "up" ? graph.in : graph.out;
  const seen = new Set<string>();
  const queue = [...(edges.get(id) ?? [])];
  while (queue.length > 0) {
    const next = queue.shift()!;
    if (seen.has(next)) continue;
    seen.add(next);
    for (const step of edges.get(next) ?? []) {
      if (!seen.has(step)) queue.push(step);
    }
  }
  seen.delete(id);
  return seen;
}

/** The shortest chain of dependencies joining two nodes, in the direction
 *  the arrows actually run. Tries `a -> b` first, then `b -> a`, so the
 *  reader can pick the two ends in either order. Null when nothing connects
 *  them. */
export function routeBetween(graph: FlowGraph, a: string, b: string): string[] | null {
  return walk(graph, a, b) ?? walk(graph, b, a);
}

function walk(graph: FlowGraph, from: string, to: string): string[] | null {
  if (!graph.node.has(from) || !graph.node.has(to)) return null;
  if (from === to) return [from];
  const parent = new Map<string, string>();
  const seen = new Set([from]);
  const queue = [from];
  while (queue.length > 0) {
    const at = queue.shift()!;
    for (const next of graph.out.get(at) ?? []) {
      if (seen.has(next)) continue;
      seen.add(next);
      parent.set(next, at);
      if (next === to) {
        const path = [to];
        let cursor = to;
        while (parent.has(cursor)) {
          cursor = parent.get(cursor)!;
          path.push(cursor);
        }
        return path.reverse();
      }
      queue.push(next);
    }
  }
  return null;
}

/** The pairs of an ordered chain, as the `from>to` keys the edges use. */
export function chainEdges(path: readonly string[]): Set<string> {
  const set = new Set<string>();
  for (let i = 0; i + 1 < path.length; i += 1) set.add(`${path[i]}>${path[i + 1]}`);
  return set;
}

/** Nodes matching a typed query, best first. Matches `#12`, a short_ref, or
 *  any part of the title; a title that starts with the query beats one that
 *  merely contains it, so typing the first word lands on the right node. */
export function searchNodes(nodes: readonly FlowNodeDto[], query: string): FlowNodeDto[] {
  const q = query.trim().toLowerCase();
  if (q === "") return [];
  const bare = q.startsWith("#") ? q.slice(1) : q;
  const scored: Array<{ rank: number; node: FlowNodeDto }> = [];
  for (const node of nodes) {
    const title = node.title.toLowerCase();
    const number = node.number === null ? "" : String(node.number);
    let rank = -1;
    if (number !== "" && number === bare) rank = 0;
    else if (node.short_ref.toLowerCase() === bare) rank = 1;
    else if (title.startsWith(q)) rank = 2;
    else if (number.startsWith(bare) && bare !== "") rank = 3;
    else if (title.includes(q)) rank = 4;
    else if (node.short_ref.toLowerCase().includes(bare)) rank = 5;
    if (rank >= 0) scored.push({ rank, node });
  }
  scored.sort((x, y) => x.rank - y.rank || x.node.title.localeCompare(y.node.title));
  return scored.map((s) => s.node);
}

/** Greedy word wrap for a node label. Words longer than the line are cut
 *  rather than allowed to overflow the box, and the last line ends in an
 *  ellipsis when there is more title than room. */
export function wrapLabel(title: string, perLine: number, lines: number): string[] {
  const words = title.split(/\s+/).filter((w) => w.length > 0);
  const out: string[] = [];
  let taken = 0;
  let current = "";
  for (const word of words) {
    const candidate = current === "" ? word : `${current} ${word}`;
    if (candidate.length <= perLine) {
      current = candidate;
      taken += 1;
      continue;
    }
    if (current !== "") out.push(current);
    if (out.length === lines) {
      current = "";
      break;
    }
    current = word.length > perLine ? word.slice(0, perLine) : word;
    taken += 1;
  }
  if (current !== "" && out.length < lines) out.push(current);
  if (out.length === 0) return [""];
  const dropped = taken < words.length;
  const cut = words.some((w, i) => i < taken && w.length > perLine);
  if (dropped || cut) {
    const last = out[out.length - 1] ?? "";
    out[out.length - 1] = `${last.slice(0, Math.max(0, perLine - 1)).trimEnd()}…`;
  }
  return out;
}

// -- painting ---------------------------------------------------------------

/** Which dimension the node colours stand for. Each one is already in the
 *  data, so the lens costs the schema nothing. */
export type Paint = "state" | "lane" | "type" | "priority";

/** The key a node paints by, under a given dimension. Never null: a missing
 *  value gets its own visible bucket rather than disappearing. */
export function paintKeyOf(n: FlowNodeDto, paint: Paint): string {
  switch (paint) {
    case "state":
      return semanticOf(n);
    case "lane":
      return n.lane ?? "unlaned";
    case "type":
      return n.kind;
    case "priority":
      return n.priority ?? "none";
  }
}

/** The legend for a dimension: every key present on this board with how many
 *  nodes carry it, in a stable order. `state` keeps its designed order; the
 *  rest sort by count so the busiest bucket reads first. */
export function paintLegend(
  nodes: readonly FlowNodeDto[],
  paint: Paint,
): Array<{ key: string; count: number }> {
  const counts = new Map<string, number>();
  for (const n of nodes) {
    const key = paintKeyOf(n, paint);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  const rows = [...counts.entries()].map(([key, count]) => ({ key, count }));
  if (paint === "state") {
    const order = ["ready", "doing", "blocked", "done"];
    rows.sort((a, b) => order.indexOf(a.key) - order.indexOf(b.key));
    return rows;
  }
  rows.sort((a, b) => b.count - a.count || a.key.localeCompare(b.key));
  return rows;
}

/** How many categorical swatches the stylesheet defines. */
export const PAINT_SLOTS = 8;

/** A stable swatch for a key: the legend's own order, so the colour a reader
 *  learns does not move when a node changes status. Beyond the palette the
 *  slots repeat rather than running out. */
export function paintSlot(
  legend: ReadonlyArray<{ key: string }>,
  key: string,
): number {
  const at = legend.findIndex((row) => row.key === key);
  return at < 0 ? 0 : at % PAINT_SLOTS;
}

/** How much of a node's text is worth drawing at this zoom. Below "read" the
 *  sublabels are noise; at "full" there is room for the extras. */
export type Detail = "map" | "read" | "full";

export function detailAt(scale: number): Detail {
  if (scale < 0.7) return "map";
  if (scale < 1.4) return "read";
  return "full";
}

// -- the story --------------------------------------------------------------

/** One beat of the guided reading: a caption and the nodes it is about. */
export interface Chapter {
  key: string;
  label: string;
  note: string;
  focus: string[];
}

/** The chapters a board tells itself. With a fence they are its phases, in
 *  order, which is the story the team already wrote down. Without one they
 *  are the steps of the critical path — the chain that decides when the flow
 *  lands, which is the story most readers came for. Either way nobody
 *  authors a chapter list: archify's guided views are hand-written, and a
 *  hand-written story is one more thing to keep true. */
export function storyChapters(board: FlowBoardDto): Chapter[] {
  if (board.phases.length > 0) {
    return board.phases.map((phase, i) => {
      const focus = board.nodes.filter((n) => n.stage === i).map((n) => n.id);
      return {
        key: phase.id,
        label: phase.label,
        note:
          focus.length === 0
            ? "Nothing sits in this phase yet."
            : `${focus.length} ${focus.length === 1 ? "issue" : "issues"} in this phase.`,
        focus,
      };
    });
  }
  const byId = new Map(board.nodes.map((n) => [n.id, n]));
  return board.main_path.flatMap((id, i) => {
    const node = byId.get(id);
    if (node === undefined) return [];
    return [
      {
        key: id,
        label: `Step ${i + 1}`,
        note: `${node.title} — ${node.status_label}`,
        focus: [id],
      },
    ];
  });
}

// -- the grid ---------------------------------------------------------------

export const METRICS = {
  nodeW: 208,
  nodeH: 70,
  gapX: 88,
  gapY: 26,
  padX: 34,
  padY: 16,
  laneHead: 28,
  laneGap: 16,
  stageHead: 40,
  radius: 7,
} as const;

export interface FlowLayout {
  width: number;
  height: number;
  stageX: (stage: number) => number;
  lanes: Array<{ key: string; label: string; top: number; height: number }>;
  node: Map<string, { x: number; y: number }>;
  /** Per-edge exit and entry heights, as fractions of the node's side, so a
   *  fan-out spreads across the box instead of stacking on one point. */
  port: Map<string, { fromY: number; toY: number }>;
}

export function layoutBoard(board: FlowBoardDto): FlowLayout {
  const m = METRICS;
  const laneRows = new Map<string, number>();
  for (const n of board.nodes) {
    const key = n.lane ?? "";
    laneRows.set(key, Math.max(laneRows.get(key) ?? 0, n.row + 1));
  }
  const lanes: FlowLayout["lanes"] = [];
  let top = m.stageHead + m.padY;
  for (const lane of board.lanes) {
    const key = lane.id ?? "";
    const rows = laneRows.get(key) ?? 0;
    if (rows === 0) continue;
    const height = m.laneHead + rows * (m.nodeH + m.gapY);
    lanes.push({ key, label: lane.label, top, height });
    top += height + m.laneGap;
  }
  const laneTop = new Map(lanes.map((l) => [l.key, l.top]));
  const stageX = (stage: number) => m.padX + stage * (m.nodeW + m.gapX);
  const node = new Map<string, { x: number; y: number }>();
  for (const n of board.nodes) {
    const lane = laneTop.get(n.lane ?? "") ?? m.stageHead + m.padY;
    node.set(n.id, { x: stageX(n.stage), y: lane + m.laneHead + n.row * (m.nodeH + m.gapY) });
  }
  return {
    width: m.padX * 2 + board.stages * m.nodeW + Math.max(0, board.stages - 1) * m.gapX,
    height: top + m.padY,
    stageX,
    lanes,
    node,
    port: ports(board.edges),
  };
}

function ports(edges: readonly FlowEdgeDto[]): FlowLayout["port"] {
  const outCount = new Map<string, number>();
  const inCount = new Map<string, number>();
  for (const e of edges) {
    outCount.set(e.from, (outCount.get(e.from) ?? 0) + 1);
    inCount.set(e.to, (inCount.get(e.to) ?? 0) + 1);
  }
  const outSeen = new Map<string, number>();
  const inSeen = new Map<string, number>();
  const port = new Map<string, { fromY: number; toY: number }>();
  for (const e of edges) {
    const kOut = outSeen.get(e.from) ?? 0;
    outSeen.set(e.from, kOut + 1);
    const kIn = inSeen.get(e.to) ?? 0;
    inSeen.set(e.to, kIn + 1);
    port.set(`${e.from}>${e.to}`, {
      fromY: (kOut + 1) / ((outCount.get(e.from) ?? 1) + 1),
      toY: (kIn + 1) / ((inCount.get(e.to) ?? 1) + 1),
    });
  }
  return port;
}

/** What a column is called. With a fence the columns are the authored
 *  phases, plus a trailing band for members nobody has placed; without one
 *  they are the computed ranks they always were. */
export function columnLabel(board: FlowBoardDto, index: number): string {
  if (board.phases.length === 0) return `stage ${index}`;
  return board.phases[index]?.label ?? "unphased";
}

/** The frames a fence's groups draw: one rectangle per group, covering the
 *  columns it spans inside the single lane it belongs to. A group whose lane
 *  or phases are not on this board draws nothing rather than a box around
 *  the wrong nodes. */
export function groupFrames(
  board: FlowBoardDto,
  layout: FlowLayout,
): Array<{ id: string; label: string; x: number; y: number; w: number; h: number }> {
  const out: Array<{ id: string; label: string; x: number; y: number; w: number; h: number }> = [];
  for (const group of board.groups) {
    const lane = layout.lanes.find((l) => l.key === (group.lane ?? ""));
    if (lane === undefined) continue;
    const columns = group.phases
      .map((id) => board.phases.findIndex((p) => p.id === id))
      .filter((i) => i >= 0);
    if (columns.length === 0) continue;
    const first = Math.min(...columns);
    const last = Math.max(...columns);
    out.push({
      id: group.id,
      label: group.label,
      x: layout.stageX(first) - 14,
      y: lane.top + METRICS.laneHead - 10,
      w: layout.stageX(last) + METRICS.nodeW + 14 - (layout.stageX(first) - 14),
      h: lane.height - METRICS.laneHead + 4,
    });
  }
  return out;
}

/** An orthogonal two-bend connector between two node sides. A straight
 *  segment when the two ports already line up. */
export function edgePath(
  a: { x: number; y: number },
  b: { x: number; y: number },
  port: { fromY: number; toY: number } | undefined,
): string {
  const m = METRICS;
  const ax = a.x + m.nodeW;
  const ay = a.y + m.nodeH * (port?.fromY ?? 0.5);
  const bx = b.x;
  const by = b.y + m.nodeH * (port?.toY ?? 0.5);
  if (Math.abs(ay - by) < 1) return `M ${ax} ${ay} L ${bx} ${by}`;
  // A backward edge (a cycle the layering could not break) needs the bend
  // outside both boxes, or it would draw straight through them.
  const mid = bx > ax ? (ax + bx) / 2 : ax + m.gapX / 2;
  return `M ${ax} ${ay} L ${mid} ${ay} L ${mid} ${by} L ${bx} ${by}`;
}
