// One folder of a code root, drawn: its subfolders as boxes and its files as
// pills, with an arrow for the heaviest imports between them. A small folder
// is laid out by force; a big one in layers, importers left of what they
// import, one row slot per unit so no two labels meet. Either way the layout
// is computed once per folder and left alone — a map that moves under the
// pointer is not a map. Hovering a unit shows every import it takes part in.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  boundsOf,
  boxExit,
  capUnits,
  charsFor,
  connector,
  EDGE_CAP,
  edgeKey,
  edgeWidth,
  fitViewBox,
  fullyGenerated,
  LAYERED_OVER,
  lastSegment,
  layeredLayout,
  layoutUnits,
  middleTruncate,
  topEdges,
  type PlacedUnit,
} from "../../lib/codemap";
import type { CodeOverviewDto, CodeUnitDto, CodeUnitEdgeDto } from "../../lib/types";

export function FolderGraph({
  overview,
  focus,
  onFolder,
  onFile,
}: {
  overview: CodeOverviewDto;
  /** The file in focus, outlined when it is one of this folder's units. */
  focus: string | null;
  onFolder: (path: string) => void;
  onFile: (path: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{ path: string; x: number; y: number } | null>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });

  useEffect(() => {
    const element = host.current;
    if (!element || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setSize({ w: entry.contentRect.width, h: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const drawn = useMemo(() => {
    const cut = capUnits(overview.units, overview.edges);
    const layered = cut.units.length > LAYERED_OVER;
    const placed = layered ? layeredLayout(cut.units, cut.edges) : layoutUnits(cut.units, cut.edges);
    return {
      ...cut,
      layered,
      placed,
      top: topEdges(cut.edges),
      at: new Map(placed.map((p) => [p.unit.path, p])),
      box: boundsOf(placed),
    };
  }, [overview]);

  const hovered = hover?.path ?? null;
  const near = useMemo(() => {
    if (hovered === null) return null;
    const set = new Set([hovered]);
    for (const e of drawn.edges) {
      if (e.from === hovered) set.add(e.to);
      if (e.to === hovered) set.add(e.from);
    }
    return set;
  }, [drawn.edges, hovered]);

  // The heaviest imports always; every import of the hovered unit on top.
  const shown = useMemo(
    () =>
      drawn.edges.filter((e) => drawn.top.has(edgeKey(e)) || (hovered !== null && (e.from === hovered || e.to === hovered))),
    [drawn.edges, drawn.top, hovered],
  );
  // Two units importing each other get two lines; bending both the same way
  // off the straight line keeps them from drawing on top of each other.
  const pairs = useMemo(() => new Set(drawn.edges.map(edgeKey)), [drawn.edges]);

  const tip = hover ? drawn.at.get(hover.path) : undefined;
  // A layered folder draws at its own size and scrolls; a small one is fitted
  // to the screen (without being blown up).
  const view = drawn.layered ? drawn.box : fitViewBox(drawn.box, size.w, size.h);

  const nodes = drawn.placed.map((p) => (
    <Unit
      key={p.unit.path}
      placed={p}
      focused={p.unit.path === focus}
      dim={near !== null && !near.has(p.unit.path)}
      lit={hovered === p.unit.path}
      onEnter={(ev) => {
        const r = host.current?.getBoundingClientRect();
        setHover({ path: p.unit.path, x: ev.clientX - (r?.left ?? 0), y: ev.clientY - (r?.top ?? 0) });
      }}
      onLeave={() => setHover((h) => (h?.path === p.unit.path ? null : h))}
      onOpen={() => (p.unit.folder ? onFolder(p.unit.path) : onFile(p.unit.path))}
    />
  ));

  const svg = (
    <svg
      className={drawn.layered ? "mx-auto block select-none" : "min-h-0 w-full flex-1 select-none"}
      viewBox={`${view.x} ${view.y} ${view.w} ${view.h}`}
      width={drawn.layered ? view.w : undefined}
      height={drawn.layered ? view.h : undefined}
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label={`Code map of ${overview.folder || overview.root}`}
    >
      <defs>
        <marker id="cm-arrow" markerWidth="8" markerHeight="6" refX="7.5" refY="3" orient="auto" markerUnits="userSpaceOnUse">
          <polygon points="0 0, 8 3, 0 6" fill="var(--dit-dim)" />
        </marker>
        <marker id="cm-arrow-lit" markerWidth="8" markerHeight="6" refX="7.5" refY="3" orient="auto" markerUnits="userSpaceOnUse">
          <polygon points="0 0, 8 3, 0 6" fill="var(--dit-accent)" />
        </marker>
      </defs>
      <g>
        {shown.map((e) => (
          <Edge
            key={edgeKey(e)}
            edge={e}
            a={drawn.at.get(e.from)}
            b={drawn.at.get(e.to)}
            layered={drawn.layered}
            both={pairs.has(`${e.to}\u0000${e.from}`)}
            state={hovered === null ? "plain" : e.from === hovered || e.to === hovered ? "lit" : "dim"}
          />
        ))}
      </g>
      <g>{nodes}</g>
    </svg>
  );

  return (
    <div ref={host} className="relative flex min-h-0 flex-1 flex-col">
      {drawn.layered ? <div className="min-h-0 flex-1 overflow-auto p-4">{svg}</div> : svg}

      {tip && hover ? <Tooltip unit={tip.unit} x={hover.x} y={hover.y} /> : null}

      {/* In the flow, not over it: a legend on top of the graph hides its last row. */}
      <div className="pointer-events-none mx-3 mb-3 flex shrink-0 flex-wrap items-center gap-3 self-start rounded-md border border-edge bg-card/90 px-3 py-1.5 text-[11px] text-muted">
        <Legend />
        {drawn.edges.length > EDGE_CAP ? (
          <span>
            showing {EDGE_CAP} of {drawn.edges.length} imports — hover a node for all of it
          </span>
        ) : null}
        {drawn.hidden > 0 ? <span className="text-warn-text">{drawn.hidden} more — narrow the folder</span> : null}
      </div>
    </div>
  );
}

function Edge({
  edge,
  a,
  b,
  layered,
  both,
  state,
}: {
  edge: CodeUnitEdgeDto;
  a: PlacedUnit | undefined;
  b: PlacedUnit | undefined;
  layered: boolean;
  both: boolean;
  state: "plain" | "lit" | "dim";
}) {
  if (!a || !b) return null;
  let d: string;
  if (layered && Math.abs(b.x - a.x) > (a.w + b.w) / 2) {
    // Between layers: leave one box's facing side, reach the other's.
    const right = b.x > a.x;
    d = connector(a.x + (right ? a.w / 2 : -a.w / 2), a.y, b.x + (right ? -b.w / 2 - 3 : b.w / 2 + 3), b.y);
  } else {
    // Bend by a fixed offset perpendicular to the line; the pair's second
    // edge bends the other way because its direction is reversed.
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy) || 1;
    const bend = both || layered ? 18 : 0;
    const cx = (a.x + b.x) / 2 + (-dy / len) * bend;
    const cy = (a.y + b.y) / 2 + (dx / len) * bend;
    const start = boxExit(a.x, a.y, a.w + 4, a.h + 4, cx, cy);
    const end = boxExit(b.x, b.y, b.w + 6, b.h + 6, cx, cy);
    d = `M${start.x},${start.y} Q${cx},${cy} ${end.x},${end.y}`;
  }
  const lit = state === "lit";
  return (
    <path
      d={d}
      fill="none"
      stroke={lit ? "var(--dit-accent)" : "var(--dit-dim)"}
      strokeOpacity={state === "dim" ? 0.15 : lit ? 0.95 : 0.7}
      strokeWidth={edgeWidth(edge.imports)}
      strokeLinecap="round"
      markerEnd={lit ? "url(#cm-arrow-lit)" : "url(#cm-arrow)"}
    >
      <title>{`${edge.from} → ${edge.to}: ${edge.imports} import${edge.imports === 1 ? "" : "s"}`}</title>
    </path>
  );
}

function Unit({
  placed,
  focused,
  dim,
  lit,
  onEnter,
  onLeave,
  onOpen,
}: {
  placed: PlacedUnit;
  focused: boolean;
  dim: boolean;
  lit: boolean;
  onEnter: (ev: React.PointerEvent) => void;
  onLeave: () => void;
  onOpen: () => void;
}) {
  const { unit, x, y, w, h } = placed;
  const gen = fullyGenerated(unit);
  // Folders carry a trailing slash; files are mono, a touch narrower.
  const room = charsFor(w - 16, unit.folder ? 7.2 : 7);
  const label = unit.folder ? `${middleTruncate(lastSegment(unit.path), room - 1)}/` : middleTruncate(lastSegment(unit.path), room);
  const stroke = focused || lit ? "var(--dit-accent)" : gen ? "var(--dit-edge)" : "var(--dit-ctl)";
  return (
    // An SVG node cannot be a <button>, so it takes the role and the keys.
    <g
      role="button"
      tabIndex={0}
      aria-label={`${unit.folder ? "Open folder" : "Focus file"} ${unit.path}`}
      className="cursor-pointer outline-none"
      transform={`translate(${x - w / 2},${y - h / 2})`}
      opacity={dim ? 0.3 : 1}
      onPointerEnter={onEnter}
      onPointerMove={onEnter}
      onPointerLeave={onLeave}
      onClick={onOpen}
      onKeyDown={(ev) => {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          onOpen();
        }
      }}
    >
      <title>{unit.path}</title>
      <rect
        width={w}
        height={h}
        rx={unit.folder ? 8 : h / 2}
        fill={focused ? "var(--dit-accent-soft)" : gen ? "var(--dit-sunken)" : "var(--dit-card)"}
        stroke={stroke}
        strokeWidth={focused ? 2 : lit ? 1.6 : 1}
        strokeDasharray={gen ? "4 3" : undefined}
      />
      <text
        x={w / 2}
        y={unit.folder ? h / 2 - 3 : h / 2 + 4}
        textAnchor="middle"
        fontSize={unit.folder ? 12.5 : 11.5}
        fontWeight={unit.folder ? 600 : 500}
        fontFamily={unit.folder ? "var(--font-sans)" : "var(--font-mono)"}
        fill={gen ? "var(--dit-muted)" : "var(--dit-ink)"}
      >
        {label}
      </text>
      {unit.folder ? (
        <text
          x={w / 2}
          y={h / 2 + 12}
          textAnchor="middle"
          fontSize={10.5}
          fill="var(--dit-muted)"
          fontFamily="var(--font-sans)"
        >
          {unit.files} file{unit.files === 1 ? "" : "s"}
        </text>
      ) : null}
      {gen ? (
        <g transform={`translate(${w - 24},${-7})`}>
          <rect width={24} height={14} rx={4} fill="var(--dit-sunken)" stroke="var(--dit-ctl)" />
          <text x={12} y={10} textAnchor="middle" fontSize={9} fill="var(--dit-muted)" fontFamily="var(--font-mono)">
            gen
          </text>
        </g>
      ) : null}
    </g>
  );
}

function Tooltip({ unit, x, y }: { unit: CodeUnitDto; x: number; y: number }) {
  const gen = fullyGenerated(unit);
  return (
    <div
      role="tooltip"
      className="pointer-events-none absolute z-10 max-w-80 rounded-md border border-edge bg-card px-3 py-2 text-xs shadow-lg"
      style={{ left: x + 14, top: y + 14 }}
    >
      <div className="break-all font-mono text-[11.5px] text-ink">
        {unit.path}
        {unit.folder ? "/" : ""}
      </div>
      <div className="mt-1 flex flex-wrap gap-x-3 text-muted">
        <span>
          {unit.files} file{unit.files === 1 ? "" : "s"}
          {unit.generated > 0 ? ` · ${gen ? "all" : unit.generated} generated` : ""}
        </span>
        <span>↘ {unit.inbound} in</span>
        <span>↗ {unit.outbound} out</span>
      </div>
      <div className="mt-1 text-faint">{unit.folder ? "Click to open the folder" : "Click to focus the file"}</div>
    </div>
  );
}

function Legend() {
  return (
    <>
      <span className="flex items-center gap-1.5">
        <svg width="18" height="12" aria-hidden>
          <rect x="0.5" y="0.5" width="17" height="11" rx="3" fill="var(--dit-card)" stroke="var(--dit-ctl)" />
        </svg>
        folder
      </span>
      <span className="flex items-center gap-1.5">
        <svg width="18" height="10" aria-hidden>
          <rect x="0.5" y="0.5" width="17" height="9" rx="4.5" fill="var(--dit-card)" stroke="var(--dit-ctl)" />
        </svg>
        file
      </span>
      <span className="flex items-center gap-1.5">
        <svg width="18" height="12" aria-hidden>
          <rect x="0.5" y="0.5" width="17" height="11" rx="3" fill="var(--dit-sunken)" stroke="var(--dit-edge)" strokeDasharray="3 2" />
        </svg>
        generated
      </span>
      <span className="flex items-center gap-1.5">
        <svg width="22" height="8" aria-hidden>
          <line x1="1" y1="4" x2="16" y2="4" stroke="var(--dit-dim)" strokeWidth="2" />
          <polygon points="15 1, 21 4, 15 7" fill="var(--dit-dim)" />
        </svg>
        imports · thicker = more
      </span>
    </>
  );
}
