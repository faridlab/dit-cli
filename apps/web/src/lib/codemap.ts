// The code map screen's pure half: which units of a folder are drawn, how
// big each one is, where the force layout starts them, and how a path is
// shortened for a label. No React, no DOM — the layout runs d3-force
// synchronously, so the same folder always lands in the same picture.

import {
  forceCenter,
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import type { CodeUnitDto, CodeUnitEdgeDto } from "./types";

/** More units than this and the graph stops being readable. */
export const UNIT_CAP = 80;

/** FNV-1a, 32 bit: a stable number for a path, so layout seeds and anything
 *  else derived from a name never depend on Math.random. */
export function hashString(text: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i += 1) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** A starting point for `path` inside a `width` × `height` box, from its hash:
 *  the low half picks x, the high half picks y. */
export function seedPosition(path: string, width: number, height: number): { x: number; y: number } {
  const h = hashString(path);
  const fx = (h & 0xffff) / 0xffff;
  const fy = (h >>> 16) / 0xffff;
  return { x: (fx - 0.5) * width * 0.8, y: (fy - 0.5) * height * 0.8 };
}

/** How much a unit matters in this folder: imports across the folder's edge
 *  plus the imports on every drawn edge touching it. */
export function unitWeights(units: CodeUnitDto[], edges: CodeUnitEdgeDto[]): Map<string, number> {
  const weight = new Map<string, number>();
  for (const unit of units) weight.set(unit.path, unit.inbound + unit.outbound);
  for (const edge of edges) {
    if (weight.has(edge.from)) weight.set(edge.from, (weight.get(edge.from) ?? 0) + edge.imports);
    if (weight.has(edge.to)) weight.set(edge.to, (weight.get(edge.to) ?? 0) + edge.imports);
  }
  return weight;
}

/** The units worth drawing: all of them up to `cap`, otherwise the `cap`
 *  heaviest (ties broken by path so the cut is stable), plus the edges whose
 *  both ends survived and how many were left out. */
export function capUnits(
  units: CodeUnitDto[],
  edges: CodeUnitEdgeDto[],
  cap: number = UNIT_CAP,
): { units: CodeUnitDto[]; edges: CodeUnitEdgeDto[]; hidden: number } {
  if (units.length <= cap) {
    const known = new Set(units.map((u) => u.path));
    return { units, edges: edges.filter((e) => known.has(e.from) && known.has(e.to)), hidden: 0 };
  }
  const weight = unitWeights(units, edges);
  const kept = [...units]
    .sort((a, b) => (weight.get(b.path) ?? 0) - (weight.get(a.path) ?? 0) || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0))
    .slice(0, cap);
  const known = new Set(kept.map((u) => u.path));
  // Keep the folder's own order for what is drawn; only the cut is by weight.
  const ordered = units.filter((u) => known.has(u.path));
  return {
    units: ordered,
    edges: edges.filter((e) => known.has(e.from) && known.has(e.to)),
    hidden: units.length - cap,
  };
}

/** A unit is drawn muted and marked "gen" only when every file in it is
 *  generated — a folder with one hand-written file is still a place to edit. */
export function fullyGenerated(unit: Pick<CodeUnitDto, "files" | "generated">): boolean {
  return unit.files > 0 && unit.generated >= unit.files;
}

/** Node size from file count: square-root scale, clamped, so a folder of
 *  900 files is bigger than one of 9 without swallowing the canvas. */
/** Widest a file pill grows to fit its name before the name is cut. */
const FILE_PILL_MAX = 180;

export function unitSize(
  unit: Pick<CodeUnitDto, "files" | "folder"> & { path?: string },
): { w: number; h: number } {
  const t = Math.min(1, Math.sqrt(Math.max(1, unit.files)) / 20);
  if (unit.folder) return { w: Math.round(96 + t * 84), h: Math.round(36 + t * 24) };
  // A file is one name: give the pill room for it, so a short name such as
  // `vite-env.d.ts` is never cut.
  const base = Math.round(84 + t * 40);
  const fit = unit.path ? lastSegment(unit.path).length * 7 + 20 : 0;
  return { w: Math.min(FILE_PILL_MAX, Math.max(base, fit)), h: 24 };
}

/** Edge stroke width from import count: logarithmic, 1 to 6 px. */
export function edgeWidth(imports: number): number {
  return Math.min(6, 1 + Math.log2(Math.max(1, imports)));
}

