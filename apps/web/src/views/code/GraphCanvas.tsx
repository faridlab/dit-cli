// The whole-root network on a <canvas>: thousands of files as circles and
// every import between them as a hairline. SVG would need a DOM node per
// circle and per line; a canvas redraws the lot in a few milliseconds, and
// only when something changed. The view pans and zooms with the same maths
// as the folder graph; the pointer finds a node through a spatial grid, never
// by scanning all of them.

import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { Maximize, Minus, Plus } from "lucide-react";
import {
  fitTransform,
  isDrag,
  lastSegment,
  middleTruncate,
  panBy,
  wheelFactor,
  ZOOM,
  zoomAt,
  type ViewTransform,
} from "../../lib/codemap";
import {
  adjacency,
  buildGrid,
  centreOn,
  degrees,
  GRAPH_ZOOM,
  hitTest,
  LABEL_ALL_AT,
  labelled,
  PALETTE_SIZE,
  blitTransform,
  placeLabels,
  positionBounds,
} from "../../lib/codegraph";
import type { CodeGraphFileDto } from "../../lib/types";
import { useTheme } from "../../lib/theme";
import { IBtn } from "../../components/chrome";

export interface GraphCanvasHandle {
  /** Centre and zoom on one node. */
  centre: (index: number) => void;
}

interface Colours {
  palette: string[];
  edge: string;
  accent: string;
  label: string;
  halo: string;
  muted: string;
}

function readColours(): Colours {
  const css = getComputedStyle(document.documentElement);
  const v = (name: string, fallback: string) => css.getPropertyValue(name).trim() || fallback;
  return {
    palette: Array.from({ length: PALETTE_SIZE }, (_, i) => v(`--dit-cat-${i}`, "#8b98a1")),
    edge: v("--dit-ink-2", "#414c54"),
    accent: v("--dit-accent", "#0f766e"),
    label: v("--dit-ink", "#161b1e"),
    halo: v("--dit-app", "#ffffff"),
    muted: v("--dit-dim", "#a6b0b7"),
  };
}

/** Segments per stroked path. One path of thousands of translucent lines
 *  is rasterised as a whole, to keep overlaps from darkening — on a software
 *  canvas at 2× that alone costs over 100 ms a frame. Paths of a few dozen
 *  lines cost almost nothing, and the overlap they give up is invisible at
 *  this alpha. */
const STROKE_BATCH = 64;
const FILL_BATCH = 128;

function strokeSegments(
  ctx: CanvasRenderingContext2D,
  pos: Float32Array,
  edges: ReadonlyArray<readonly [number, number]>,
  keep: (a: number, b: number) => boolean,
): void {
  let n = 0;
  ctx.beginPath();
  for (const [a, b] of edges) {
    if (!keep(a, b)) continue;
    ctx.moveTo(pos[a * 2] ?? 0, pos[a * 2 + 1] ?? 0);
    ctx.lineTo(pos[b * 2] ?? 0, pos[b * 2 + 1] ?? 0);
    n += 1;
    if (n % STROKE_BATCH === 0) {
      ctx.stroke();
      ctx.beginPath();
    }
  }
  ctx.stroke();
}

/** Labels drawn at most, when zoomed in far enough to label anything. */
const LABEL_CAP = 300;

/** After the last pan or zoom, how long before the layer is repainted sharp. */
const SETTLE_MS = 140;
/** While the layout streams in, the layer is repainted at most this often. */
const REPAINT_MS = 250;

interface Scene {
  files: CodeGraphFileDto[];
  edges: Array<[number, number]>;
  positions: Float32Array | null;
  radii: Float32Array;
  slot: Int32Array;
  cluster: Int32Array;
  isolated: number | null;
  matchSet: Set<number>;
  near: number[][];
  top: Set<number>;
}

/** The world rectangle a view shows, padded, to skip what cannot be seen. */
function visibleTest(pos: Float32Array, t: ViewTransform, w: number, h: number) {
  const x0 = -t.x / t.k - 20;
  const y0 = -t.y / t.k - 20;
  const x1 = (w - t.x) / t.k + 20;
  const y1 = (h - t.y) / t.k + 20;
  return (i: number) => {
    const x = pos[i * 2] ?? 0;
    const y = pos[i * 2 + 1] ?? 0;
    return x >= x0 && x <= x1 && y >= y0 && y <= y1;
  };
}

