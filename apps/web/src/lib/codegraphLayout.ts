// The whole-root layout: a d3-force simulation seeded from the paths and
// run in steps, so the worker can hand back a picture every few ticks and a
// test can run the very same steps without a worker. Deterministic: every
// starting point comes from a stable hash of the path, and d3-force's own
// jiggle uses its fixed-seed random source, never Math.random.

import {
  forceCollide,
  forceLink,
  forceManyBody,
  forceSimulation,
  forceX,
  forceY,
  type Simulation,
  type SimulationLinkDatum,
  type SimulationNodeDatum,
} from "d3-force";
import { seedPosition } from "./codemap";

export interface LayoutInput {
  paths: string[];
  radii: number[];
  /** Each node's cluster, as an index into `anchors`. */
  cluster: number[];
  anchors: Array<{ x: number; y: number }>;
  edges: Array<[number, number]>;
}

/** When the layout is done: cool enough, or out of ticks. */
export const LAYOUT_ALPHA_MIN = 0.01;
export const LAYOUT_TICK_BUDGET = 320;
/** Ticks between two pictures sent back to the screen. */
export const LAYOUT_STREAM_EVERY = 20;

interface Node extends SimulationNodeDatum {
  r: number;
  ax: number;
  ay: number;
}

export interface LayoutRun {
  /** Run up to `ticks` more ticks (fewer if it cools first). */
  step: (ticks: number) => void;
  /** x, y interleaved, one pair per input node. */
  positions: () => Float32Array;
  ticks: () => number;
  alpha: () => number;
  done: () => boolean;
  /** 0 to 1: whichever of cooling and the tick budget is further along. */
  progress: () => number;
}

export function startLayout(input: LayoutInput, budget: number = LAYOUT_TICK_BUDGET): LayoutRun {
  const spread = 40 + Math.sqrt(input.paths.length) * 4;
  const nodes: Node[] = input.paths.map((path, i) => {
    const anchor = input.anchors[input.cluster[i] ?? 0] ?? { x: 0, y: 0 };
    const seed = seedPosition(path, spread * 2, spread * 2);
    return { r: input.radii[i] ?? 2, ax: anchor.x, ay: anchor.y, x: anchor.x + seed.x, y: anchor.y + seed.y };
  });
  const links: Array<SimulationLinkDatum<Node>> = input.edges.map(([a, b]) => ({ source: a, target: b }));
  const sim: Simulation<Node, SimulationLinkDatum<Node>> = forceSimulation(nodes)
    .alphaMin(LAYOUT_ALPHA_MIN)
    .force("link", forceLink<Node, SimulationLinkDatum<Node>>(links).distance(24).strength(0.12))
    .force("charge", forceManyBody<Node>().strength(-18).theta(1.1).distanceMax(260))
    .force("collide", forceCollide<Node>((d) => d.r + 1.5).iterations(1))
    // The pull toward each folder's anchor is also what centres the picture.
    .force("x", forceX<Node>((d) => d.ax).strength(0.045))
    .force("y", forceY<Node>((d) => d.ay).strength(0.045))
    .stop();
  let ticks = 0;
  const done = () => ticks >= budget || sim.alpha() < LAYOUT_ALPHA_MIN;
  return {
    step: (n) => {
      for (let i = 0; i < n && !done(); i += 1) {
        sim.tick();
        ticks += 1;
      }
    },
    positions: () => {
      const out = new Float32Array(nodes.length * 2);
      nodes.forEach((d, i) => {
        out[i * 2] = d.x ?? 0;
        out[i * 2 + 1] = d.y ?? 0;
      });
      return out;
    },
    ticks: () => ticks,
    alpha: () => sim.alpha(),
    done,
    progress: () => {
      const cooled = Math.log(Math.max(sim.alpha(), LAYOUT_ALPHA_MIN)) / Math.log(LAYOUT_ALPHA_MIN);
      return Math.min(1, Math.max(ticks / budget, cooled));
    },
  };
}

// ---- the message protocol the worker speaks ----------------------------------------

export interface LayoutRequest {
  id: number;
  input: LayoutInput;
}

export type LayoutMessage =
  | { id: number; kind: "tick"; positions: Float32Array; progress: number }
  | { id: number; kind: "done"; positions: Float32Array; ticks: number };

/** Run a whole layout, reporting every `LAYOUT_STREAM_EVERY` ticks — the
 *  worker's body, and the fallback where there is no worker. */
export function runLayout(request: LayoutRequest, post: (message: LayoutMessage) => void): void {
  const run = startLayout(request.input);
  while (!run.done()) {
    run.step(LAYOUT_STREAM_EVERY);
    if (!run.done()) post({ id: request.id, kind: "tick", positions: run.positions(), progress: run.progress() });
  }
  post({ id: request.id, kind: "done", positions: run.positions(), ticks: run.ticks() });
}