/** The last segment of a path — the label a node carries. */
export function lastSegment(path: string): string {
  const parts = path.split("/").filter((p) => p.length > 0);
  return parts[parts.length - 1] ?? path;
}

/** Breadcrumb for a folder: the root first (folder `""`), then one entry per
 *  segment, each carrying the folder it leads back to. */
export function folderCrumbs(root: string, folder: string): Array<{ label: string; folder: string }> {
  const parts = folder.split("/").filter((p) => p.length > 0);
  return [
    { label: root, folder: "" },
    ...parts.map((label, i) => ({ label, folder: parts.slice(0, i + 1).join("/") })),
  ];
}

/** The folder holding `path` (`""` at the top). */
export function parentFolder(path: string): string {
  const parts = path.split("/").filter((p) => p.length > 0);
  return parts.slice(0, -1).join("/");
}

export interface PlacedUnit {
  unit: CodeUnitDto;
  x: number;
  y: number;
  w: number;
  h: number;
}

interface SimNode extends SimulationNodeDatum {
  id: string;
  r: number;
}

/** Where every unit sits, centred on (0, 0). Deterministic: seeded from the
 *  paths and run a fixed number of ticks before anything is drawn, so a
 *  reload — or a screenshot — shows the same picture. */
export function layoutUnits(
  units: CodeUnitDto[],
  edges: CodeUnitEdgeDto[],
  opts: { width?: number; height?: number; ticks?: number } = {},
): PlacedUnit[] {
  const width = opts.width ?? 900;
  const height = opts.height ?? 600;
  const ticks = opts.ticks ?? 300;
  const sizes = new Map(units.map((u) => [u.path, unitSize(u)]));
  const nodes: SimNode[] = units.map((u) => {
    const s = sizes.get(u.path) ?? { w: 80, h: 24 };
    const seed = seedPosition(u.path, width, height);
    return { id: u.path, r: Math.hypot(s.w, s.h) / 2 + 8, x: seed.x, y: seed.y };
  });
  const links: Array<SimulationLinkDatum<SimNode> & { imports: number }> = edges.map((e) => ({
    source: e.from,
    target: e.to,
    imports: e.imports,
  }));
  if (nodes.length > 1) {
    const sim = forceSimulation(nodes)
      .force(
        "link",
        forceLink<SimNode, SimulationLinkDatum<SimNode> & { imports: number }>(links)
          .id((d) => d.id)
          .distance((l) => {
            const s = l.source as SimNode;
            const t = l.target as SimNode;
            return s.r + t.r + 40;
          })
          .strength(0.3),
      )
      .force("charge", forceManyBody<SimNode>().strength(-320))
      .force("collide", forceCollide<SimNode>((d) => d.r).iterations(2))
      .force("x", forceX<SimNode>(0).strength(0.06))
      .force("y", forceY<SimNode>(0).strength(0.09))
      .force("center", forceCenter(0, 0))
      .stop();
    sim.tick(ticks);
  }
  return units.map((unit, i) => {
    const n = nodes[i];
    const s = sizes.get(unit.path) ?? { w: 80, h: 24 };
    return { unit, x: n?.x ?? 0, y: n?.y ?? 0, w: s.w, h: s.h };
  });
}

/** The box the placed units need, with a margin — the SVG's viewBox. */
export function boundsOf(placed: PlacedUnit[], margin = 32): { x: number; y: number; w: number; h: number } {
  if (placed.length === 0) return { x: -100, y: -60, w: 200, h: 120 };
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (const p of placed) {
    minX = Math.min(minX, p.x - p.w / 2);
    minY = Math.min(minY, p.y - p.h / 2);
    maxX = Math.max(maxX, p.x + p.w / 2);
    maxY = Math.max(maxY, p.y + p.h / 2);
  }
  return { x: minX - margin, y: minY - margin, w: maxX - minX + margin * 2, h: maxY - minY + margin * 2 };
}

/** Where a line from the centre of a box toward (tx, ty) leaves the box —
 *  so an arrowhead lands on the node's edge, not under it. */
export function boxExit(
  cx: number,
  cy: number,
  w: number,
  h: number,
  tx: number,
  ty: number,
): { x: number; y: number } {
  const dx = tx - cx;
  const dy = ty - cy;
  if (dx === 0 && dy === 0) return { x: cx, y: cy };
  const sx = dx === 0 ? Number.POSITIVE_INFINITY : w / 2 / Math.abs(dx);
  const sy = dy === 0 ? Number.POSITIVE_INFINITY : h / 2 / Math.abs(dy);
  const s = Math.min(sx, sy);
  return { x: cx + dx * s, y: cy + dy * s };
}

