// The coordination board (ADR 0015): one swimlane per registered work
// stream, Unlaned last, statuses as columns — the view of several parallel
// actors (human or AI session) moving at once.
//
// Everything that matters here is derived server-side and never written
// back: readiness (is the card pickable), per-blocker state (satisfied,
// unsatisfied, broken) and claim liveness (claimed_by + claimed_at against
// the TTL). The screen is read-only on purpose — a card opens the issue
// panel, and writes go through it, exactly like any other list view.

import { useMemo } from "react";
import { CircleDot, Lock } from "lucide-react";
import { useSchema, useWorkflowBoard } from "../lib/queries";
import { useRegisterPeekList } from "../lib/peeklist";
import { ErrorBox, Loading } from "../components/states";
import { cn } from "../lib/cn";
import type { WorkflowCardDto } from "../lib/types";

export function WorkflowView({ onOpen }: { onOpen: (id: string) => void }) {
  const board = useWorkflowBoard();
  const schema = useSchema();

  const statuses = useMemo(
    () => schema.data?.workflow.statuses.map((status) => ({ id: status.id, label: status.label })) ?? [],
    [schema.data],
  );

  useRegisterPeekList(
    useMemo(
      () =>
        (board.data?.lanes ?? []).flatMap((lane) =>
          lane.cards.filter((card) => card.readiness === "ready").map((card) => card.short_ref),
        ),
      [board.data],
    ),
  );

  if (board.isPending) return <Loading label="Loading workflow…" className="flex-1" />;
  if (board.isError) {
    return (
      <ErrorBox
        error={board.error}
        title="Could not load the workflow board"
        onRetry={() => void board.refetch()}
      />
    );
  }

  const data = board.data;

  return (
    <div className="wflow">
      {data.lanes.map((lane) => {
        const id = lane.id ?? "(unlaned)";
        const ready = lane.cards.filter((card) => card.readiness === "ready").length;
        return (
          <section key={id} className="wlane">
            <header className="wlhead">
              <span className="wlname">{lane.label}</span>
              {lane.owners.length > 0 ? (
                <span className="wlowners mono">{lane.owners.join(", ")}</span>
              ) : null}
              <span className="wlmeta mono">
                {ready} ready · {lane.cards.length} total
              </span>
            </header>
            <div className="wlboard">
              {statuses.map((status) => {
                const cards = lane.cards.filter((card) => card.status === status.id);
                return (
                  <div key={status.id} className="col">
                    <div className="col-h">
                      <span>{status.label}</span>
                      <span className="cnt">{cards.length}</span>
                    </div>
                    <div className="cards">
                      {cards.map((card) => (
                        <WorkflowCard key={card.id} card={card} onOpen={onOpen} />
                      ))}
                    </div>
                  </div>
                );
              })}
              {/* Cards whose status left the workflow still must not vanish —
                  same rule as the classic board's stray column. */}
              {(() => {
                const strays = lane.cards.filter(
                  (card) => !statuses.some((status) => status.id === card.status),
                );
                if (strays.length === 0) return null;
                return (
                  <div className="col">
                    <div className="col-h">
                      <span>not in workflow</span>
                      <span className="cnt">{strays.length}</span>
                    </div>
                    <div className="cards">
                      {strays.map((card) => (
                        <WorkflowCard key={card.id} card={card} onOpen={onOpen} />
                      ))}
                    </div>
                  </div>
                );
              })()}
            </div>
          </section>
        );
      })}
      <footer className="wfoot">
        <span className="legend">
          claim TTL {data.claim_ttl_minutes} min · readiness, blocker states and claim age are derived,
          never stored
        </span>
      </footer>
    </div>
  );
}

function WorkflowCard({ card, onOpen }: { card: WorkflowCardDto; onOpen: (id: string) => void }) {
  const handle = card.number !== null ? `#${card.number}` : card.short_ref;
  const claim = card.claim;
  return (
    <button
      type="button"
      className={cn("card", card.readiness === "blocked" && "wblocked")}
      onClick={() => onOpen(card.short_ref)}
      title="Open the issue"
    >
      <div className="top">
        <span className="mono">{handle}</span>
        {card.priority ? <span className="chip">{card.priority}</span> : null}
        <span className="sp" />
        {card.readiness === "ready" ? (
          <CircleDot className="i ready" aria-hidden />
        ) : card.readiness === "blocked" ? (
          <Lock className="i" aria-hidden />
        ) : null}
      </div>
      <div className="title">{card.title}</div>
      <div className="bot">
        {card.blockers.map((blocker) => (
          <span
            key={blocker.id}
            className={cn("chip", blocker.state)}
            title={
              blocker.state === "broken"
                ? `${blocker.short_ref} is cancelled or gone — needs a human to re-point`
                : blocker.state === "unsatisfied"
                  ? `waiting on ${blocker.short_ref} (${blocker.status})`
                  : `${blocker.short_ref} is through the gate`
            }
          >
            {blocker.number !== null ? `#${blocker.number}` : blocker.short_ref}
            {blocker.state === "broken" ? "!" : ""}
          </span>
        ))}
        <span className="sp" />
        {claim ? (
          <span className={cn("chip", "claim", claim.stale && "stale")} title={claim.claimed_at}>
            {claim.claimed_by}
            {claim.stale ? " (stale)" : ""}
          </span>
        ) : null}
      </div>
    </button>
  );
}
