// The Flow sidebar section: which orchestration is on screen, and — once a
// node is picked — everything the diagram cannot fit inside a 208-pixel box.
//
// This is archify's relationship lens and route probe, in the place this app
// already keeps contextual detail. It reads the same selection the canvas
// draws from, so clicking a node out there fills this in, and clicking a
// blocker in here moves the canvas. Nothing here is written: every list
// below is a derivation over the board's nodes and edges.

import {
  ArrowDown,
  ArrowUp,
  ChevronLeft,
  ChevronRight,
  GitCommitHorizontal,
  Route,
  Target,
  Waypoints,
} from "lucide-react";
import { Btn, Row, Sp } from "../chrome";
import { PaneSection } from "../PaneSection";
import { cn } from "../../lib/cn";
import { ALL_FLOWS, useViewOptions } from "../../lib/viewopts";
import { semanticOf } from "../../lib/flowgraph";
import { useFlowModel } from "../../views/FlowView";
import { useFieldEvents } from "../../lib/queries";
import type { FlowNodeDto } from "../../lib/types";

export function FlowPane({ onOpen }: { onOpen: (id: string) => void }) {
  const model = useFlowModel();
  const { flow: opts, setFlow, selectFlowNode, setFlowProbe, clearFlowProbe, clearFlowLens } =
    useViewOptions();
  const { data, graph, selected } = model;

  const go = (id: string) => selectFlowNode(id, { center: true });

  return (
    <>
      <PaneSection id="flow.flows" title="Flows" count={model.flows.data?.length ?? null}>
        {model.flows.data?.map((f) => (
          <Row key={f.name} on={opts.flow === f.name} onClick={() => setFlow(f.name)}>
            <Waypoints className="i" aria-hidden />
            <span className="lbl">{f.name}</span>
            <span className="cnt">{f.issues}</span>
          </Row>
        ))}
        <Row on={opts.flow === ALL_FLOWS} onClick={() => setFlow(ALL_FLOWS)}>
          <Waypoints className="i" aria-hidden />
          <span className="lbl">all flows</span>
          <span className="cnt">{data?.nodes.length ?? 0}</span>
        </Row>
      </PaneSection>

      <PaneSection id="flow.path" title="Critical path" count={`${Math.max(0, model.mainPath.length - 1)} hops`}>
        {model.mainPath.length === 0 ? (
          <p className="sb-note">No dependencies on this board yet.</p>
        ) : (
          <div className="fstep">
            <Btn onClick={() => model.stepPath(-1)} title="Previous step ([)">
              <ChevronLeft className="i" aria-hidden />
            </Btn>
            <span className="fstep-at">
              {model.pathIndex === -1 ? "—" : model.pathIndex + 1} / {model.mainPath.length}
            </span>
            <Btn onClick={() => model.stepPath(1)} title="Next step (])">
              <ChevronRight className="i" aria-hidden />
            </Btn>
          </div>
        )}
      </PaneSection>

      <PaneSection
        id="flow.route"
        title="Route"
        actions={
          opts.from !== null || opts.to !== null ? (
            <button type="button" className="dql lnk" onClick={clearFlowProbe}>
              clear
            </button>
          ) : null
        }
      >
        <RouteProbe model={model} onGo={go} />
      </PaneSection>

      <PaneSection
        id="flow.selected"
        title="Selected"
        fill
        actions={
          selected !== null ? (
            <button type="button" className="dql lnk" onClick={clearFlowLens}>
              clear
            </button>
          ) : null
        }
      >
        {selected === null ? (
          <p className="sb-note">
            Click a node to light up everything it waits on and everything waiting on it. Press{" "}
            <span className="mono">f</span> to find one by name.
          </p>
        ) : (
          <div className="fpass">
            <div className="fpass-head">
              <i className={cn("nd", semanticOf(selected))} aria-hidden />
              <span className="t">{selected.title}</span>
            </div>
            <dl className="fpass-meta">
              <dt>ref</dt>
              <dd className="mono">
                {selected.number !== null ? `#${selected.number}` : selected.short_ref}
              </dd>
              <dt>status</dt>
              <dd>{selected.status_label}</dd>
              {selected.lane !== null ? (
                <>
                  <dt>lane</dt>
                  <dd>{selected.lane}</dd>
                </>
              ) : null}
              <dt>stage</dt>
              <dd className="mono">{selected.stage}</dd>
              {selected.priority !== null ? (
                <>
                  <dt>priority</dt>
                  <dd className="mono">{selected.priority}</dd>
                </>
              ) : null}
              {selected.claim !== null ? (
                <>
                  <dt>claim</dt>
                  <dd>
                    @{selected.claim.claimed_by}
                    {selected.claim.stale ? " · stale" : ""}
                  </dd>
                </>
              ) : null}
              <dt>reach</dt>
              <dd className="mono">
                {model.upstream.size} up · {model.downstream.size} down
              </dd>
            </dl>

            <div className="fpass-acts">
              <Btn primary onClick={() => onOpen(selected.short_ref)}>
                Open issue
              </Btn>
              <Btn onClick={() => setFlowProbe("from", selected.id)} title="Use as the route's start">
                <Route className="i" aria-hidden />
                From
              </Btn>
              <Btn onClick={() => setFlowProbe("to", selected.id)} title="Use as the route's end">
                <Target className="i" aria-hidden />
                To
              </Btn>
            </div>

            <NodeList
              heading="Waits on"
              icon={<ArrowUp className="i" aria-hidden />}
              ids={graph?.in.get(selected.id) ?? []}
              lookup={(id) => graph?.node.get(id) ?? null}
              onGo={go}
              empty="Nothing on this board blocks it."
            />
            <NodeList
              heading="Blocks"
              icon={<ArrowDown className="i" aria-hidden />}
              ids={graph?.out.get(selected.id) ?? []}
              lookup={(id) => graph?.node.get(id) ?? null}
              onGo={go}
              empty="Nothing on this board waits on it."
            />

            <Evidence id={selected.short_ref} commits={selected.commits} />

            {selected.outside_blockers.length > 0 ? (
              <>
                <div className="sb-h">Outside this flow</div>
                <ul className="fplist">
                  {selected.outside_blockers.map((b) => (
                    <li key={b.id}>
                      <button
                        type="button"
                        className={cn("fplist-row", b.satisfied && "muted")}
                        onClick={() => onOpen(b.short_ref)}
                        title="Open the blocker — it is not a member of this flow, so it never draws"
                      >
                        <span className="t">{b.title}</span>
                        <span className="r mono">
                          {b.gone ? "gone" : b.satisfied ? "cleared" : b.status_label}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </>
            ) : null}
          </div>
        )}
      </PaneSection>
    </>
  );
}

/** Source evidence, derived (ADR 0021): the commits that have actually
 *  touched this issue. Archify's equivalent has to be written by hand and
 *  pinned to a revision; here it is read from history that git already
 *  keeps, so it cannot go stale and nobody maintains it. */
function Evidence({ id, commits }: { id: string; commits: number }) {
  const events = useFieldEvents(id);
  if (commits === 0) return null;
  // One row per commit, newest first — the same event may touch several
  // fields, and the reader is asking about commits, not fields.
  const seen = new Set<string>();
  const rows = (events.data ?? [])
    .slice()
    .reverse()
    .filter((e) => !seen.has(e.commit_sha) && seen.add(e.commit_sha) !== undefined)
    .slice(0, 8);
  return (
    <>
      <div className="sb-h">
        <GitCommitHorizontal className="i" aria-hidden />
        Commits
        <Sp />
        <span className="dql">{commits}</span>
      </div>
      {rows.length === 0 ? (
        <p className="sb-note">
          {events.isPending ? "Reading the history…" : "The history is still being indexed."}
        </p>
      ) : (
        <ul className="fplist">
          {rows.map((e) => (
            <li key={e.commit_sha}>
              <span className="fplist-row" title={`${e.field} changed by ${e.author}`}>
                <span className="r mono">{e.commit_sha.slice(0, 7)}</span>
                <span className="t">
                  {e.field} · {e.author}
                </span>
              </span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function RouteProbe({
  model,
  onGo,
}: {
  model: ReturnType<typeof useFlowModel>;
  onGo: (id: string) => void;
}) {
  const { flow: opts } = useViewOptions();
  const { graph, route } = model;
  const name = (id: string | null) => {
    if (id === null) return null;
    const node = graph?.node.get(id);
    if (node === undefined) return null;
    return node.number !== null ? `#${node.number}` : node.short_ref;
  };
  const from = name(opts.from);
  const to = name(opts.to);

  if (from === null && to === null) {
    return (
      <p className="sb-note">
        Pick a node and press <span className="mono">From</span>, then another and{" "}
        <span className="mono">To</span>, to trace the dependencies between them.
      </p>
    );
  }
  return (
    <div className="fprobe">
      <div className="fprobe-ends mono">
        {from ?? "…"} → {to ?? "…"}
      </div>
      {route === null ? (
        <p className="sb-note">
          {from !== null && to !== null
            ? "No chain of dependencies joins these two."
            : "Pick the other end."}
        </p>
      ) : (
        <ol className="fplist">
          {route.map((id) => {
            const node = graph?.node.get(id);
            if (node === undefined) return null;
            return (
              <li key={id}>
                <button type="button" className="fplist-row" onClick={() => onGo(id)}>
                  <i className={cn("nd", semanticOf(node))} aria-hidden />
                  <span className="t">{node.title}</span>
                </button>
              </li>
            );
          })}
        </ol>
      )}
    </div>
  );
}

function NodeList({
  heading,
  icon,
  ids,
  lookup,
  onGo,
  empty,
}: {
  heading: string;
  icon: React.ReactNode;
  ids: readonly string[];
  lookup: (id: string) => FlowNodeDto | null;
  onGo: (id: string) => void;
  empty: string;
}) {
  return (
    <>
      <div className="sb-h">
        {icon}
        {heading}
        <Sp />
        <span className="dql">{ids.length}</span>
      </div>
      {ids.length === 0 ? (
        <p className="sb-note">{empty}</p>
      ) : (
        <ul className="fplist">
          {ids.map((id) => {
            const node = lookup(id);
            if (node === null) return null;
            return (
              <li key={id}>
                <button type="button" className="fplist-row" onClick={() => onGo(id)}>
                  <i className={cn("nd", semanticOf(node))} aria-hidden />
                  <span className="t">{node.title}</span>
                  <span className="r mono">
                    {node.number !== null ? `#${node.number}` : node.short_ref}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </>
  );
}
