// The whole-root network, the pure half: which files are drawn under the
// current filters, which cluster (top folder) each belongs to, how big each
// is, where the clusters are anchored, how the pointer finds a node, and what
// a search matches. No DOM, no worker — the renderer and the layout worker
// both build on this, and every piece of it is unit-tested.

import { hashString, lastSegment } from "./codemap";
import type { CodeGraphDto, CodeGraphFileDto } from "./types";

// ---- filters ----------------------------------------------------------------------

export interface GraphFilters {
  generated: boolean;
  tests: boolean;
  minUsers: number;
}

/** The owner's defaults: generated files hidden (they swamp the picture),
 *  tests shown, every file regardless of how many use it. */
export const DEFAULT_FILTERS: GraphFilters = { generated: false, tests: true, minUsers: 0 };

export const MIN_USERS_STEPS = [0, 1, 3, 10] as const;

/** A test file, by the conventions the repositories here use. */
export function isTestPath(path: string): boolean {
  return /\.(test|spec)\./.test(path) || path.includes("__tests__/") || path.startsWith("tests/") || path.includes("/tests/");
}

export interface FilteredGraph {
  files: CodeGraphFileDto[];
  /** Pairs of indexes into `files`, importer first. */
  edges: Array<[number, number]>;
  /** Each drawn file's index in the full graph. */
  source: number[];
  total: { files: number; edges: number };
}

/** The graph with the filters applied: files dropped, and every edge that
 *  touched a dropped file with them, indexes renumbered. */
export function filterGraph(graph: Pick<CodeGraphDto, "files" | "edges">, filters: GraphFilters): FilteredGraph {
  const keep = new Int32Array(graph.files.length).fill(-1);
  const files: CodeGraphFileDto[] = [];
  const source: number[] = [];
  graph.files.forEach((f, i) => {
    if (!filters.generated && f.generated) return;
    if (!filters.tests && isTestPath(f.path)) return;
    if (f.users < filters.minUsers) return;
    keep[i] = files.length;
    files.push(f);
    source.push(i);
  });
  const edges: Array<[number, number]> = [];
  for (const [a, b] of graph.edges) {
    const ka = keep[a] ?? -1;
    const kb = keep[b] ?? -1;
    if (ka >= 0 && kb >= 0 && ka !== kb) edges.push([ka, kb]);
  }
  return { files, edges, source, total: { files: graph.files.length, edges: graph.edges.length } };
}

/** A key for one filtered graph: the root, the filters, and a hash of what
 *  is actually drawn — so a commit that changes the files changes the key,
 *  and a cached layout is never laid over a different graph. */
export function layoutKey(root: string, filters: GraphFilters, graph: FilteredGraph): string {
  const paths = hashString(graph.files.map((f) => f.path).join("\n"));
  const edges = hashString(graph.edges.map(([a, b]) => `${a}>${b}`).join(","));
  return `${root}|g${filters.generated ? 1 : 0}t${filters.tests ? 1 : 0}u${filters.minUsers}|${paths.toString(36)}.${edges.toString(36)}`;
}

// ---- clusters ---------------------------------------------------------------------

/** The folder every path shares, as whole segments (`src` for a tree whose
 *  every file sits under `src/`). */
export function commonFolder(paths: readonly string[]): string {
  if (paths.length === 0) return "";
  let common = (paths[0] ?? "").split("/").slice(0, -1);
  for (const p of paths) {
    const parts = p.split("/").slice(0, -1);
    let n = 0;
    while (n < common.length && n < parts.length && common[n] === parts[n]) n += 1;
    common = common.slice(0, n);
    if (common.length === 0) break;
  }
  return common.join("/");
}

/** The name of files sitting directly in the common folder. */
export const ROOT_CLUSTER = "·";

/** The cluster a file belongs to: the first segment under the common
 *  folder (`src/components/x.tsx` → `components`). */
export function topFolderOf(path: string, common: string): string {
  const rest = common.length > 0 && path.startsWith(`${common}/`) ? path.slice(common.length + 1) : path;
  const slash = rest.indexOf("/");
  return slash < 0 ? ROOT_CLUSTER : rest.slice(0, slash);
}

/** A cluster's name for the legend: the files sitting directly in the
 *  common folder are not a folder of their own, so they are named after it. */
export function clusterLabel(name: string, common: string): string {
  if (name !== ROOT_CLUSTER) return name;
  return `${common.length > 0 ? lastSegment(common) : "top level"} (root files)`;
}

/** Colours in the palette; the last one is "other". */
export const PALETTE_SIZE = 10;

export interface Clusters {
  /** The folder every file shares; clusters are the folders under it. */
  common: string;
  /** Cluster names, most files first (ties by name). */
  names: string[];
  counts: number[];
  /** Each file's cluster, as an index into `names`. */
  of: Int32Array;
  /** Each cluster's colour slot: its rank among clusters with hand-written
   *  files, everything past the palette sharing the last slot; -1 for a
   *  cluster of generated files only, drawn muted. */
  colour: number[];
}

