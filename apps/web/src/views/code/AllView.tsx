// Every file of a root at once: the whole import network, clustered by top
// folder. The graph is fetched only when this view opens, filtered here, laid
// out by force in a worker (streaming a picture every few ticks so it settles
// in view), and remembered per root and filter set so coming back is instant.

import { useEffect, useMemo, useRef, useState } from "react";
import { cn } from "../../lib/cn";
import {
  clusterAnchors,
  clusterLabel,
  clustersOf,
  DEFAULT_FILTERS,
  filterGraph,
  formatCount,
  graphRadius,
  layoutKey,
  MIN_USERS_STEPS,
  searchGraph,
  type GraphFilters,
} from "../../lib/codegraph";
import {
  LAYOUT_STREAM_EVERY,
  startLayout,
  type LayoutInput,
  type LayoutMessage,
  type LayoutRequest,
} from "../../lib/codegraphLayout";
import { useCodeGraph } from "../../lib/queries";
import { ErrorBox, Loading } from "../../components/states";
import { GraphCanvas, type GraphCanvasHandle } from "./GraphCanvas";

/** Finished layouts, by `layoutKey`: this visit's only, never persisted. */
const LAYOUTS = new Map<string, Float32Array>();
const LAYOUTS_KEPT = 12;

let nextRun = 1;

/** Lay out off the main thread; where there is no worker (tests, very old
 *  browsers), in slices on the main thread instead. Returns a cancel. */
