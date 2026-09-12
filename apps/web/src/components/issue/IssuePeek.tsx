// The issue panel: an issue opened beside the list it came from, instead of
// replacing it. The list stays on screen and scrollable, J/K walk it, Escape
// closes, and ⌘↵ hands the same issue to the full page. The open issue lives
// in the URL, so a reload or a shared link reopens the panel over the list.
//
// Everything inside is the same components the page renders (./parts), so
// there is one implementation of "editing an issue", not two.

import { ChevronDown, ChevronUp, Link2, Maximize2, X } from "lucide-react";
import { toast } from "sonner";
import { ApiError } from "../../lib/api";
import { useFieldEvents, useIssue } from "../../lib/queries";
import { relativeTime } from "../../lib/format";
import { routeToHash, withPeek, type Route } from "../../lib/router";
import { AssigneeCircles, IssueHandle, PriorityDot, TypeBadge } from "../badges";
import { ErrorBox, Loading } from "../states";
import { Kbd } from "../chrome";
import { DescriptionSection, IssueActivity, IssueFields, IssueTitle, StarButton } from "./parts";

function PanelButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      disabled={disabled}
      className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted transition-colors hover:bg-hover hover:text-ink disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-muted"
    >
      {children}
    </button>
  );
}

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
  // One unfiltered history query feeds the per-field blame lines; the field
  // param exists on the wire but would cost a request per field.
  const history = useFieldEvents(id);

  const copyLink = () => {
    const url = `${window.location.origin}${window.location.pathname}${routeToHash(withPeek(route, id))}`;
    void navigator.clipboard
      ?.writeText(url)
      .then(
        () => toast.success("Link copied"),
        () => toast.message(url),
      );
  };

  return (
    <aside
      aria-label={`Issue ${id}`}
      className="dit-panel-in absolute inset-y-0 right-0 z-20 flex w-[min(620px,100%)] flex-col border-l border-edge bg-app shadow-[var(--dit-shadow-lg)]"
    >
      <header className="flex h-12 shrink-0 items-center gap-1.5 border-b border-edge pl-4 pr-2">
        {issue.data ? (
          <>
            <IssueHandle shortRef={issue.data.short_ref} number={issue.data.number} />
            <span
              className="font-mono text-[10px] text-dim"
              title="the permanent short ref behind the number"
            >
              {issue.data.short_ref}
            </span>
            <TypeBadge type={issue.data.type} />
            <PriorityDot priority={issue.data.priority} />
            <span className="ml-1">
              <AssigneeCircles assignees={issue.data.assignees} />
            </span>
          </>
        ) : (
          <span className="font-mono text-xs text-muted">{id}</span>
        )}

        <span className="ml-auto flex items-center gap-0.5">
          <PanelButton label="Previous issue (K)" onClick={() => onStep(-1)} disabled={!canStepBack}>
            <ChevronUp className="size-4" aria-hidden />
          </PanelButton>
          <PanelButton label="Next issue (J)" onClick={() => onStep(1)} disabled={!canStepForward}>
            <ChevronDown className="size-4" aria-hidden />
          </PanelButton>
          <span className="mx-1 h-4 w-px bg-edge" aria-hidden />
          <StarButton shortRef={issue.data?.short_ref ?? id} />
          <PanelButton label="Copy link" onClick={copyLink}>
            <Link2 className="size-4" aria-hidden />
          </PanelButton>
          <PanelButton label="Open as page (⌘↵)" onClick={onExpand}>
            <Maximize2 className="size-4" aria-hidden />
          </PanelButton>
          <PanelButton label="Close (Esc)" onClick={onClose}>
            <X className="size-4" aria-hidden />
          </PanelButton>
        </span>
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-5 pb-8 pt-4">
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
            <div className="-mx-1.5">
              <IssueTitle issue={issue.data} size="panel" />
            </div>
            <IssueFields issue={issue.data} history={history.data ?? []} layout="panel" />
            <DescriptionSection issue={issue.data} />
            <IssueActivity issueId={issue.data.id} history={history.data ?? []} />
          </>
        ) : null}
      </div>

      {issue.data ? (
        <footer className="flex h-8 shrink-0 items-center gap-3 border-t border-edge px-4 font-mono text-[10.5px] text-faint">
          <span>
            created {relativeTime(issue.data.created)} · updated {relativeTime(issue.data.updated)}
          </span>
          <span className="ml-auto hidden items-center gap-3 min-[520px]:flex">
            <span className="flex items-center gap-1">
              <Kbd>J</Kbd>
              <Kbd>K</Kbd> next / prev
            </span>
            <span className="flex items-center gap-1">
              <Kbd>⌘↵</Kbd> page
            </span>
            <span className="flex items-center gap-1">
              <Kbd>esc</Kbd> close
            </span>
          </span>
        </footer>
      ) : null}
    </aside>
  );
}