function fillNodes(
  ctx: CanvasRenderingContext2D,
  p: Scene,
  pos: Float32Array,
  keep: (i: number) => boolean,
  dimmed: (i: number) => boolean,
  c: Colours,
): void {
  // One path per (colour, generated, dimmed), each cut into paths of
  // FILL_BATCH circles: forty fill-style changes a frame, not thousands.
  const groups = new Map<number, Path2D[]>();
  const counts = new Map<Path2D, number>();
  for (let i = 0; i < p.files.length; i += 1) {
    if (!keep(i)) continue;
    const gen = p.files[i]?.generated === true;
    // Slot -1 (a generated-only cluster) is shifted to 0.. by +1 so the key stays non-negative.
    const key = (((p.slot[i] ?? 0) + 1) * 2 + (gen ? 1 : 0)) * 2 + (dimmed(i) ? 0 : 1);
    let paths = groups.get(key);
    if (!paths) {
      paths = [];
      groups.set(key, paths);
    }
    let path = paths[paths.length - 1];
    if (!path || (counts.get(path) ?? 0) >= FILL_BATCH) {
      path = new Path2D();
      paths.push(path);
    }
    counts.set(path, (counts.get(path) ?? 0) + 1);
    const x = pos[i * 2] ?? 0;
    const y = pos[i * 2 + 1] ?? 0;
    const r = p.radii[i] ?? 2;
    path.moveTo(x + r, y);
    path.arc(x, y, r, 0, Math.PI * 2);
  }
  for (const [key, paths] of groups) {
    const on = key % 2 === 1;
    const gen = Math.floor(key / 2) % 2 === 1;
    const slotIndex = Math.floor(key / 4) - 1;
    ctx.fillStyle = gen || slotIndex < 0 ? c.muted : (c.palette[slotIndex] ?? c.muted);
    ctx.globalAlpha = (on ? 1 : 0.14) * (gen ? 0.55 : 0.9);
    for (const path of paths) ctx.fill(path);
  }
}

function ringNodes(ctx: CanvasRenderingContext2D, p: Scene, pos: Float32Array, nodes: Iterable<number>, k: number, c: Colours): void {
  ctx.globalAlpha = 1;
  ctx.strokeStyle = c.accent;
  ctx.lineWidth = 2 / k;
  ctx.beginPath();
  let n = 0;
  for (const i of nodes) {
    if (n++ > 200) break;
    const x = pos[i * 2] ?? 0;
    const y = pos[i * 2 + 1] ?? 0;
    const r = (p.radii[i] ?? 2) + 2 / k;
    ctx.moveTo(x + r, y);
    ctx.arc(x, y, r, 0, Math.PI * 2);
  }
  ctx.stroke();
}

function paintLabels(
  ctx: CanvasRenderingContext2D,
  p: Scene,
  pos: Float32Array,
  t: ViewTransform,
  dpr: number,
  nodes: number[],
  c: Colours,
): void {
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.font = "500 11px 'IBM Plex Mono', ui-monospace, monospace";
  ctx.textBaseline = "middle";
  ctx.lineJoin = "round";
  // Biggest first, and a label that would overlap one already drawn waits
  // for a closer zoom.
  const sorted = [...nodes].sort((a, b) => (p.radii[b] ?? 0) - (p.radii[a] ?? 0)).slice(0, LABEL_CAP);
  const items = sorted.map((i) => {
    const text = middleTruncate(lastSegment(p.files[i]?.path ?? ""), 28);
    const x = (pos[i * 2] ?? 0) * t.k + t.x + (p.radii[i] ?? 2) * t.k + 4;
    const y = (pos[i * 2 + 1] ?? 0) * t.k + t.y;
    return { text, x, y, w: ctx.measureText(text).width };
  });
  const keep = placeLabels(items.map((l) => ({ x: l.x, y: l.y - 7, w: l.w, h: 14 })));
  items.forEach((l, n) => {
    if (!keep[n]) return;
    ctx.globalAlpha = 0.9;
    ctx.strokeStyle = c.halo;
    ctx.lineWidth = 3;
    ctx.strokeText(l.text, l.x, l.y);
    ctx.globalAlpha = 1;
    ctx.fillStyle = c.label;
    ctx.fillText(l.text, l.x, l.y);
  });
}