/** The viewBox to draw `box` in a `width` × `height` viewport: the box
 *  itself when it is bigger than the viewport can show at `maxScale`,
 *  otherwise grown around its centre — so a folder of four units is drawn at
 *  a readable size instead of being blown up to fill the screen. An unknown
 *  (zero) viewport leaves the box as it is. */
export function fitViewBox(
  box: { x: number; y: number; w: number; h: number },
  width: number,
  height: number,
  maxScale = 1.15,
): { x: number; y: number; w: number; h: number } {
  if (width <= 0 || height <= 0) return box;
  const w = Math.max(box.w, width / maxScale);
  const h = Math.max(box.h, height / maxScale);
  return { x: box.x + box.w / 2 - w / 2, y: box.y + box.h / 2 - h / 2, w, h };
}


// ---- shortening a name -------------------------------------------------------

/** Clip `text` to `max` characters in the middle, keeping its start and its
 *  end — the end of a file name (`Page.tsx`, `.test.tsx`) is what tells two
 *  files apart, and the start is what they are about. */
export function middleTruncate(text: string, max: number): string {
  if (text.length <= max) return text;
  if (max <= 1) return "…".slice(0, Math.max(0, max));
  const budget = max - 1;
  const dot = text.lastIndexOf(".");
  const ext = dot > 0 ? text.length - dot : 0;
  // Half the room for the end, more if the extension needs it, but always
  // leave at least one character of the start.
  const tail = Math.min(budget - 1, Math.max(Math.ceil(budget / 2), ext));
  const head = budget - tail;
  return `${text.slice(0, head)}…${text.slice(text.length - tail)}`;
}

/** A path in at most `max` characters: whole when it fits; else its first
 *  segment, an ellipsis and as many trailing segments as fit
 *  (`src/…/people/Page.tsx`); else `…/name`; else the name clipped in the
 *  middle. The one shortener for every path and name on the code screen. */
export function shortenPath(path: string, max: number): string {
  if (path.length <= max) return path;
  const parts = path.split("/").filter((p) => p.length > 0);
  const name = parts[parts.length - 1] ?? path;
  if (parts.length >= 3) {
    const first = parts[0];
    for (let k = parts.length - 2; k >= 1; k -= 1) {
      const candidate = `${first}/…/${parts.slice(-k).join("/")}`;
      if (candidate.length <= max) return candidate;
    }
  }
  if (parts.length >= 2 && name.length + 2 <= max) return `…/${name}`;
  return middleTruncate(name, max);
}

/** How many characters of a label fit in `px` at roughly `charPx` each. */
export function charsFor(px: number, charPx = 7): number {
  return Math.max(4, Math.floor(px / charPx));
}

// ---- which edges are drawn ---------------------------------------------------

/** Edges drawn before anyone hovers: every edge up to this many. */
export const EDGE_CAP = 30;

export function edgeKey(edge: Pick<CodeUnitEdgeDto, "from" | "to">): string {
  return `${edge.from}\u0000${edge.to}`;
}