/** Clusters of the drawn files. A cluster made only of generated files is
 *  drawn muted like its files and takes no palette colour, so the
 *  hand-written clusters keep their colours whether generated files are
 *  shown or not. */
export function clustersOf(files: readonly { path: string; generated?: boolean }[]): Clusters {
  const common = commonFolder(files.map((f) => f.path));
  const tally = new Map<string, number>();
  const top = files.map((f) => topFolderOf(f.path, common));
  for (const t of top) tally.set(t, (tally.get(t) ?? 0) + 1);
  const names = [...tally.keys()].sort((a, b) => (tally.get(b) ?? 0) - (tally.get(a) ?? 0) || (a < b ? -1 : a > b ? 1 : 0));
  const index = new Map(names.map((n, i) => [n, i]));
  const of = new Int32Array(files.length);
  top.forEach((t, i) => {
    of[i] = index.get(t) ?? 0;
  });
  const handWritten = new Set<number>();
  files.forEach((f, i) => {
    if (!f.generated) handWritten.add(of[i] ?? 0);
  });
  let next = 0;
  const colour = names.map((_, i) => {
    if (!handWritten.has(i)) return -1;
    const slot = Math.min(next, PALETTE_SIZE - 1);
    next += 1;
    return slot;
  });
  return { common, names, counts: names.map((n) => tally.get(n) ?? 0), of, colour };
}

/** Where each cluster pulls its files: evenly round a circle whose size
 *  grows with the graph, biggest cluster at twelve o'clock. */
export function clusterAnchors(count: number, files: number): Array<{ x: number; y: number }> {
  if (count <= 1) return Array.from({ length: count }, () => ({ x: 0, y: 0 }));
  const radius = 60 + Math.sqrt(files) * 9;
  return Array.from({ length: count }, (_, i) => {
    const a = -Math.PI / 2 + (i / count) * Math.PI * 2;
    return { x: Math.cos(a) * radius, y: Math.sin(a) * radius };
  });
}

// ---- nodes ------------------------------------------------------------------------

/** Node radius from how many files use it: square root, 2 to 14 px. */
export function graphRadius(users: number): number {
  return Math.min(14, Math.max(2, 2 + Math.sqrt(Math.max(0, users)) * 1.2));
}

/** The indexes of the `k` most-used files — the ones labelled at any zoom. */
export function labelled(files: readonly { users: number; path: string }[], k = 40): Set<number> {
  const order = files.map((_, i) => i);
  order.sort((a, b) => (files[b]?.users ?? 0) - (files[a]?.users ?? 0) || ((files[a]?.path ?? "") < (files[b]?.path ?? "") ? -1 : 1));
  return new Set(order.slice(0, k));
}

/** Zoomed in past this, any node on screen may carry its label. */
export const LABEL_ALL_AT = 1.5;

/** Each file's degree (imports in + out) over the drawn edges. */
export function degrees(count: number, edges: ReadonlyArray<readonly [number, number]>): { ins: Int32Array; outs: Int32Array } {
  const ins = new Int32Array(count);
  const outs = new Int32Array(count);
  for (const [a, b] of edges) {
    outs[a] = (outs[a] ?? 0) + 1;
    ins[b] = (ins[b] ?? 0) + 1;
  }
  return { ins, outs };
}

/** Each file's neighbours over the drawn edges, both directions. */
export function adjacency(count: number, edges: ReadonlyArray<readonly [number, number]>): number[][] {
  const near: number[][] = Array.from({ length: count }, () => []);
  for (const [a, b] of edges) {
    near[a]?.push(b);
    near[b]?.push(a);
  }
  return near;
}

// ---- finding a node under the pointer ---------------------------------------------

/** A uniform grid over node positions. A lookup reads the cells around the
 *  point, so hovering costs the same on 6,000 nodes as on 60. */
export interface SpatialGrid {
  cell: number;
  cells: Map<string, number[]>;
}

export function buildGrid(xs: ArrayLike<number>, ys: ArrayLike<number>, cell = 32): SpatialGrid {
  const cells = new Map<string, number[]>();
  for (let i = 0; i < xs.length; i += 1) {
    const key = `${Math.floor((xs[i] ?? 0) / cell)},${Math.floor((ys[i] ?? 0) / cell)}`;
    const list = cells.get(key);
    if (list) list.push(i);
    else cells.set(key, [i]);
  }
  return { cell, cells };
}

/** The node nearest (x, y) whose disc — radius plus `slop` — contains it,
 *  or -1. Only cells that disc could reach are read. */
