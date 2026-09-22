// The Flow screen (ADR 0019): an orchestration rendered as a diagram —
// issues as nodes on a computed stage grid, `blocked_by` as orthogonal
// edges, lanes as horizontal bands, the critical path emphasized.
//
// The visual language is archify's, ported from its generated artifacts
// (github.com/tt-a1i/archify), not re-imagined: dotted ground grid, dashed
// lane bands labelled "01 / Name" top-left, stage rails running the whole
// height under mask-chip captions, nodes as an opaque mask rect under a
// semantic fill/stroke pair (ready=green, in-flight=cyan, blocked=rose,
// done=slate) with a status sigil, wrapped label and sublabel, and
// orthogonal edges with per-variant arrowheads (default, emphasis for the
// critical path, dashed for satisfied, rose-dashed for broken).
//
// What archify's artifacts do beyond drawing is the point of this screen,
// and it is ported too: the diagram is a thing you read, not a picture. It
// pans and zooms and fits, it has a minimap, `f` finds a node by name or
// number, selecting a node lights its whole dependency reach and dims the
// rest, two picks trace the route between them, and the legend isolates a
// colour. The selection itself lives in the view options, so the sidebar's
// Flow section is looking at the same thing the canvas is.
//
// Everything on screen is derived server-side and never written back; the
// screen is read-only, live on any process's write.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  ChevronLeft,
  ChevronRight,
  Crosshair,
  HelpCircle,
  Maximize2,
  Minimize2,
  Minus,
  Palette,
  Pause,
  Play,
  Plus,
  Search,
  Waypoints,
  X,
} from "lucide-react";
import { useFlows, useFlowBoard } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import { ALL_FLOWS, FLOW_PAINTS, useViewOptions, type FlowPaint } from "../lib/viewopts";
import { replaceRoute, useRoute } from "../lib/router";
import {
  METRICS as M,
  buildGraph,
  chainEdges,
  columnLabel,
  detailAt,
  edgePath,
  layoutBoard,
  paintKeyOf,
  groupFrames,
  paintLegend,
  paintSlot,
  reach,
  storyChapters,
  routeBetween,
  searchNodes,
  semanticOf,
  wrapLabel,
  type Detail,
  type FlowLayout,
  type Semantic,
} from "../lib/flowgraph";
import type {
  FlowBoardDto,
  FlowEdgeDto,
  FlowNodeDto,
  FlowShapeProblemDto,
} from "../lib/types";

const MIN_SCALE = 0.25;
const MAX_SCALE = 2.4;
const TITLE_CHARS = 25;
const TITLE_LINES = 2;

/** Everything both the canvas and the sidebar's Flow section read. The board
 *  is fetched once and shared by react-query; every set below it is derived
 *  from `nodes` + `edges` and nothing is stored. */
export function useFlowModel() {
  const { flow: opts, selectFlowNode } = useViewOptions();
  const flows = useFlows();
  const board = useFlowBoard(opts.flow);
  const data = board.data ?? null;

  const graph = useMemo(() => (data ? buildGraph(data) : null), [data]);
  const layout = useMemo(() => (data ? layoutBoard(data) : null), [data]);

  // The legend for whichever dimension the colours currently mean.
  const legend = useMemo(
    () => (data ? paintLegend(data.nodes, opts.paint) : []),
    [data, opts.paint],
  );

  // The guided reading, derived: the fence's phases when there is one, the
  // critical path when there is not. Nobody authors a chapter list.
  const chapters = useMemo(() => (data ? storyChapters(data) : []), [data]);
  const chapter =
    opts.chapter === null ? null : (chapters[opts.chapter] ?? null);

  const mainPath = data?.main_path ?? [];
  const mainEdges = useMemo(() => chainEdges(mainPath), [mainPath]);

  const selected = opts.selected !== null ? (graph?.node.get(opts.selected) ?? null) : null;
  const upstream = useMemo(
    () => (graph && selected ? reach(graph, selected.id, "up") : new Set<string>()),
    [graph, selected],
  );
  const downstream = useMemo(
    () => (graph && selected ? reach(graph, selected.id, "down") : new Set<string>()),
    [graph, selected],
  );

  // The route probe: two picks, the chain of dependencies that joins them.
  const route = useMemo(() => {
    if (!graph || opts.from === null || opts.to === null) return null;
    return routeBetween(graph, opts.from, opts.to);
  }, [graph, opts.from, opts.to]);
  const routeEdges = useMemo(() => chainEdges(route ?? []), [route]);

  // What draws lit. Null means "everything" — the diagram at rest. The
  // probe wins over the lens, and the lens over the legend, because that is
  // the order the reader asked for them.
  const lit = useMemo<ReadonlySet<string> | null>(() => {
    if (route !== null) return new Set(route);
    if (selected !== null) return new Set([selected.id, ...upstream, ...downstream]);
    if (chapter !== null) return new Set(chapter.focus);
    if (opts.isolate.size > 0 && data !== null) {
      return new Set(
        data.nodes.filter((n) => opts.isolate.has(paintKeyOf(n, opts.paint))).map((n) => n.id),
      );
    }
    return null;
  }, [route, selected, chapter, upstream, downstream, opts.isolate, opts.paint, data]);

  const litEdges = useMemo<ReadonlySet<string> | null>(() => {
    if (route !== null) return routeEdges;
    if (lit === null || data === null) return null;
    return new Set(
      data.edges.filter((e) => lit.has(e.from) && lit.has(e.to)).map((e) => `${e.from}>${e.to}`),
    );
  }, [route, routeEdges, lit, data]);

  // Stepping the critical path: where the selection sits on it, if at all.
  const pathIndex = selected === null ? -1 : mainPath.indexOf(selected.id);
  const stepPath = useCallback(
    (delta: -1 | 1) => {
      if (mainPath.length === 0) return;
      const next = pathIndex === -1 ? (delta === 1 ? 0 : mainPath.length - 1) : pathIndex + delta;
      const id = mainPath[Math.max(0, Math.min(mainPath.length - 1, next))];
      if (id !== undefined) selectFlowNode(id, { center: true });
    },
    [mainPath, pathIndex, selectFlowNode],
  );

  return {
    flows,
    board,
    data,
    graph,
    layout,
    legend,
    chapters,
    chapter,
    mainPath,
    mainEdges,
    selected,
    upstream,
    downstream,
    route,
    lit,
    litEdges,
    pathIndex,
    stepPath,
  };
}