function byPath(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** The heaviest `limit` edges (all of them when there are no more), by
 *  import count, ties by `from` then `to` — so the same folder always shows
 *  the same lines. Returned as a set of `edgeKey`s. */
export function topEdges(edges: CodeUnitEdgeDto[], limit: number = EDGE_CAP): Set<string> {
  if (edges.length <= limit) return new Set(edges.map(edgeKey));
  const sorted = [...edges].sort((a, b) => b.imports - a.imports || byPath(a.from, b.from) || byPath(a.to, b.to));
  return new Set(sorted.slice(0, limit).map(edgeKey));
}

// ---- the layered layout, for folders too big for a force layout --------------

/** Above this many units a folder is drawn in layers, not by force. */
export const LAYERED_OVER = 30;

export const LAYER = { nodeW: 180, colGap: 90, rowH: 56, folderH: 44, fileH: 26 } as const;

/** Rank every unit by import direction: importers left, what they import
 *  right. Cycles are broken by dropping the back edges a depth-first search
 *  finds when it visits units (and their targets) in path order; the rest is
 *  a DAG, ranked by longest path. Units touching no edge get rank -1. */
export function rankUnits(paths: string[], edges: CodeUnitEdgeDto[]): { rank: Map<string, number>; back: Set<string> } {
  const sorted = [...paths].sort(byPath);
  const known = new Set(sorted);
  const out = new Map<string, string[]>(sorted.map((p) => [p, []]));
  const touched = new Set<string>();
  for (const e of edges) {
    if (!known.has(e.from) || !known.has(e.to) || e.from === e.to) continue;
    const list = out.get(e.from);
    if (list && !list.includes(e.to)) list.push(e.to);
    touched.add(e.from);
    touched.add(e.to);
  }
  for (const list of out.values()) list.sort(byPath);

  const state = new Map<string, 1 | 2>();
  const back = new Set<string>();
  const post: string[] = [];
  // Iterative, so a deep import chain cannot overflow the stack.
  for (const start of sorted) {
    if (state.has(start)) continue;
    const stack: Array<{ v: string; i: number }> = [{ v: start, i: 0 }];
    state.set(start, 1);
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      if (!top) break;
      const next = out.get(top.v) ?? [];
      if (top.i < next.length) {
        const w = next[top.i] ?? "";
        top.i += 1;
        const s = state.get(w);
        if (s === 1) back.add(`${top.v}\u0000${w}`);
        else if (s === undefined) {
          state.set(w, 1);
          stack.push({ v: w, i: 0 });
        }
      } else {
        state.set(top.v, 2);
        post.push(top.v);
        stack.pop();
      }
    }
  }

  const rank = new Map<string, number>(sorted.map((p) => [p, touched.has(p) ? 0 : -1]));
  const forward = (v: string) => (out.get(v) ?? []).filter((w) => !back.has(`${v}\u0000${w}`));
  const topo = [...post].reverse();
  for (const v of topo) {
    const rv = rank.get(v) ?? 0;
    for (const w of forward(v)) {
      if ((rank.get(w) ?? 0) < rv + 1) rank.set(w, rv + 1);
    }
  }
  // Longest path leaves every importer in the first column however far away
  // what it imports sits; pull each one right to just before its nearest
  // import (sinks first, so those are final), which shortens the lines.
  for (const v of post) {
    const outs = forward(v);
    if (outs.length === 0) continue;
    const nearest = Math.min(...outs.map((w) => rank.get(w) ?? 0)) - 1;
    if (nearest > (rank.get(v) ?? 0)) rank.set(v, nearest);
  }
  return { rank, back };
}

/** Order units inside each rank by the barycentre of their neighbours in
 *  the ranks before (sweeping right) and after (sweeping left), a few
 *  passes, ties by current position then path. Pure and stable. */
export function orderRanks(layers: string[][], edges: CodeUnitEdgeDto[], sweeps = 3): string[][] {
  const order = layers.map((layer) => [...layer].sort(byPath));
  const rankOf = new Map<string, number>();
  order.forEach((layer, r) => {
    for (const p of layer) rankOf.set(p, r);
  });
  const neighbours = new Map<string, string[]>();
  for (const e of edges) {
    if (!rankOf.has(e.from) || !rankOf.has(e.to) || e.from === e.to) continue;
    neighbours.set(e.from, [...(neighbours.get(e.from) ?? []), e.to]);
    neighbours.set(e.to, [...(neighbours.get(e.to) ?? []), e.from]);
  }
  const position = new Map<string, number>();
  const index = () => {
    for (const layer of order) layer.forEach((p, i) => position.set(p, i));
  };
  index();
  const sweep = (r: number, before: boolean) => {
    const layer = order[r];
    if (!layer) return;
    const bary = new Map<string, number>();
    for (const p of layer) {
      const near = (neighbours.get(p) ?? []).filter((q) => {
        const rq = rankOf.get(q) ?? r;
        return before ? rq < r : rq > r;
      });
      bary.set(
        p,
        near.length === 0 ? (position.get(p) ?? 0) : near.reduce((sum, q) => sum + (position.get(q) ?? 0), 0) / near.length,
      );
    }
    layer.sort(
      (a, b) =>
        (bary.get(a) ?? 0) - (bary.get(b) ?? 0) || (position.get(a) ?? 0) - (position.get(b) ?? 0) || byPath(a, b),
    );
    layer.forEach((p, i) => position.set(p, i));
  };
  for (let s = 0; s < sweeps; s += 1) {
    for (let r = 1; r < order.length; r += 1) sweep(r, true);
    for (let r = order.length - 2; r >= 0; r -= 1) sweep(r, false);
  }
  return order;
}

/** A folder too big for a force layout, in layers: ranks left to right, one
 *  row slot per unit (so no two labels can meet), each rank centred on the
 *  tallest. Units touching no edge stand in trailing columns. Positions are
 *  box centres, starting at (0, 0) — the same input always lands the same. */