function spawnLayout(input: LayoutInput, onMessage: (message: LayoutMessage) => void): () => void {
  const request: LayoutRequest = { id: nextRun++, input };
  if (typeof Worker !== "undefined") {
    try {
      const worker = new Worker(new URL("../../lib/codegraph.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (event: MessageEvent<LayoutMessage>) => onMessage(event.data);
      worker.postMessage(request);
      return () => worker.terminate();
    } catch {
      /* fall through to the main thread */
    }
  }
  let cancelled = false;
  const run = startLayout(input);
  const slice = () => {
    if (cancelled) return;
    run.step(LAYOUT_STREAM_EVERY);
    if (run.done()) onMessage({ id: request.id, kind: "done", positions: run.positions(), ticks: run.ticks() });
    else {
      onMessage({ id: request.id, kind: "tick", positions: run.positions(), progress: run.progress() });
      setTimeout(slice, 0);
    }
  };
  setTimeout(slice, 0);
  return () => {
    cancelled = true;
  };
}
function mark(name: string) {
  try {
    performance.mark(`dit:code-graph:${name}`);
  } catch {
    /* timing is a nicety */
  }
}

export function AllView({
  root,
  query,
  seek,
  onOpen,
}: {
  root: string;
  /** The header search box, live: matching files light up. */
  query: string;
  /** Bumped by Enter in the search box: centre on the best match. */
  seek: number;
  onOpen: (path: string) => void;
}) {
  const graph = useCodeGraph(root, true);
  const [filters, setFilters] = useState<GraphFilters>(DEFAULT_FILTERS);
  const [isolated, setIsolated] = useState<number | null>(null);
  const canvas = useRef<GraphCanvasHandle>(null);

  const drawn = useMemo(() => (graph.data ? filterGraph(graph.data, filters) : null), [filters, graph.data]);
  const clusters = useMemo(() => (drawn ? clustersOf(drawn.files) : null), [drawn]);
  const radii = useMemo(() => Float32Array.from(drawn?.files.map((f) => graphRadius(f.users)) ?? []), [drawn]);
  const slot = useMemo(
    () => Int32Array.from(clusters ? [...clusters.of].map((c) => clusters.colour[c] ?? 0) : []),
    [clusters],
  );
  const key = useMemo(() => (drawn ? layoutKey(root, filters, drawn) : null), [drawn, filters, root]);

  const [layout, setLayout] = useState<{ key: string; positions: Float32Array; progress: number; done: boolean } | null>(null);

  useEffect(() => {
    if (!drawn || !clusters || key === null) return;
    const cached = LAYOUTS.get(key);
    if (cached) {
      setLayout({ key, positions: cached, progress: 1, done: true });
      return;
    }
    setLayout((old) => (old?.key === key ? old : null));
    const input: LayoutInput = {
      paths: drawn.files.map((f) => f.path),
      radii: [...radii],
      cluster: [...clusters.of],
      anchors: clusterAnchors(clusters.names.length, drawn.files.length),
      edges: drawn.edges,
    };
    mark("request");
    let first = true;
    const cancel = spawnLayout(input, (m) => {
      if (first) {
        first = false;
        mark("first");
      }
      if (m.kind === "done") {
        mark("settled");
        LAYOUTS.set(key, m.positions);
        if (LAYOUTS.size > LAYOUTS_KEPT) {
          const oldest = LAYOUTS.keys().next().value;
          if (oldest !== undefined) LAYOUTS.delete(oldest);
        }
        setLayout({ key, positions: m.positions, progress: 1, done: true });
      } else {
        setLayout({ key, positions: m.positions, progress: m.progress, done: false });
      }
    });
    return cancel;
  }, [clusters, drawn, key, radii]);

  const matches = useMemo(
    () => (drawn && query.trim().length >= 2 ? searchGraph(drawn.files, query) : []),
    [drawn, query],
  );
  const lastSeek = useRef(seek);
  useEffect(() => {
    if (seek === lastSeek.current) return;
    lastSeek.current = seek;
    const best = matches[0];
    if (best !== undefined) canvas.current?.centre(best);
  }, [matches, seek]);

  // A filter change can renumber clusters; an isolation must not survive it.
  useEffect(() => setIsolated(null), [clusters]);

  if (graph.isPending) return <Loading label="Reading every file of the root…" />;
  if (graph.isError) return <ErrorBox error={graph.error} onRetry={() => void graph.refetch()} title="Could not read the network" />;
  if (!drawn || !clusters) return null;

  const positions = layout?.key === key ? layout.positions : null;
  const progress = layout?.key === key ? layout.progress : 0;
  const settling = !(layout?.key === key && layout.done);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 border-b border-edge px-4 py-1.5 text-[12px] text-ink-2">
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={filters.generated}
            onChange={(ev) => setFilters((f) => ({ ...f, generated: ev.target.checked }))}
          />
          show generated
        </label>
        <label className="flex items-center gap-1.5">
          <input type="checkbox" checked={filters.tests} onChange={(ev) => setFilters((f) => ({ ...f, tests: ev.target.checked }))} />
          show tests
        </label>
        <label className="flex items-center gap-1.5">
          min users
          <select
            className="h-6 rounded border border-ctl bg-card px-1 font-mono text-[11.5px] text-ink"
            value={filters.minUsers}
            onChange={(ev) => setFilters((f) => ({ ...f, minUsers: Number(ev.target.value) }))}
          >
            {MIN_USERS_STEPS.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        <span className="flex-1" />
        {settling ? <span className="font-mono text-[11.5px] text-muted">laying out… {Math.round(progress * 100)}%</span> : null}
        <span className="font-mono text-[11.5px] text-muted">
          {formatCount(drawn.files.length)} files · {formatCount(drawn.edges.length)} imports shown of{" "}
          {formatCount(drawn.total.files)} · {formatCount(drawn.total.edges)}
        </span>
        {query.trim().length >= 2 ? (
          <span className="font-mono text-[11.5px] text-accent">
            {formatCount(matches.length)} match{matches.length === 1 ? "" : "es"}
          </span>
        ) : null}
      </div>

      {drawn.files.length === 0 ? (
        <p className="p-8 text-center text-[12.5px] text-faint">No file passes these filters.</p>
      ) : (
        <GraphCanvas
          ref={canvas}
          files={drawn.files}
          edges={drawn.edges}
          positions={positions}
          radii={radii}
          slot={slot}
          cluster={clusters.of}
          isolated={isolated}
          matches={matches}
          settling={settling}
          onOpen={(i) => {
            const f = drawn.files[i];
            if (f) onOpen(f.path);
          }}
        />
      )}
      {positions === null && drawn.files.length > 0 ? (
        <div className="pointer-events-none absolute inset-0 top-10 flex items-center justify-center">
          <Loading label="Laying out the network…" />
        </div>
      ) : null}

      <div className="absolute bottom-3 left-3 max-h-[60%] max-w-64 overflow-y-auto rounded-md border border-edge bg-card/90 p-2 text-[11.5px]">
        <div className="mb-1 flex items-center justify-between text-[10.5px] font-semibold tracking-wide text-muted uppercase">
          Folders
          {isolated !== null ? (
            <button type="button" className="font-normal normal-case text-accent" onClick={() => setIsolated(null)}>
              show all
            </button>
          ) : null}
        </div>
        <ul>
          {clusters.names.map((name, i) => (
            <li key={name}>
              <button
                type="button"
                onClick={() => setIsolated((c) => (c === i ? null : i))}
                aria-pressed={isolated === i}
                className={cn(
                  "flex w-full items-center gap-2 rounded px-1 py-0.5 text-left hover:bg-hover",
                  isolated !== null && isolated !== i && "opacity-45",
                )}
              >
                <span
                  className="size-2.5 shrink-0 rounded-full"
                  style={{
                    background: (clusters.colour[i] ?? -1) < 0 ? "var(--dit-dim)" : `var(--dit-cat-${clusters.colour[i]})`,
                  }}
                  aria-hidden
                />
                <span className="min-w-0 flex-1 truncate font-mono text-ink-2">{clusterLabel(name, clusters.common)}</span>
                <span className="font-mono text-faint">{formatCount(clusters.counts[i] ?? 0)}</span>
              </button>
            </li>
          ))}
        </ul>
        {filters.generated ? (
          <p className="mt-1 flex items-center gap-2 border-t border-edge px-1 pt-1 text-faint">
            <span className="size-2.5 rounded-full" style={{ background: "var(--dit-dim)" }} aria-hidden /> generated
          </p>
        ) : null}
      </div>
    </div>
  );
}