export function FlowView({ onOpen }: { onOpen: (id: string) => void }) {
  const model = useFlowModel();
  const { flow: opts, setFlow, applyFlowReading } = useViewOptions();
  const { flows, board, data } = model;
  useFlowUrl(opts, applyFlowReading);

  // J/K behind an open panel walks what is pickable here: the ready nodes,
  // in the order the diagram draws them.
  useRegisterPeekList(
    useMemo(
      () =>
        (data?.nodes ?? [])
          .filter((n) => n.readiness === "ready")
          .map((n) => n.short_ref),
      [data],
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

  const hops = Math.max(0, model.mainPath.length - 1);
  const ready = (data?.nodes ?? []).filter((n) => n.readiness === "ready").length;

  return (
    <div className="wflow">
      <div className="fhead">
        <div className="ftitle">
          <Waypoints className="i" aria-hidden />
          {data?.name ?? "all flows"}
          <span className="sub">
            {data
              ? `${data.nodes.length} nodes · ${data.stages} stages · ${hops}-hop critical path · ${ready} ready`
              : ""}
          </span>
        </div>
        <div className="seg" role="group" aria-label="Flow">
          {flows.data?.map((f) => (
            <button
              key={f.name}
              type="button"
              className={cn("actf", opts.flow === f.name && "on")}
              aria-pressed={opts.flow === f.name}
              onClick={() => setFlow(f.name)}
              title={`Show the ${f.name} flow`}
            >
              {f.name}
              <span className="cnt">{f.issues}</span>
            </button>
          ))}
          <button
            type="button"
            className={cn("actf", opts.flow === ALL_FLOWS && "on")}
            aria-pressed={opts.flow === ALL_FLOWS}
            onClick={() => setFlow(ALL_FLOWS)}
            title="Every flow's members together"
          >
            all
          </button>
        </div>
      </div>
      {data?.shape_problem ? <ShapeProblem problem={data.shape_problem} /> : null}
      {data ? <Canvas model={model} onOpen={onOpen} /> : null}
      <FlowLegend legend={model.legend} />
    </div>
  );
}

/** The reading in the address bar. A link to this screen should reopen what
 *  the sender was looking at — which flow, which node, the traced route, the
 *  isolated colour — so it can be pasted into a thread and answered. The URL
 *  is hydrated once on arrival and mirrored afterwards; mirroring replaces
 *  the history entry rather than pushing, so Back still means "the previous
 *  screen", not "the previous click on this one". */
function useFlowUrl(
  opts: ReturnType<typeof useViewOptions>["flow"],
  apply: ReturnType<typeof useViewOptions>["applyFlowReading"],
) {
  const route = useRoute();
  const hydrated = useRef(false);

  useEffect(() => {
    if (hydrated.current || route.name !== "flow") return;
    hydrated.current = true;
    const [from = null, to = null] = (route.r ?? "").split("~");
    const paint = FLOW_PAINTS.find((p) => p === route.paint) ?? null;
    const isolate = (route.k ?? "").split(",").filter((k) => k.length > 0);
    // Only what the link actually carries is applied; the rest keeps
    // whatever this session was already showing.
    apply({
      ...(route.f !== null && route.f !== undefined ? { flow: route.f } : {}),
      ...(route.n ? { selected: route.n, center: Date.now() } : {}),
      ...(from ? { from } : {}),
      ...(to ? { to } : {}),
      ...(isolate.length > 0 ? { isolate: new Set(isolate) } : {}),
      ...(paint !== null ? { paint } : {}),
    });
  }, [route, apply]);

  const openIssue = route.name === "flow" ? (route.issue ?? null) : null;
  useEffect(() => {
    if (!hydrated.current || route.name !== "flow") return;
    replaceRoute({
      name: "flow",
      issue: openIssue,
      f: opts.flow === ALL_FLOWS ? null : opts.flow,
      n: opts.selected,
      r: opts.from !== null && opts.to !== null ? `${opts.from}~${opts.to}` : null,
      k: opts.isolate.size > 0 ? [...opts.isolate].join(",") : null,
      paint: opts.paint === "state" ? null : opts.paint,
    });
  }, [opts, route.name, openIssue]);
}

/** A fence that is there and unreadable. The diagram is still drawn — a
 *  typo in one document must never cost someone their diagram — so this
 *  says what is wrong and exactly where, above it. */
function ShapeProblem({ problem }: { problem: FlowShapeProblemDto }) {
  return (
    <div className="fproblem" role="status">
      <AlertTriangle className="i" aria-hidden />
      <span className="t">
        <strong>
          {problem.path}:{problem.line}
        </strong>
        {problem.detail} — the phases in that fence are being ignored; the columns below are
        computed from <span className="mono">blocked_by</span> instead.
      </span>
    </div>
  );
}

// -- the canvas -------------------------------------------------------------

interface View {
  s: number;
  x: number;
  y: number;
}

function Canvas({
  model,
  onOpen,
}: {
  model: ReturnType<typeof useFlowModel>;
  onOpen: (id: string) => void;
}) {
  const { data, layout } = model;
  const { flow: opts, selectFlowNode, clearFlowLens } = useViewOptions();
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const [view, setView] = useState<View>({ s: 1, x: 0, y: 0 });
  const [finding, setFinding] = useState(false);
  const [guiding, setGuiding] = useState(false);
  const [present, setPresent] = useState(false);
  // Intent trace: a one-hop preview under the pointer, before anyone commits
  // to a durable selection. It only speaks when nothing else is being read.
  const [hover, setHover] = useState<string | null>(null);
  const detail = detailAt(view.s);

  useLayoutEffect(() => {
    const element = host.current;
    if (element === null) return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setSize({ w: entry.contentRect.width, h: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const fitView = useCallback((): View => {
    if (layout === null || size.w === 0) return { s: 1, x: 0, y: 0 };
    // Never blow a small diagram up past life size: three nodes should not
    // fill a 27-inch screen just because they can.
    const s = Math.max(MIN_SCALE, Math.min(1, (size.w - 24) / layout.width, (size.h - 24) / layout.height));
    return { s, x: (size.w - layout.width * s) / 2, y: (size.h - layout.height * s) / 2 };
  }, [layout, size]);

  // Fit whenever the diagram or the viewport changes shape. The board's own
  // dimensions are the dependency, not its identity: a live refresh that
  // changes nothing about the layout must not throw the reader's view away.
  const shape = layout === null ? "" : `${layout.width}x${layout.height}`;
  useEffect(() => {
    if (shape === "" || size.w === 0) return;
    setView(fitView());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shape, size.w, size.h]);

  const centerOn = useCallback(
    (id: string) => {
      const at = layout?.node.get(id);
      if (at === undefined || size.w === 0) return;
      setView((v) => ({
        ...v,
        x: size.w / 2 - (at.x + M.nodeW / 2) * v.s,
        y: size.h / 2 - (at.y + M.nodeH / 2) * v.s,
      }));
    },
    [layout, size],
  );

  // Presentation stage: the whole app goes fullscreen and every piece of
  // chrome steps aside, so the diagram is the only thing on the screen.
  const togglePresent = useCallback(() => {
    const root = document.documentElement;
    if (!document.fullscreenElement) {
      void root.requestFullscreen?.().catch(() => undefined);
    } else {
      void document.exitFullscreen?.().catch(() => undefined);
    }
  }, []);
  useEffect(() => {
    // Leaving fullscreen with the browser's own Escape must put the chrome
    // back, so the flag follows the browser rather than leading it.
    const sync = () => {
      const on = Boolean(document.fullscreenElement);
      setPresent(on);
      document.documentElement.toggleAttribute("data-present", on);
    };
    document.addEventListener("fullscreenchange", sync);
    return () => {
      document.removeEventListener("fullscreenchange", sync);
      document.documentElement.removeAttribute("data-present");
    };
  }, []);

  // A selection made anywhere that asked to be centered — the finder, the
  // sidebar, the critical-path stepper — brings the node into view.
  useEffect(() => {
    if (opts.center > 0 && opts.selected !== null) centerOn(opts.selected);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [opts.center]);

  const zoomBy = useCallback(
    (factor: number, at?: { x: number; y: number }) => {
      setView((v) => {
        const s = Math.max(MIN_SCALE, Math.min(MAX_SCALE, v.s * factor));
        const anchor = at ?? { x: size.w / 2, y: size.h / 2 };
        // Keep whatever sits under the anchor exactly where it is.
        return {
          s,
          x: anchor.x - ((anchor.x - v.x) * s) / v.s,
          y: anchor.y - ((anchor.y - v.y) * s) / v.s,
        };
      });
    },
    [size],
  );

  // Wheel has to be a non-passive listener or the page scrolls with it.
  useEffect(() => {
    const element = host.current;
    if (element === null) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      if (event.ctrlKey || event.metaKey) {
        const box = element.getBoundingClientRect();
        zoomBy(Math.exp(-event.deltaY / 240), {
          x: event.clientX - box.left,
          y: event.clientY - box.top,
        });
      } else {
        setView((v) => ({ ...v, x: v.x - event.deltaX, y: v.y - event.deltaY }));
      }
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [zoomBy]);

  // Drag anywhere that is not a node pans the diagram.
  const drag = useRef<{ id: number; x: number; y: number } | null>(null);
  const onPointerDown = (event: React.PointerEvent) => {
    if ((event.target as Element).closest(".fnode") !== null) return;
    drag.current = { id: event.pointerId, x: event.clientX, y: event.clientY };
    (event.currentTarget as Element).setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: React.PointerEvent) => {
    const at = drag.current;
    if (at === null || at.id !== event.pointerId) return;
    const dx = event.clientX - at.x;
    const dy = event.clientY - at.y;
    drag.current = { ...at, x: event.clientX, y: event.clientY };
    setView((v) => ({ ...v, x: v.x + dx, y: v.y + dy }));
  };
  const endDrag = (event: React.PointerEvent) => {
    if (drag.current?.id === event.pointerId) drag.current = null;
  };

  // Keys that belong to the diagram. The shell owns `/`, `c` and J/K; these
  // are the ones it leaves free.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      const typing =
        target !== null &&
        (target.isContentEditable ||
          ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
      if (event.key === "Escape") {
        // Unwind in the order things were opened, innermost first, so one
        // Escape never throws away more than the reader expects.
        if (finding) {
          event.preventDefault();
          setFinding(false);
        } else if (guiding) {
          event.preventDefault();
          setGuiding(false);
        } else if (!typing && (opts.selected !== null || opts.from !== null || opts.isolate.size > 0)) {
          event.preventDefault();
          clearFlowLens();
        }
        return;
      }
      if (typing) return;
      // Two shifted keys, handled before the plain-key guard: `?` is the
      // guide, `F` is the presentation stage. `f` stays find, because that
      // is the reflex, so the stage takes its shifted twin.
      if (event.key === "?") {
        event.preventDefault();
        setGuiding((open) => !open);
        return;
      }
      if (event.key === "F") {
        event.preventDefault();
        togglePresent();
        return;
      }
      if (event.shiftKey) return;
      if (event.key === "f") {
        event.preventDefault();
        setFinding(true);
      } else if (event.key === "0") {
        event.preventDefault();
        setView(fitView());
      } else if (event.key === "=" || event.key === "+") {
        event.preventDefault();
        zoomBy(1.2);
      } else if (event.key === "-") {
        event.preventDefault();
        zoomBy(1 / 1.2);
      } else if (event.key === "[") {
        event.preventDefault();
        model.stepPath(-1);
      } else if (event.key === "]") {
        event.preventDefault();
        model.stepPath(1);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [
    finding,
    guiding,
    fitView,
    zoomBy,
    model,
    opts.selected,
    opts.from,
    opts.isolate.size,
    clearFlowLens,
    togglePresent,
  ]);

  if (data === null || layout === null) return null;

  // The hover preview only speaks when the reader has not asked for
  // something louder; a lens or a probe already has the floor.
  const previewing =
    hover !== null && model.lit === null && model.graph !== null ? hover : null;
  const preview =
    previewing === null || model.graph === null
      ? null
      : new Set([
          previewing,
          ...(model.graph.in.get(previewing) ?? []),
          ...(model.graph.out.get(previewing) ?? []),
        ]);

  return (
    <div className={cn("fcanvas", present && "presenting")} ref={host}>
      <svg
        className="fsurface"
        width={size.w || 1}
        height={size.h || 1}
        role="img"
        aria-label={`Flow diagram: ${data.name ?? "all flows"}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <defs>
          <pattern id="wf-dots" width="24" height="24" patternUnits="userSpaceOnUse">
            <circle cx="1" cy="1" r="1" className="wf-c-dot" />
          </pattern>
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
        <g transform={`translate(${view.x},${view.y}) scale(${view.s})`}>
          <Ground board={data} layout={layout} />
          {data.edges.map((e) => (
            <Edge
              key={`${e.from}>${e.to}`}
              e={e}
              layout={layout}
              main={model.mainEdges.has(`${e.from}>${e.to}`)}
              lit={
                model.litEdges !== null
                  ? model.litEdges.has(`${e.from}>${e.to}`)
                  : preview === null || (preview.has(e.from) && preview.has(e.to))
              }
            />
          ))}
          {data.nodes.map((n) => (
            <Node
              key={n.id}
              n={n}
              layout={layout}
              lit={model.lit !== null ? model.lit.has(n.id) : preview === null || preview.has(n.id)}
              role={
                n.id === opts.selected
                  ? "selected"
                  : n.id === opts.from || n.id === opts.to
                    ? "probe"
                    : model.upstream.has(n.id)
                      ? "upstream"
                      : model.downstream.has(n.id)
                        ? "downstream"
                        : null
              }
              paint={opts.paint}
              slot={paintSlot(model.legend, paintKeyOf(n, opts.paint))}
              detail={detail}
              onSelect={() => selectFlowNode(n.id === opts.selected ? null : n.id)}
              onOpen={() => onOpen(n.short_ref)}
              onHover={setHover}
            />
          ))}
        </g>
      </svg>

      {finding ? (
        <Finder
          nodes={data.nodes}
          onClose={() => setFinding(false)}
          onPick={(id) => {
            selectFlowNode(id, { center: true });
            setFinding(false);
          }}
        />
      ) : null}

      {guiding ? <Guide onClose={() => setGuiding(false)} board={data} /> : null}

      <Story model={model} />

      <Minimap
        data={data}
        layout={layout}
        view={view}
        size={size}
        lit={model.lit}
        paint={opts.paint}
        legend={model.legend}
        onGo={centerOn}
      />

      <div className="fnav" role="group" aria-label="Diagram view">
        <button type="button" onClick={() => setFinding(true)} title="Find a node (f)">
          <Search className="i" aria-hidden />
        </button>
        <button
          type="button"
          onClick={() => setGuiding((open) => !open)}
          title="What this screen can do (?)"
          aria-pressed={guiding}
        >
          <HelpCircle className="i" aria-hidden />
        </button>
        <button
          type="button"
          onClick={togglePresent}
          title={present ? "Leave the presentation stage (F)" : "Presentation stage (F)"}
          aria-pressed={present}
        >
          {present ? <Minimize2 className="i" aria-hidden /> : <Maximize2 className="i" aria-hidden />}
        </button>
        <button type="button" onClick={() => setView(fitView())} title="Fit the whole diagram (0)">
          <Crosshair className="i" aria-hidden />
        </button>
        {model.selected !== null ? (
          <button
            type="button"
            onClick={() => centerOn(model.selected!.id)}
            title="Center the selection"
          >
            <Waypoints className="i" aria-hidden />
          </button>
        ) : null}
        <button type="button" onClick={() => zoomBy(1 / 1.2)} title="Zoom out (-)">
          <Minus className="i" aria-hidden />
        </button>
        <span className="fpct">{Math.round(view.s * 100)}%</span>
        <button type="button" onClick={() => zoomBy(1.2)} title="Zoom in (+)">
          <Plus className="i" aria-hidden />
        </button>
      </div>
    </div>
  );
}

/** The dotted ground, the lane bands and the stage rails — everything the
 *  nodes sit on. Stage rails run the full height so a column reads as one
 *  thing across every lane, which the per-lane chips never did. */
function Ground({ board, layout }: { board: FlowBoardDto; layout: FlowLayout }) {
  const stages = board.stages;
  const frames = groupFrames(board, layout);
  return (
    <>
      <rect
        x={-M.padX}
        y={-M.padY}
        width={layout.width + M.padX * 2}
        height={layout.height + M.padY * 2}
        fill="url(#wf-dots)"
      />
      {Array.from({ length: stages }, (_, s) => {
        const x = layout.stageX(s);
        return (
          <g key={s}>
            <rect
              x={x - 12}
              y={M.stageHead - 10}
              width={M.nodeW + 24}
              height={layout.height - M.stageHead + 4}
              rx={12}
              className={cn("wf-c-rail", s % 2 === 1 && "alt")}
            />
            <line x1={x} y1={M.stageHead - 16} x2={x + M.nodeW} y2={M.stageHead - 16} className="wf-a-default" strokeWidth="1.1" />
            <rect
              x={x + M.nodeW / 2 - captionWidth(columnLabel(board, s)) / 2}
              y={M.stageHead - 24}
              width={captionWidth(columnLabel(board, s))}
              height={16}
              rx={4}
              className="wf-c-mask"
            />
            <text
              x={x + M.nodeW / 2}
              y={M.stageHead - 13}
              className="wf-t-muted"
              fontSize="9"
              fontWeight="600"
              textAnchor="middle"
              fontFamily="var(--mono)"
            >
              {columnLabel(board, s)}
            </text>
          </g>
        );
      })}
      {/* group frames sit above the lane band and below the nodes */}
      {frames.map((frame) => (
        <g key={frame.id}>
          <rect
            x={frame.x}
            y={frame.y}
            width={frame.w}
            height={frame.h}
            rx={8}
            className="wf-c-group"
          />
          <rect
            x={frame.x + 8}
            y={frame.y - 7}
            width={captionWidth(frame.label)}
            height={14}
            rx={3}
            className="wf-c-mask"
          />
          <text
            x={frame.x + 14}
            y={frame.y + 3}
            className="wf-t-dim"
            fontSize="9"
            fontWeight="600"
            fontFamily="var(--mono)"
          >
            {frame.label}
          </text>
        </g>
      ))}
      {layout.lanes.map((lane, i) => (
        <g key={lane.key}>
          <rect
            x={-M.padX / 2}
            y={lane.top}
            width={layout.width + M.padX}
            height={lane.height}
            rx={10}
            className="wf-c-lane"
          />
          <rect x={M.padX - 6} y={lane.top + 6} width={laneChipWidth(lane.label)} height={16} rx={4} className="wf-c-mask" />
          <text
            x={M.padX}
            y={lane.top + 18}
            className="wf-t-dim"
            fontSize="11"
            fontWeight="600"
            fontFamily="var(--mono)"
          >
            {String(i + 1).padStart(2, "0")} / {lane.label}
          </text>
        </g>
      ))}
    </>
  );
}

function laneChipWidth(label: string): number {
  return 12 + (label.length + 5) * 6.6;
}

/** Mono glyphs are a fixed width, so a caption chip can be sized from the
 *  character count rather than measured. */
function captionWidth(label: string): number {
  return Math.max(36, label.length * 5.6 + 14);
}

const MARKERS = {
  default: "wf-m-default",
  emph: "wf-m-emph",
  dashed: "wf-m-dashed",
  broken: "wf-m-broken",
  feed: "wf-m-feed",
} as const;

function Edge({
  e,
  layout,
  main,
  lit,
}: {
  e: FlowEdgeDto;
  layout: FlowLayout;
  main: boolean;
  lit: boolean;
}) {
  const a = layout.node.get(e.from);
  const b = layout.node.get(e.to);
  if (a === undefined || b === undefined) return null;
  // Variant grammar: the critical path is emphasized; satisfied mutes to a
  // dashed neutral; broken is dashed rose; anything else is the default.
  // A non-gating arrow must never be mistaken for a dependency, so it takes
  // its own muted, dotted line and is never emphasised as critical path.
  const [cls, marker] = !e.gating
    ? ["wf-a-feed", "feed"]
    : main
      ? ["wf-a-emph", "emph"]
      : e.disposition === "broken"
        ? ["wf-a-broken", "broken"]
        : e.disposition === "satisfied"
          ? ["wf-a-dashed", "dashed"]
          : ["wf-a-default", "default"];
  const d = edgePath(a, b, layout.port.get(`${e.from}>${e.to}`));
  // A caption sits at the midpoint of the connector's span, over its own
  // mask so the line does not read through the words.
  const mid = { x: (a.x + M.nodeW + b.x) / 2, y: (a.y + b.y + M.nodeH) / 2 };
  return (
    <g className={cn("fedge", !lit && "dim")}>
      <path
        d={d}
        className={cn(cls, e.backward && "backward")}
        strokeWidth={e.gating && main ? 1.8 : 1.4}
        markerEnd={`url(#wf-arrow-${marker})`}
      />
      {!e.gating ? (
        <title>
          {`${e.label ?? "feeds"} — this arrow carries a result, it does not gate: readiness, stages and the critical path ignore it`}
        </title>
      ) : null}
      {e.backward ? (
        <title>
          {`this blocker sits in a later phase than the issue it blocks — the dependency and the authored order disagree`}
        </title>
      ) : null}
      {e.label !== null && e.label !== "" ? (
        <g>
          <rect
            x={mid.x - captionWidth(e.label) / 2}
            y={mid.y - 8}
            width={captionWidth(e.label)}
            height={15}
            rx={3}
            className="wf-c-mask"
          />
          <text
            x={mid.x}
            y={mid.y + 3}
            className="wf-t-muted"
            fontSize="9"
            textAnchor="middle"
            fontFamily="var(--mono)"
          >
            {e.label}
          </text>
        </g>
      ) : null}
    </g>
  );
}

function Node({
  n,
  layout,
  lit,
  role,
  paint,
  slot,
  detail,
  onSelect,
  onOpen,
  onHover,
}: {
  n: FlowNodeDto;
  layout: FlowLayout;
  lit: boolean;
  role: "selected" | "probe" | "upstream" | "downstream" | null;
  paint: FlowPaint;
  slot: number;
  detail: Detail;
  onSelect: () => void;
  onOpen: () => void;
  onHover: (id: string | null) => void;
}) {
  const pos = layout.node.get(n.id);
  if (pos === undefined) return null;
  const sem = semanticOf(n);
  const handle = n.number !== null ? `#${n.number}` : n.short_ref;
  // Zoomed out, a wrapped two-line title is a grey smear; one line reads.
  const title = wrapLabel(n.title, TITLE_CHARS, detail === "map" ? 1 : TITLE_LINES);
  const titleY = (i: number) =>
    detail === "map" ? M.nodeH / 2 + 4 : title.length === 1 ? 34 : 28 + i * 14;
  const claim = n.claim ? `@${n.claim.claimed_by}${n.claim.stale ? " · stale" : ""}` : n.status_label;
  const held = n.outside_blockers.filter((b) => !b.satisfied).length;
  // Two phase labels is what a merge of two branches produces. The node
  // still draws once, in the earliest — and says that it is in two minds.
  const split = n.phases.length > 1 ? ` · in ${n.phases.length} phases` : "";
  const sub = `${handle} · ${claim}${held > 0 ? ` · held by ${held} outside` : ""}${split}`;
  return (
    <g
      transform={`translate(${pos.x},${pos.y})`}
      className={cn("fnode", !lit && "dim", role !== null && role, n.phases.length > 1 && "split")}
      onClick={onSelect}
      onDoubleClick={onOpen}
      onPointerEnter={() => onHover(n.id)}
      onPointerLeave={() => onHover(null)}
      role="button"
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          onOpen();
        } else if (event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      aria-label={`${handle} ${n.title}`}
    >
      <title>{`${handle} · ${n.title} · ${n.status_label}${n.claim ? ` · claim ${n.claim.claimed_by}` : ""}\nclick to trace · double-click or Enter to open`}</title>
      {/* the mask first: opaque, so edges underneath are hidden */}
      <rect width={M.nodeW} height={M.nodeH} rx={M.radius} className="wf-c-mask" />
      <rect
        width={M.nodeW}
        height={M.nodeH}
        rx={M.radius}
        // State keeps its designed colours; every other dimension paints
        // from the categorical palette, and the sigil still carries state
        // so nothing is lost by switching lens.
        className={cn("wf-node-frame", paint === "state" ? `wf-c-${sem}` : `wf-p-${slot}`)}
        strokeWidth="1.5"
      />
      <Sigil sem={sem} x={9} y={9} />
      {n.commits > 0 && detail !== "map" ? (
        <text
          x={9}
          y={M.nodeH - 9}
          className="wf-t-dim"
          fontSize="8"
          fontWeight="600"
          fontFamily="var(--mono)"
        >
          {n.commits === 1 ? "1 commit" : `${n.commits} commits`}
        </text>
      ) : null}
      {n.priority !== null && detail === "full" ? (
        <text x={M.nodeW - 9} y={17} className="wf-t-dim" fontSize="8.5" textAnchor="end" fontFamily="var(--mono)">
          {n.priority}
        </text>
      ) : null}
      {title.map((line, i) => (
        <text
          key={i}
          x={M.nodeW / 2}
          y={titleY(i)}
          className="wf-t-primary"
          fontSize="11.5"
          fontWeight="600"
          textAnchor="middle"
          fontFamily="var(--mono)"
        >
          {line}
        </text>
      ))}
      {detail !== "map" ? (
        <text
          x={M.nodeW / 2}
          y={M.nodeH - 13}
          className={cn(sem === "done" ? "wf-t-dim" : "wf-t-muted")}
          fontSize="9"
          textAnchor="middle"
          fontFamily="var(--mono)"
        >
          {sub}
        </text>
      ) : null}
    </g>
  );
}

/** The per-semantic mini glyph, archify's semantic sigils in miniature:
 *  ready = a check-forward tick, doing = two moving bars, blocked = an
 *  octagon stop hint, done = a settled square. */
function Sigil({ sem, x, y }: { sem: Semantic; x: number; y: number }) {
  const stroke = `var(--wf-${sem}-stroke)`;
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

// -- find a node ------------------------------------------------------------

function Finder({
  nodes,
  onPick,
  onClose,
}: {
  nodes: readonly FlowNodeDto[];
  onPick: (id: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const hits = useMemo(() => searchNodes(nodes, query).slice(0, 8), [nodes, query]);
  const at = Math.min(cursor, Math.max(0, hits.length - 1));

  return (
    <div className="ffind">
      <div className="ffind-row">
        <Search className="i" aria-hidden />
        <input
          autoFocus
          className="ffind-input"
          placeholder="Find a node by title or #number…"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setCursor(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setCursor((c) => Math.min(hits.length - 1, c + 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setCursor((c) => Math.max(0, c - 1));
            } else if (event.key === "Enter") {
              event.preventDefault();
              const hit = hits[at];
              if (hit !== undefined) onPick(hit.id);
            }
          }}
        />
        <button type="button" onClick={onClose} title="Close (Esc)">
          <X className="i" aria-hidden />
        </button>
      </div>
      {hits.length > 0 ? (
        <ul className="ffind-list">
          {hits.map((hit, i) => (
            <li key={hit.id}>
              <button
                type="button"
                className={cn("ffind-hit", i === at && "on")}
                onMouseEnter={() => setCursor(i)}
                onClick={() => onPick(hit.id)}
              >
                <i className={cn("nd", semanticOf(hit))} aria-hidden />
                <span className="t">{hit.title}</span>
                <span className="r">{hit.number !== null ? `#${hit.number}` : hit.short_ref}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : query.trim() !== "" ? (
        <p className="ffind-empty">Nothing on this board matches.</p>
      ) : null}
    </div>
  );
}

// -- the guided reading -------------------------------------------------------

const BEAT_MS = 3200;

/** The story bar: the chapters this board tells, stepped or played. Archify
 *  writes its chapters by hand; here they are derived, so they cannot go
 *  stale and a flow that nobody has shaped still has something to say. */
function Story({ model }: { model: ReturnType<typeof useFlowModel> }) {
  const { flow: opts, setFlowChapter } = useViewOptions();
  const { chapters } = model;
  const [playing, setPlaying] = useState(false);
  const at = opts.chapter;

  const step = useCallback(
    (delta: -1 | 1) => {
      if (chapters.length === 0) return;
      const next = (at === null ? (delta === 1 ? 0 : chapters.length - 1) : at + delta);
      if (next < 0 || next >= chapters.length) {
        setPlaying(false);
        setFlowChapter(null);
        return;
      }
      setFlowChapter(next);
    },
    [at, chapters.length, setFlowChapter],
  );

  useEffect(() => {
    if (!playing) return;
    const timer = window.setTimeout(() => step(1), BEAT_MS);
    return () => window.clearTimeout(timer);
  }, [playing, at, step]);

  if (chapters.length === 0) return null;
  const current = at === null ? null : (chapters[at] ?? null);

  return (
    <div className="fstory">
      <button
        type="button"
        onClick={() => step(-1)}
        title="Previous beat"
        disabled={at === null}
      >
        <ChevronLeft className="i" aria-hidden />
      </button>
      <button
        type="button"
        className={cn(playing && "on")}
        onClick={() => {
          if (at === null) setFlowChapter(0);
          setPlaying((on) => !on);
        }}
        aria-pressed={playing}
        title={playing ? "Pause the story" : "Play the story"}
      >
        {playing ? <Pause className="i" aria-hidden /> : <Play className="i" aria-hidden />}
      </button>
      <button type="button" onClick={() => step(1)} title="Next beat">
        <ChevronRight className="i" aria-hidden />
      </button>
      <span className="fstory-copy">
        {current === null ? (
          <>
            <strong>{chapters.length} beats</strong>
            {model.data !== null && model.data.phases.length > 0
              ? "the phases this flow declares"
              : "the critical path, step by step"}
          </>
        ) : (
          <>
            <strong>{current.label}</strong>
            {current.note}
          </>
        )}
      </span>
      <span className="fstory-at mono">
        {at === null ? "—" : at + 1}/{chapters.length}
      </span>
      {at !== null ? (
        <button
          type="button"
          onClick={() => {
            setPlaying(false);
            setFlowChapter(null);
          }}
          title="Show the whole diagram again"
        >
          <X className="i" aria-hidden />
        </button>
      ) : null}
    </div>
  );
}

// -- the guide ---------------------------------------------------------------

const GUIDE: Array<[string, string, string]> = [
  ["f", "Find a node", "by title or #number, then jump to it"],
  ["click", "Trace a node", "lights everything it waits on and everything waiting on it"],
  ["From / To", "Trace a route", "pick two nodes in the sidebar to see the chain between them"],
  ["[ ]", "Walk the critical path", "step through what decides when this flow lands"],
  ["legend", "Isolate a colour", "click a swatch to dim everything else"],
  ["0 − +", "Move the view", "fit, zoom, or drag anywhere to pan"],
  ["F", "Presentation stage", "fullscreen, chrome hidden"],
  ["Esc", "Step back out", "closes the innermost thing first"],
];

/** What this screen can do, in one place — archify's diagram guide. Facts
 *  about the board on screen, not a manual. */
function Guide({ board, onClose }: { board: FlowBoardDto; onClose: () => void }) {
  const hops = Math.max(0, board.main_path.length - 1);
  return (
    <div className="fguide" role="dialog" aria-modal="false" aria-label="What this screen can do">
      <div className="fguide-head">
        <span className="eyebrow">Reading this diagram</span>
        <button type="button" onClick={onClose} title="Close (Esc or ?)">
          <X className="i" aria-hidden />
        </button>
      </div>
      <p className="fguide-facts mono">
        {board.nodes.length} nodes · {board.edges.length} arrows · {board.stages} stages ·{" "}
        {hops}-hop critical path
      </p>
      <ul className="fguide-list">
        {GUIDE.map(([key, title, detail]) => (
          <li key={title}>
            <kbd>{key}</kbd>
            <span className="t">
              <strong>{title}</strong>
              {detail}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

// -- minimap ----------------------------------------------------------------

function Minimap({
  data,
  layout,
  view,
  size,
  lit,
  paint,
  legend,
  onGo,
}: {
  data: FlowBoardDto;
  layout: FlowLayout;
  view: View;
  size: { w: number; h: number };
  lit: ReadonlySet<string> | null;
  paint: FlowPaint;
  legend: Array<{ key: string; count: number }>;
  onGo: (id: string) => void;
}) {
  const box = { w: 172, h: 108 };
  const scale = Math.min(box.w / layout.width, box.h / layout.height);
  const w = layout.width * scale;
  const h = layout.height * scale;
  // Where the viewport currently sits over the diagram, in map units.
  const port = {
    x: (-view.x / view.s) * scale,
    y: (-view.y / view.s) * scale,
    w: (size.w / view.s) * scale,
    h: (size.h / view.s) * scale,
  };
  // A map that shows the whole diagram at once is not telling the reader
  // anything they cannot already see.
  if (port.w >= w && port.h >= h) return null;

  const jump = (event: React.MouseEvent<SVGSVGElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    const x = (event.clientX - rect.left) / scale;
    const y = (event.clientY - rect.top) / scale;
    // Jump to whichever node the click lands nearest.
    let best: { id: string; d: number } | null = null;
    for (const n of data.nodes) {
      const at = layout.node.get(n.id);
      if (at === undefined) continue;
      const dx = at.x + M.nodeW / 2 - x;
      const dy = at.y + M.nodeH / 2 - y;
      const d = dx * dx + dy * dy;
      if (best === null || d < best.d) best = { id: n.id, d };
    }
    if (best !== null) onGo(best.id);
  };

  return (
    <div className="fmap" aria-hidden>
      <svg width={w} height={h} onClick={jump}>
        {data.nodes.map((n) => {
          const at = layout.node.get(n.id);
          if (at === undefined) return null;
          return (
            <rect
              key={n.id}
              x={at.x * scale}
              y={at.y * scale}
              width={Math.max(2, M.nodeW * scale)}
              height={Math.max(2, M.nodeH * scale)}
              rx={1}
              className={cn(
                paint === "state"
                  ? `wf-c-${semanticOf(n)}`
                  : `wf-p-${paintSlot(legend, paintKeyOf(n, paint))}`,
                lit !== null && !lit.has(n.id) && "dim",
              )}
            />
          );
        })}
        <rect
          x={port.x}
          y={port.y}
          width={Math.min(w, port.w)}
          height={Math.min(h, port.h)}
          className="fmap-port"
        />
      </svg>
    </div>
  );
}

// -- legend -----------------------------------------------------------------

const STATE_LABELS: Record<string, string> = {
  ready: "ready",
  doing: "in flight",
  blocked: "blocked",
  done: "done",
};

const PAINT_LABELS: Record<FlowPaint, string> = {
  state: "state",
  lane: "lane",
  type: "type",
  priority: "priority",
};

/** The legend is a filter and a lens at once, the way archify's is: it says
 *  what the colours currently mean, lets that be changed, and clicking a
 *  swatch isolates it. Every dimension it offers is already in the data. */
function FlowLegend({ legend }: { legend: Array<{ key: string; count: number }> }) {
  const { flow: opts, toggleFlowIsolate, setFlowPaint } = useViewOptions();
  return (
    <footer className="wfoot">
      <span className="legend">
        <span className="lens" role="group" aria-label="Colour by">
          <Palette className="i" aria-hidden />
          {FLOW_PAINTS.map((paint) => (
            <button
              key={paint}
              type="button"
              className={cn("lensb", opts.paint === paint && "on")}
              aria-pressed={opts.paint === paint}
              onClick={() => setFlowPaint(paint)}
              title={`Colour the nodes by ${PAINT_LABELS[paint]}`}
            >
              {PAINT_LABELS[paint]}
            </button>
          ))}
        </span>
        {legend.map((row, i) => (
          <button
            key={row.key}
            type="button"
            className={cn("ndb", opts.isolate.has(row.key) && "on")}
            aria-pressed={opts.isolate.has(row.key)}
            onClick={() => toggleFlowIsolate(row.key)}
            title={`Show only ${row.key}`}
          >
            <i
              className={cn("nd", opts.paint === "state" ? row.key : `p${i % 8}`)}
              aria-hidden
            />
            {opts.paint === "state" ? (STATE_LABELS[row.key] ?? row.key) : row.key}
            <span className="ct">{row.count}</span>
          </button>
        ))}
        · arrows
        <i className="sw" /> in flight
        <i className="sw dashed" /> through the gate
        <i className="sw broken" /> cancelled/gone
        <i className="sw emph" /> critical path
        · <span className="mono">?</span> for everything this screen can do
      </span>
    </footer>
  );
}