export function layeredLayout(units: CodeUnitDto[], edges: CodeUnitEdgeDto[]): PlacedUnit[] {
  const { rank } = rankUnits(
    units.map((u) => u.path),
    edges,
  );
  let maxRank = -1;
  for (const r of rank.values()) maxRank = Math.max(maxRank, r);
  const layers: string[][] = Array.from({ length: maxRank + 1 }, () => []);
  const loose: string[] = [];
  for (const u of units) {
    const r = rank.get(u.path) ?? -1;
    if (r < 0) loose.push(u.path);
    else layers[r]?.push(u.path);
  }
  const ordered = orderRanks(layers, edges);
  const tallest = Math.max(8, ...ordered.map((l) => l.length));
  loose.sort(byPath);
  for (let i = 0; i < loose.length; i += tallest) ordered.push(loose.slice(i, i + tallest));

  const at = new Map<string, { x: number; y: number }>();
  const step = LAYER.nodeW + LAYER.colGap;
  const rows = Math.max(...ordered.map((l) => l.length), 1);
  ordered.forEach((layer, r) => {
    const offset = ((rows - layer.length) * LAYER.rowH) / 2;
    layer.forEach((p, i) => {
      at.set(p, { x: r * step + LAYER.nodeW / 2, y: offset + i * LAYER.rowH + LAYER.rowH / 2 });
    });
  });
  return units.map((unit) => {
    const p = at.get(unit.path) ?? { x: 0, y: 0 };
    return { unit, x: p.x, y: p.y, w: LAYER.nodeW, h: unit.folder ? LAYER.folderH : LAYER.fileH };
  });
}

// ---- the focus view: users | the file | uses -----------------------------------

/** Neighbours shown on each side before "+N more". */
export const FOCUS_CAP = 12;

/** The neighbours worth a pill: most depended-on first, then by path, the
 *  first `cap` of them — plus the full sorted list for "+N more". */
export function capNeighbours<T extends { path: string; users: number }>(
  list: T[],
  cap: number = FOCUS_CAP,
): { all: T[]; shown: T[]; hidden: number } {
  const all = [...list].sort((a, b) => b.users - a.users || byPath(a.path, b.path));
  return { all, shown: all.slice(0, cap), hidden: Math.max(0, all.length - cap) };
}

export const FOCUS_GEO = { rowH: 36, pillH: 28, gap: 72, anchor: 30, minCenter: 300, maxCenter: 420, minSide: 200 } as const;

/** Where the three columns sit in a `width`-wide view: two equal side
 *  columns and the card between them, each side centred on the card's anchor
 *  (the line its connectors meet), all of it fixed arithmetic. */
export function focusGeometry(
  width: number,
  leftRows: number,
  rightRows: number,
): {
  sideW: number;
  centerW: number;
  width: number;
  leftTop: number;
  rightTop: number;
  cardTop: number;
  anchorY: number;
  pillY: (side: "left" | "right", i: number) => number;
} {
  const G = FOCUS_GEO;
  const centerW = Math.round(Math.min(G.maxCenter, Math.max(G.minCenter, width * 0.34)));
  const sideW = Math.max(G.minSide, Math.floor((width - centerW - 2 * G.gap) / 2));
  const hL = leftRows * G.rowH;
  const hR = rightRows * G.rowH;
  const anchorY = Math.max(hL, hR, 2 * G.anchor) / 2;
  const leftTop = anchorY - hL / 2;
  const rightTop = anchorY - hR / 2;
  return {
    sideW,
    centerW,
    width: sideW * 2 + centerW + G.gap * 2,
    leftTop,
    rightTop,
    cardTop: anchorY - G.anchor,
    anchorY,
    pillY: (side, i) => (side === "left" ? leftTop : rightTop) + i * G.rowH + G.rowH / 2,
  };
}

/** A horizontal S-curve from (x1, y1) to (x2, y2). */
export function connector(x1: number, y1: number, x2: number, y2: number): string {
  const dx = (x2 - x1) / 2;
  return `M${x1},${y1} C${x1 + dx},${y1} ${x2 - dx},${y2} ${x2},${y2}`;
}

/** The two halves of the code screen. */
export type CodeHalf = "folder" | "focus";

/** Which half shows when the link does not say: the file, if one is in focus. */
export function halfOf(view: CodeHalf | null | undefined, focus: string | null): CodeHalf {
  return view ?? (focus !== null ? "focus" : "folder");
}