/** The layer: every edge and node, the isolated cluster or the search
 *  matches lit, and the labels — everything but the hover. */
function paintScene(
  ctx: CanvasRenderingContext2D,
  p: Scene,
  t: ViewTransform,
  box: { w: number; h: number; dpr: number },
  c: Colours,
): void {
  const pos = p.positions;
  if (!pos) return;
  const { w, h, dpr } = box;
  const visible = visibleTest(pos, t, w, h);
  const emphasis = p.isolated !== null || p.matchSet.size > 0;
  const lit = (i: number) => (p.isolated !== null ? p.cluster[i] === p.isolated : p.matchSet.size > 0 ? p.matchSet.has(i) : true);

  ctx.setTransform(dpr * t.k, 0, 0, dpr * t.k, dpr * t.x, dpr * t.y);
  // Edges faint, exactly one device pixel wide: a hairline rasterises on a
  // fast path, where a wider translucent line costs tens of milliseconds a
  // frame at 2× once there are thousands. Thinner at 2×, so less see-through.
  ctx.lineWidth = 1 / (t.k * dpr);
  ctx.strokeStyle = c.edge;
  ctx.globalAlpha = emphasis ? 0.05 : dpr > 1 ? 0.17 : 0.1;
  strokeSegments(ctx, pos, p.edges, (a, b) => visible(a) || visible(b));
  if (emphasis) {
    ctx.globalAlpha = 0.3;
    ctx.lineWidth = 0.8 / t.k;
    strokeSegments(ctx, pos, p.edges, (a, b) => lit(a) && lit(b) && (visible(a) || visible(b)));
  }
  fillNodes(ctx, p, pos, visible, (i) => emphasis && !lit(i), c);
  if (p.matchSet.size > 0) ringNodes(ctx, p, pos, [...p.matchSet].filter(visible), t.k, c);

  // Labels in screen space so the text stays crisp: the most-used files
  // always, anything on screen once zoomed in, only lit ones under emphasis.
  const labels: number[] = [];
  for (let i = 0; i < p.files.length && labels.length < LABEL_CAP * 4; i += 1) {
    if (!visible(i) || (emphasis && !lit(i))) continue;
    if (p.top.has(i) || t.k >= LABEL_ALL_AT || (p.matchSet.has(i) && p.matchSet.size <= 40)) labels.push(i);
  }
  paintLabels(ctx, p, pos, t, dpr, labels, c);
}

/** The hover, over the copied layer: a veil that dims everything, then the
 *  hovered node, its neighbours and the edges between them, bright. */
function paintHover(
  ctx: CanvasRenderingContext2D,
  p: Scene,
  hv: number,
  t: ViewTransform,
  box: { w: number; h: number; dpr: number },
  c: Colours,
): void {
  const pos = p.positions;
  if (!pos || hv < 0) return;
  const { dpr } = box;
  const lit = new Set([hv, ...(p.near[hv] ?? [])]);
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalAlpha = 0.72;
  ctx.fillStyle = c.halo;
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.setTransform(dpr * t.k, 0, 0, dpr * t.k, dpr * t.x, dpr * t.y);
  ctx.strokeStyle = c.accent;
  ctx.globalAlpha = 0.85;
  ctx.lineWidth = 1.4 / t.k;
  strokeSegments(ctx, pos, p.edges, (a, b) => a === hv || b === hv);
  fillNodes(ctx, p, pos, (i) => lit.has(i), () => false, c);
  ringNodes(ctx, p, pos, [hv], t.k, c);
  paintLabels(ctx, p, pos, t, dpr, [...lit], c);
  ctx.globalAlpha = 1;
}

export const GraphCanvas = forwardRef<
  GraphCanvasHandle,
  {
    files: CodeGraphFileDto[];
    edges: Array<[number, number]>;
    /** x, y interleaved; null until the first picture arrives. */
    positions: Float32Array | null;
    radii: Float32Array;
    /** Each node's colour slot. */
    slot: Int32Array;
    /** Each node's cluster. */
    cluster: Int32Array;
    /** The cluster isolated from the legend, if any. */
    isolated: number | null;
    /** Nodes a search matched, best first. */
    matches: number[];
    /** The layout is still streaming pictures in. */
    settling: boolean;
    onOpen: (index: number) => void;
  }
