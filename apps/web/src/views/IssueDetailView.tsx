// Issue detail, full page: prose on the left, properties and history on the
// right rail.
//
// This is the expanded form of the side panel, not a different screen — the
// title, the properties, the description editor and the activity stream are
// the same components (components/issue/parts). What the page adds is room:
// a column wide enough to write in, and a rail that lays the field history
// and the data commits next to the prose. The markup is the design's `.page`
// recipe; the shell's header already draws the breadcrumbs and actions.

import { useState } from "react";
import { Link2, MoreHorizontal, Star } from "lucide-react";
import { IssueHandle, PriorityDot, TypeBadge } from "../components/badges";
import { HeadingNote, IBtn, MenuButton, SectionHeading, Sp } from "../components/chrome";
import { ErrorBox, Loading } from "../components/states";
import {
  copyText,
  DescriptionSection,
  IssueActivity,
  IssueProps,
  IssueTitle,
  starWithToast,
  useIssuePool,
  useMoreMenu,
  type ActivityFilter,
} from "../components/issue/parts";
import { ApiError } from "../lib/api";
import { relativeTime, resolveIdValue, shortSha } from "../lib/format";
import { useFieldEvents, useIssue } from "../lib/queries";
import { navigate, routeToHash, useRoute, withPeek, type PeekHost } from "../lib/router";
import { useIsStarred } from "../lib/starred";
import type { FieldEventDto, IssueDto } from "../lib/types";

/** Fields the server writes on every commit; the rail lists what people
 *  changed, not the bookkeeping around it. */
const NOISE_FIELDS = new Set(["updated", "number", "created"]);

export function IssueDetailView({
  id,
  from,
  onCollapse,
}: {
  id: string;
  /** The list this page was opened from — where "Show as panel" goes. */
  from: PeekHost;
  onCollapse: () => void;
}) {
  const issue = useIssue(id);
  // One unfiltered history query feeds the blame lines, the stream and the
  // rail — the field param exists on the wire but costs a request per field.
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

  return <IssuePage issue={issue.data} history={history.data ?? []} from={from} onCollapse={onCollapse} />;
}

function IssuePage({
  issue,
  history,
  from,
  onCollapse,
}: {
  issue: IssueDto;
  history: FieldEventDto[];
  from: PeekHost;
  onCollapse: () => void;
}) {
  const route = useRoute();
  const starred = useIsStarred(issue.short_ref);
  const [filter, setFilter] = useState<ActivityFilter>("all");
  const [highlight, setHighlight] = useState<{ seq: number; nonce: number } | null>(null);

  const showChanges = (seq?: number) => {
    setFilter("changes");
    if (seq !== undefined) setHighlight({ seq, nonce: Date.now() });
  };

  const link = `${window.location.origin}${window.location.pathname}${routeToHash(withPeek(route, issue.short_ref))}`;
  const more = useMoreMenu({
    issue,
    inPeek: false,
    route,
    from,
    onToggleSurface: onCollapse,
    // The page is gone with the issue; go back to the list it came from.
    onDeleted: () => navigate(withPeek({ name: "issue", id: issue.short_ref, from }, null)),
  });

  const pool = useIssuePool();
  const changes = history.filter((event) => !NOISE_FIELDS.has(event.field));
  const newestFirst = [...changes].reverse();
  // The rail records `epic` as an issue id; the pool names it.
  const titleOf = (id: string) => pool.find((candidate) => candidate.id === id)?.title;

  // The distinct data commits behind the events, newest first. Derived on
  // read: commit↔issue links are never stored (invariant 5).
  const commits: Array<{ sha: string; count: number; ts: string }> = [];
  for (const event of newestFirst) {
    const open = commits.find((each) => each.sha === event.commit_sha);
    if (open) open.count += 1;
    else commits.push({ sha: event.commit_sha, count: 1, ts: event.ts });
  }

  return (
    <div className="page">
      <div className="left">
        <div className="inner">
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <IssueHandle shortRef={issue.short_ref} number={issue.number} />
            <button
              type="button"
              className="mono refBtn"
              style={{ fontSize: 11, color: "var(--faint)" }}
              title="Permanent short ref — click to copy"
              onClick={() => void copyText(issue.short_ref, "Short ref copied")}
            >
              {issue.short_ref}
            </button>
            <TypeBadge type={issue.type} />
            <PriorityDot priority={issue.priority || null} />
            <span style={{ flex: 1 }} />
            <IBtn
              on={starred}
              title={starred ? "Unstar" : "Star"}
              aria-label={starred ? "Unstar" : "Star"}
              aria-pressed={starred}
              onClick={() => starWithToast(issue.short_ref)}
            >
              <Star className="i" aria-hidden />
            </IBtn>
            <IBtn title="Copy link" aria-label="Copy link" onClick={() => void copyText(link, "Link copied")}>
              <Link2 className="i" aria-hidden />
            </IBtn>
            <MenuButton items={more} className="relative" align="end">
              <IBtn title="More" aria-label="More">
                <MoreHorizontal className="i" aria-hidden />
              </IBtn>
            </MenuButton>
          </div>
          <IssueTitle issue={issue} as="h1" className="-mt-2" />
          <DescriptionSection issue={issue} />
          <IssueActivity
            issue={issue}
            history={history}
            filter={filter}
            onFilterChange={setFilter}
            highlight={highlight}
          />
        </div>
      </div>

      <aside className="rail">
        <div>
          <SectionHeading className="mb-1.5">
            Properties
            <Sp />
            <HeadingNote>hover: who touched it last</HeadingNote>
          </SectionHeading>
          <IssueProps issue={issue} history={history} compact={false} onShowChanges={showChanges} />
        </div>

        <div>
          <SectionHeading className="mb-2">
            History
            <Sp />
            <HeadingNote>from field_events · by seq</HeadingNote>
          </SectionHeading>
          <div className="hist">
            {newestFirst.length === 0 ? (
              <p className="empty">No changes since creation.</p>
            ) : (
              newestFirst.map((event) => (
                <button
                  key={event.seq}
                  type="button"
                  className="h click histRow text-left"
                  title="Show this change in the activity timeline"
                  onClick={() => showChanges(event.seq)}
                >
                  <span>
                    {event.field}:{" "}
                    <span className="mono">
                      {event.old_value === null ? "∅" : resolveIdValue(event.field, event.old_value, titleOf)}
                    </span>{" "}
                    →{" "}
                    <span className="mono">
                      {event.new_value === null ? "∅" : resolveIdValue(event.field, event.new_value, titleOf)}
                    </span>
                  </span>
                  <small>
                    {event.author} · {relativeTime(event.ts)}
                  </small>
                </button>
              ))
            )}
          </div>
        </div>

        <div>
          <SectionHeading className="mb-2">
            Commits
            <Sp />
            <HeadingNote>the data commits that touched this issue</HeadingNote>
          </SectionHeading>
          <div className="hist">
            {commits.length === 0 ? (
              <p className="empty">No commits recorded yet.</p>
            ) : (
              commits.map((commit) => (
                <button
                  key={commit.sha}
                  type="button"
                  className="h click commitRow text-left"
                  title="Copy the git show command"
                  onClick={() => void copyText(`git show ${commit.sha}`, "Command copied")}
                >
                  <span className="mono">{shortSha(commit.sha)}</span>
                  <small>
                    {commit.count} {commit.count === 1 ? "change" : "changes"} · {relativeTime(commit.ts)}
                  </small>
                </button>
              ))
            )}
          </div>
        </div>
      </aside>
    </div>
  );
}
