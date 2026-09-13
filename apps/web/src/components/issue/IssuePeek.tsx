// The issue panel: an issue opened beside the list it came from, instead of
// replacing it. The list stays on screen and scrollable, J/K walk it, Escape
// closes, and ⌘↵ hands the same issue to the full page. The open issue lives
// in the URL, so a reload or a shared link reopens the panel over the list.
//
// Everything inside is the same components the page renders (./parts), so
// there is one implementation of "editing an issue", not two. The markup is
// the design's `.peek` recipe: header strip, scrolling body, key-hint footer.

import { useState } from "react";
import { Link2, Maximize2, MoreHorizontal, Star, X } from "lucide-react";
import { ApiError } from "../../lib/api";
import { useFieldEvents, useIssue } from "../../lib/queries";
import { relativeTime } from "../../lib/format";
import { peekHost, routeToHash, withPeek, type Route } from "../../lib/router";
import { useIsStarred } from "../../lib/starred";
import { IssueHandle, TypeBadge } from "../badges";
import { ErrorBox, Loading } from "../states";
import { IBtn, Kbd, MenuButton, Sp } from "../chrome";
import {
  copyText,
  DescriptionSection,
  IssueActivity,
  IssueProps,
  IssueTitle,
  starWithToast,
  useMoreMenu,
  type ActivityFilter,
} from "./parts";
import type { IssueDto } from "../../lib/types";

export function IssuePeek({
  id,
  route,
  onClose,
  onExpand,
  onStep,
  canStepBack,
  canStepForward,
}: {
  id: string;
  /** The route the panel is open over — the link it copies points here. */
  route: Route;
  onClose: () => void;
  onExpand: () => void;
  /** Walk the list behind the panel: -1 previous, +1 next. */
  onStep: (delta: -1 | 1) => void;
  canStepBack: boolean;
  canStepForward: boolean;
}) {
  const issue = useIssue(id);
  // One unfiltered history query feeds the per-field blame lines and the
  // stream; the field param exists on the wire but would cost a request
  // per field.
  const history = useFieldEvents(id);
  const [filter, setFilter] = useState<ActivityFilter>("all");
  const [highlight, setHighlight] = useState<{ seq: number; nonce: number } | null>(null);

  const showChanges = (seq?: number) => {
    setFilter("changes");
    if (seq !== undefined) setHighlight({ seq, nonce: Date.now() });
  };

  return (
    <section className="peek open" aria-label={`Issue ${id}`}>
      {issue.data ? (
        <PeekHeader
          issue={issue.data}
          route={route}
          onClose={onClose}
          onExpand={onExpand}
        />
      ) : (
        <div className="peek-h">
          <span className="mono text-xs text-muted">{id}</span>
          <Sp />
          <IBtn title="Close (Esc)" aria-label="Close" onClick={onClose}>
            <X className="i" aria-hidden />
          </IBtn>
        </div>
      )}

      <div className="peek-body">
        {issue.isPending ? <Loading label={`Loading ${id}…`} /> : null}
        {issue.isError ? (
          <ErrorBox
            error={issue.error}
            title={
              issue.error instanceof ApiError && issue.error.status === 404
                ? `No issue “${id}” in this workspace`
                : "Could not load the issue"
            }
            onRetry={() => void issue.refetch()}
          />
        ) : null}
        {issue.data ? (
          <>
            <IssueTitle issue={issue.data} as="h2" />
            <IssueProps issue={issue.data} history={history.data ?? []} compact onShowChanges={showChanges} />
            <DescriptionSection issue={issue.data} />
            <IssueActivity
              issue={issue.data}
              history={history.data ?? []}
              filter={filter}
              onFilterChange={setFilter}
              highlight={highlight}
            />
          </>
        ) : null}
      </div>

      {issue.data ? (
        <div className="peek-f">
          <span>
            created {relativeTime(issue.data.created)} · updated {relativeTime(issue.data.updated)}
          </span>
          <Sp />
          {/* The shell owns J/K; the hints are also buttons so a mouse can
              walk the list from here. */}
          <span className="k">
            <button
              type="button"
              className="inline-flex border-0 bg-transparent p-0"
              disabled={!canStepForward}
              aria-label="Next issue"
              onClick={() => onStep(1)}
            >
              <Kbd>J</Kbd>
            </button>
            <button
              type="button"
              className="inline-flex border-0 bg-transparent p-0"
              disabled={!canStepBack}
              aria-label="Previous issue"
              onClick={() => onStep(-1)}
            >
              <Kbd>K</Kbd>
            </button>{" "}
            next / prev
          </span>
          <span className="k">
            <button type="button" className="inline-flex border-0 bg-transparent p-0" aria-label="Open as page" onClick={onExpand}>
              <Kbd>⌘↵</Kbd>
            </button>{" "}
            page
          </span>
          <span className="k">
            <button type="button" className="inline-flex border-0 bg-transparent p-0" aria-label="Close" onClick={onClose}>
              <Kbd>esc</Kbd>
            </button>{" "}
            close
          </span>
        </div>
      ) : null}
    </section>
  );
}

function PeekHeader({
  issue,
  route,
  onClose,
  onExpand,
}: {
  issue: IssueDto;
  route: Route;
  onClose: () => void;
  onExpand: () => void;
}) {
  const starred = useIsStarred(issue.short_ref);
  const link = `${window.location.origin}${window.location.pathname}${routeToHash(withPeek(route, issue.short_ref))}`;
  const more = useMoreMenu({
    issue,
    inPeek: true,
    route,
    from: peekHost(route),
    onToggleSurface: onExpand,
    onDeleted: onClose,
  });
  return (
    <div className="peek-h">
      <IssueHandle shortRef={issue.short_ref} number={issue.number} />
      <button
        type="button"
        className="mono refBtn"
        style={{ fontSize: 10.5, color: "var(--faint)" }}
        title="Permanent short ref — click to copy"
        onClick={() => void copyText(issue.short_ref, "Short ref copied")}
      >
        {issue.short_ref}
      </button>
      <TypeBadge type={issue.type} />
      <Sp />
      <IBtn title="Open as page (⌘↵)" aria-label="Open as page" onClick={onExpand}>
        <Maximize2 className="i" aria-hidden />
      </IBtn>
      <IBtn title="Copy link" aria-label="Copy link" onClick={() => void copyText(link, "Link copied")}>
        <Link2 className="i" aria-hidden />
      </IBtn>
      <IBtn
        on={starred}
        title={starred ? "Unstar" : "Star"}
        aria-label={starred ? "Unstar" : "Star"}
        aria-pressed={starred}
        onClick={() => starWithToast(issue.short_ref)}
      >
        <Star className="i" aria-hidden />
      </IBtn>
      <MenuButton items={more} className="relative" align="end">
        <IBtn title="More" aria-label="More">
          <MoreHorizontal className="i" aria-hidden />
        </IBtn>
      </MenuButton>
      <span className="sep" />
      <IBtn title="Close (Esc)" aria-label="Close" onClick={onClose}>
        <X className="i" aria-hidden />
      </IBtn>
    </div>
  );
}
