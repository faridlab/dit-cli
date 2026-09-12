// Issue detail, full page: fields on the right rail, description and
// activity in the main column.
//
// This is the expanded form of the side panel, not a different screen — the
// title, the fields, the description editor and the activity stream are the
// same components (components/issue/parts). What the page adds is room: a
// column wide enough to write in, and the rail's blame lines side by side
// with the prose.

import { ArrowLeft, PanelRight } from "lucide-react";
import { AssigneeCircles, IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { ErrorBox, Loading } from "../components/states";
import {
  DescriptionSection,
  IssueActivity,
  IssueFields,
  IssueTitle,
  StarButton,
} from "../components/issue/parts";
import { ApiError } from "../lib/api";
import { relativeTime } from "../lib/format";
import { useFieldEvents, useIssue } from "../lib/queries";
import type { PeekHost } from "../lib/router";

export function IssueDetailView({
  id,
  from,
  onCollapse,
}: {
  id: string;
  /** The list this page was opened from — where Back and "Show as panel" go. */
  from: PeekHost;
  onCollapse: () => void;
}) {
  const issue = useIssue(id);
  // One unfiltered history query feeds both the blame lines and the
  // timeline — the field param exists on the wire but costs a second request.
  const history = useFieldEvents(id);

  if (issue.isPending) {
    return <Loading label={`Loading ${id}…`} className="flex-1" />;
  }
  if (issue.isError) {
    const notFound = issue.error instanceof ApiError && issue.error.status === 404;
    return (
      <ErrorBox
        error={issue.error}
        title={notFound ? `No issue “${id}” in this workspace` : "Could not load the issue"}
        onRetry={() => void issue.refetch()}
      />
    );
  }

  const data = issue.data;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex items-center gap-3 border-b border-edge px-5 py-3">
        <button
          type="button"
          onClick={onCollapse}
          title={`Back to ${from}`}
          aria-label={`Back to ${from}`}
          className="rounded-md p-1 text-muted hover:bg-hover hover:text-ink"
        >
          <ArrowLeft className="size-4" aria-hidden />
        </button>
        <IssueHandle shortRef={data.short_ref} number={data.number} />
        <span
          className="font-mono text-[10px] text-dim"
          title="the permanent short ref behind the number"
        >
          {data.short_ref}
        </span>
        <TypeBadge type={data.type} />
        <PriorityDot priority={data.priority} />
        <div className="min-w-0 flex-1">
          <IssueTitle issue={data} size="page" />
        </div>
        <AssigneeCircles assignees={data.assignees} />
        <StarButton shortRef={data.short_ref} />
        <button
          type="button"
          onClick={onCollapse}
          title={`Show as a panel over ${from}`}
          className="flex shrink-0 items-center gap-1.5 rounded-md border border-edge px-2.5 py-1 text-[12px] text-ink-2 transition-colors hover:border-ctl hover:text-ink"
        >
          <PanelRight className="size-3.5" aria-hidden />
          <span className="hidden min-[900px]:inline">Show as panel</span>
        </button>
      </header>

      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col gap-[22px] overflow-y-auto px-6 pb-8 pt-5">
          <DescriptionSection issue={data} />
          <IssueActivity issueId={data.id} history={history.data ?? []} />
        </div>
        <aside className="flex w-[288px] shrink-0 flex-col gap-5 overflow-y-auto border-l border-edge px-4 pb-7 pt-5 min-[1180px]:w-[336px]">
          <section>
            <div className="mb-2.5 flex items-center justify-between gap-2">
              <span className="text-[11px] font-semibold uppercase tracking-[0.07em] text-muted">
                Fields
              </span>
              <span className="text-[11px] text-dim">who touched it last</span>
            </div>
            <IssueFields issue={data} history={history.data ?? []} layout="rail" />
          </section>

          {/* Epic is read-only here: v0.1's patch contract has no epic field,
              so linking an issue into an epic is an edit to the file itself. */}
          <p className="mt-auto border-t border-edge pt-3 font-mono text-[11px] leading-relaxed text-dim">
            {data.epic ? (
              <>
                epic {data.epic.slice(0, 8)}
                <br />
              </>
            ) : null}
            reported by {data.reporter ?? "—"}
            <br />
            created {relativeTime(data.created)} · updated {relativeTime(data.updated)}
          </p>
        </aside>
      </div>
    </div>
  );
}