>(function GraphCanvas({ files, edges, positions, radii, slot, cluster, isolated, matches, settling, onOpen }, ref) {
  const streaming = useRef(settling);
  streaming.current = settling;
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const size = useRef({ w: 0, h: 0, dpr: 1 });
  const view = useRef<ViewTransform>({ x: 0, y: 0, k: 1 });
  const moved = useRef(false);
  const hover = useRef(-1);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const raf = useRef(0);
  const [pct, setPct] = useState(100);
  const [tip, setTip] = useState<{ i: number; x: number; y: number } | null>(null);
  const theme = useTheme();
  const colours = useRef<Colours | null>(null);

  const near = useMemo(() => adjacency(files.length, edges), [edges, files.length]);
  const degree = useMemo(() => degrees(files.length, edges), [edges, files.length]);
  const top = useMemo(() => labelled(files), [files]);
  const matchSet = useMemo(() => new Set(matches.slice(0, 500)), [matches]);
  const grid = useMemo(() => {
    if (!positions) return null;
    const xs = new Float32Array(files.length);
    const ys = new Float32Array(files.length);
    for (let i = 0; i < files.length; i += 1) {
      xs[i] = positions[i * 2] ?? 0;
      ys[i] = positions[i * 2 + 1] ?? 0;
    }
    return { xs, ys, grid: buildGrid(xs, ys, 32) };
  }, [files.length, positions]);

  // Everything the frame reads, in one ref, so the draw loop never waits on
  // React and React never re-renders to draw.
  const props = useRef<Scene & { grid: typeof grid }>({ files, edges, positions, radii, slot, cluster, isolated, matchSet, near, top, grid });
  props.current = { files, edges, positions, radii, slot, cluster, isolated, matchSet, near, top, grid };

  // -- drawing -----------------------------------------------------------------
  // The whole scene is painted into an off-screen layer only when what it
  // shows changes, and at rest. Panning and zooming copy that layer across
  // with a transform — one image draw, however many lines it holds — and the
  // layer is repainted at the new view once the gesture pauses. Hover is an
  // overlay on top: a veil and the handful of lit nodes, never a repaint.
  const scheduleRef = useRef<() => void>(() => undefined);
  const layer = useRef<{ canvas: HTMLCanvasElement | null; view: ViewTransform | null; at: number }>({ canvas: null, view: null, at: 0 });
  const stale = useRef<"none" | "view" | "content">("content");
  const lastGesture = useRef(0);
  const idle = useRef<ReturnType<typeof setTimeout> | null>(null);

  const paintLayer = useCallback(() => {
    const el = canvas.current;
    const p = props.current;
    if (!el || !p.positions) return;
    let base = layer.current.canvas;
    if (!base) {
      base = document.createElement("canvas");
      layer.current.canvas = base;
    }
    if (base.width !== el.width || base.height !== el.height) {
      base.width = el.width;
      base.height = el.height;
    }
    const ctx = base.getContext("2d");
    if (!ctx) return;
    const c = colours.current ?? readColours();
    colours.current = c;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, base.width, base.height);
    paintScene(ctx, p, view.current, size.current, c);
    layer.current.view = view.current;
    layer.current.at = performance.now();
    stale.current = "none";
  }, []);

  const draw = useCallback(() => {
    const el = canvas.current;
    const ctx = el?.getContext("2d");
    const p = props.current;
    if (!el || !ctx) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.clearRect(0, 0, el.width, el.height);
    if (!p.positions) return;
    const now = performance.now();
    const gesturing = now - lastGesture.current < SETTLE_MS;
    // Content changes repaint at once, except the stream of pictures while
    // the layout settles, which repaints at most every REPAINT_MS.
    if (stale.current === "content" && (layer.current.view === null || now - layer.current.at >= REPAINT_MS || !streaming.current)) {
      paintLayer();
    } else if (stale.current === "view" && !gesturing) {
      paintLayer();
    }
    const base = layer.current.canvas;
    const from = layer.current.view;
    if (base && from) {
      const b = blitTransform(from, view.current);
      const dpr = size.current.dpr;
      ctx.setTransform(b.s, 0, 0, b.s, b.dx * dpr, b.dy * dpr);
      ctx.drawImage(base, 0, 0);
    }
    const c = colours.current ?? readColours();
    paintHover(ctx, p, hover.current, view.current, size.current, c);
    // Whatever is still owed gets a frame once the gesture or the throttle
    // has passed.
    if (stale.current !== "none") {
      if (idle.current) clearTimeout(idle.current);
      idle.current = setTimeout(() => {
        idle.current = null;
        scheduleRef.current();
      }, stale.current === "view" ? SETTLE_MS : REPAINT_MS);
    }
  }, [paintLayer]);

  const frame = useCallback(() => {
    raf.current = 0;
    // Hover is read once a frame, however many pointer events arrived.
    const pt = pointer.current;
    const p = props.current;
    if (pt && p.grid) {
      const t = view.current;
      const i = hitTest(p.grid.grid, p.grid.xs, p.grid.ys, p.radii, (pt.x - t.x) / t.k, (pt.y - t.y) / t.k, 4 / t.k);
      if (i !== hover.current) {
        hover.current = i;
        setTip(i >= 0 ? { i, x: pt.x, y: pt.y } : null);
      } else if (i >= 0) {
        setTip((old) => (old && old.i === i && Math.abs(old.x - pt.x) + Math.abs(old.y - pt.y) < 2 ? old : { i, x: pt.x, y: pt.y }));
      }
    } else if (!pt && hover.current !== -1) {
      hover.current = -1;
      setTip(null);
    }
    draw();
  }, [draw]);

  const schedule = useCallback(() => {
    if (raf.current === 0) raf.current = requestAnimationFrame(frame);
  }, [frame]);
  scheduleRef.current = schedule;

  /** Something the layer shows changed: repaint it. */
  const invalidate = useCallback(() => {
    stale.current = "content";
    schedule();
  }, [schedule]);

  const setView = useCallback(
    (t: ViewTransform, byHand: boolean) => {
      view.current = t;
      if (byHand) {
        moved.current = true;
        if (stale.current === "none") stale.current = "view";
        lastGesture.current = performance.now();
      } else {
        stale.current = "content";
      }
      setPct(Math.round(t.k * 100));
      schedule();
    },
    [schedule],
  );

  const fit = useCallback(() => {
    const p = props.current;
    if (!p.grid) return;
    const { w, h } = size.current;
    setView(fitTransform(positionBounds(p.grid.xs, p.grid.ys), w, h, "whole", GRAPH_ZOOM), false);
    moved.current = false;
  }, [setView]);

  useImperativeHandle(
    ref,
    () => ({
      centre: (i: number) => {
        const p = props.current;
        if (!p.grid) return;
        const { w, h } = size.current;
        setView(centreOn(p.grid.xs[i] ?? 0, p.grid.ys[i] ?? 0, w, h, Math.max(view.current.k, 2)), true);
      },
    }),
    [setView],
  );

  // Size the canvas to its box, in device pixels.
  useEffect(() => {
    const element = host.current;
    const el = canvas.current;
    if (!element || !el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (!entry) return;
      const dpr = window.devicePixelRatio || 1;
      const w = entry.contentRect.width;
      const h = entry.contentRect.height;
      size.current = { w, h, dpr };
      el.width = Math.max(1, Math.round(w * dpr));
      el.height = Math.max(1, Math.round(h * dpr));
      el.style.width = `${w}px`;
      el.style.height = `${h}px`;
      if (!moved.current) fit();
      invalidate();
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [fit, invalidate]);

  // A new picture from the layout: keep it framed until the reader moves it.
  useEffect(() => {
    if (!moved.current) fit();
    invalidate();
  }, [fit, grid, invalidate]);

  // Anything else that changes what is drawn.
  useEffect(() => {
    colours.current = null;
    invalidate();
  }, [edges, isolated, matchSet, invalidate, theme.resolved]);

  // Cancelling must also forget the frame: `schedule` only asks for a new one
  // when none is pending, so a cancelled id left behind (StrictMode runs this
  // cleanup once on mount) would stop every redraw for good.
  useEffect(
    () => () => {
      cancelAnimationFrame(raf.current);
      raf.current = 0;
      if (idle.current) clearTimeout(idle.current);
      idle.current = null;
    },
    [],
  );

  // Wheel zoom must not scroll the page, so it cannot be React's passive one.
  useEffect(() => {
    const el = canvas.current;
    if (!el) return;
    const onWheel = (ev: WheelEvent) => {
      ev.preventDefault();
      const r = el.getBoundingClientRect();
      setView(zoomAt(view.current, wheelFactor(ev.deltaY, ev.deltaMode), ev.clientX - r.left, ev.clientY - r.top, GRAPH_ZOOM), true);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [setView]);

  const press = useRef<{ id: number; x: number; y: number; from: ViewTransform; dragging: boolean } | null>(null);
  const local = (ev: React.PointerEvent) => {
    const r = canvas.current?.getBoundingClientRect();
    return { x: ev.clientX - (r?.left ?? 0), y: ev.clientY - (r?.top ?? 0) };
  };

  const zoomBy = (factor: number) =>
    setView(zoomAt(view.current, factor, size.current.w / 2, size.current.h / 2, GRAPH_ZOOM), true);

  const tipFile = tip ? files[tip.i] : undefined;

  return (
    <div ref={host} className="relative min-h-0 flex-1 overflow-hidden">
      <canvas
        ref={canvas}
        className="absolute inset-0 touch-none select-none"
        style={{ cursor: tip ? "pointer" : "grab" }}
        aria-label="Every file of the root and the imports between them"
        role="img"
        onPointerDown={(ev) => {
          if (ev.button !== 0) return;
          press.current = { id: ev.pointerId, x: ev.clientX, y: ev.clientY, from: view.current, dragging: false };
        }}
        onPointerMove={(ev) => {
          const p = press.current;
          if (p && p.id === ev.pointerId) {
            const dx = ev.clientX - p.x;
            const dy = ev.clientY - p.y;
            if (p.dragging || isDrag(dx, dy)) {
              if (!p.dragging) {
                p.dragging = true;
                canvas.current?.setPointerCapture?.(ev.pointerId);
                pointer.current = null;
              }
              setView(panBy(p.from, dx, dy), true);
              return;
            }
          }
          pointer.current = local(ev);
          schedule();
        }}
        onPointerUp={(ev) => {
          const p = press.current;
          press.current = null;
          if (!p || p.id !== ev.pointerId) return;
          if (p.dragging) {
            canvas.current?.releasePointerCapture?.(ev.pointerId);
            return;
          }
          if (hover.current >= 0) onOpen(hover.current);
        }}
        onPointerCancel={() => {
          press.current = null;
        }}
        onPointerLeave={() => {
          pointer.current = null;
          schedule();
        }}
        onDoubleClick={() => {
          if (hover.current < 0) fit();
        }}
      />

      {tip && tipFile ? (
        <div
          role="tooltip"
          className="pointer-events-none absolute z-10 max-w-96 rounded-md border border-edge bg-card px-3 py-2 text-xs shadow-lg"
          style={{ left: tip.x + 14, top: tip.y + 14 }}
        >
          <div className="break-all font-mono text-[11.5px] text-ink">{tipFile.path}</div>
          <div className="mt-1 flex flex-wrap gap-x-3 text-muted">
            <span>
              {tipFile.users} user{tipFile.users === 1 ? "" : "s"}
            </span>
            <span>
              {degree.outs[tip.i] ?? 0} import{(degree.outs[tip.i] ?? 0) === 1 ? "" : "s"} shown
            </span>
            {tipFile.generated ? <span className="text-warn-text">generated</span> : null}
          </div>
          <div className="mt-1 text-faint">Click to focus the file</div>
        </div>
      ) : null}

      <div className="absolute top-3 right-3 flex items-center gap-1 rounded-md border border-edge bg-card/90 p-0.5">
        <IBtn onClick={() => zoomBy(ZOOM.step)} title="Zoom in" aria-label="Zoom in">
          <Plus className="i" aria-hidden />
        </IBtn>
        <IBtn onClick={() => zoomBy(1 / ZOOM.step)} title="Zoom out" aria-label="Zoom out">
          <Minus className="i" aria-hidden />
        </IBtn>
        <IBtn onClick={fit} title="Fit the network (or double-click the background)" aria-label="Fit">
          <Maximize className="i" aria-hidden />
        </IBtn>
        <span className="w-10 text-center font-mono text-[10.5px] text-muted">{pct}%</span>
      </div>
    </div>
  );
});