export function hitTest(
  grid: SpatialGrid,
  xs: ArrayLike<number>,
  ys: ArrayLike<number>,
  radii: ArrayLike<number>,
  x: number,
  y: number,
  slop = 0,
  maxRadius = 14,
): number {
  const reach = Math.ceil((maxRadius + slop) / grid.cell);
  const cx = Math.floor(x / grid.cell);
  const cy = Math.floor(y / grid.cell);
  let best = -1;
  let bestD = Number.POSITIVE_INFINITY;
  for (let gx = cx - reach; gx <= cx + reach; gx += 1) {
    for (let gy = cy - reach; gy <= cy + reach; gy += 1) {
      for (const i of grid.cells.get(`${gx},${gy}`) ?? []) {
        const d = Math.hypot((xs[i] ?? 0) - x, (ys[i] ?? 0) - y);
        if (d <= (radii[i] ?? 0) + slop && d < bestD) {
          best = i;
          bestD = d;
        }
      }
    }
  }
  return best;
}

// ---- search -----------------------------------------------------------------------

/** Files whose path contains the query (case-insensitive), best first: an
 *  exact file name, then a file name that starts with it, then the rest —
 *  each by users, then by the shorter path. */
export function searchGraph(files: readonly { path: string; users: number }[], query: string): number[] {
  const q = query.trim().toLowerCase();
  if (q.length === 0) return [];
  const scored: Array<{ i: number; rank: number }> = [];
  files.forEach((f, i) => {
    const path = f.path.toLowerCase();
    if (!path.includes(q)) return;
    const name = lastSegment(path);
    const stem = name.replace(/\.[^.]*$/, "");
    const rank = name === q || stem === q ? 0 : name.startsWith(q) ? 1 : 2;
    scored.push({ i, rank });
  });
  scored.sort(
    (a, b) =>
      a.rank - b.rank ||
      (files[b.i]?.users ?? 0) - (files[a.i]?.users ?? 0) ||
      (files[a.i]?.path.length ?? 0) - (files[b.i]?.path.length ?? 0),
  );
  return scored.map((s) => s.i);
}

/** The bounding box of the positions, padded. */
export function positionBounds(xs: ArrayLike<number>, ys: ArrayLike<number>, pad = 40): { x: number; y: number; w: number; h: number } {
  if (xs.length === 0) return { x: -100, y: -100, w: 200, h: 200 };
  let minX = Number.POSITIVE_INFINITY;
  let minY = Number.POSITIVE_INFINITY;
  let maxX = Number.NEGATIVE_INFINITY;
  let maxY = Number.NEGATIVE_INFINITY;
  for (let i = 0; i < xs.length; i += 1) {
    const x = xs[i] ?? 0;
    const y = ys[i] ?? 0;
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
  }
  return { x: minX - pad, y: minY - pad, w: maxX - minX + pad * 2, h: maxY - minY + pad * 2 };
}

/** The transform that puts world point (wx, wy) at the centre of a
 *  `width` × `height` viewport at scale `k`. */
export function centreOn(wx: number, wy: number, width: number, height: number, k: number): { x: number; y: number; k: number } {
  return { k, x: width / 2 - wx * k, y: height / 2 - wy * k };
}

/** Zoom range for the whole-root picture: thousands of nodes need to fit. */
export const GRAPH_ZOOM = { min: 0.02, max: 8 } as const;

/** Thousands, the way the counts line prints them. */
export function formatCount(n: number): string {
  return n.toLocaleString("en-US");
}

/** How to copy a layer painted at view `from` so it lands where view `to`
 *  would paint it: a scale `s` and an offset (dx, dy) in CSS pixels. Panning
 *  and zooming then cost one image draw, whatever the layer holds. */
export function blitTransform(
  from: { x: number; y: number; k: number },
  to: { x: number; y: number; k: number },
): { s: number; dx: number; dy: number } {
  const s = to.k / from.k;
  return { s, dx: to.x - from.x * s, dy: to.y - from.y * s };
}

/** Which labels to draw, given their boxes in priority order: each one only
 *  if it overlaps none already placed — so the most important label in a
 *  crowd wins and the rest wait for a closer zoom. */
export function placeLabels(boxes: ReadonlyArray<{ x: number; y: number; w: number; h: number }>, gap = 2): boolean[] {
  const placed: Array<{ x: number; y: number; w: number; h: number }> = [];
  const cell = 64;
  const grid = new Map<string, number[]>();
  const cellsOf = (b: { x: number; y: number; w: number; h: number }) => {
    const keys: string[] = [];
    for (let gx = Math.floor(b.x / cell); gx <= Math.floor((b.x + b.w) / cell); gx += 1)
      for (let gy = Math.floor(b.y / cell); gy <= Math.floor((b.y + b.h) / cell); gy += 1) keys.push(`${gx},${gy}`);
    return keys;
  };
  return boxes.map((b) => {
    const keys = cellsOf(b);
    for (const key of keys) {
      for (const j of grid.get(key) ?? []) {
        const o = placed[j];
        if (!o) continue;
        if (b.x < o.x + o.w + gap && o.x < b.x + b.w + gap && b.y < o.y + o.h + gap && o.y < b.y + b.h + gap) return false;
      }
    }
    placed.push(b);
    for (const key of keys) grid.set(key, [...(grid.get(key) ?? []), placed.length - 1]);
    return true;
  });
}
