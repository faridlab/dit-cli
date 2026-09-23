// The Timeline sidebar section: how far back to look, which kinds of event
// to show, and whose. The options live in the shared view-options context
// so the header menus and the feed read the same state; the counts beside
// each person come from the loaded feed itself, so they are never stale
// relative to what the screen shows. Nothing here is a fact about the plan,
// so nothing here is written to the repo.

import type { LucideIcon } from "lucide-react";
import {
  GitCommitHorizontal,
  MessageSquare,
  Pencil,
  Plus,
  Tag,
  User,
  Zap,
} from "lucide-react";
import { Avatar } from "../badges";
import { Btn, CheckSquare, Row } from "../chrome";
import { PaneSection } from "../PaneSection";
import { workspaceTimeline, type TimelineBucket } from "../../lib/activity";
import { useActivity, useWorkspaceComments } from "../../lib/queries";
import { useViewOptions, type TimelineRange } from "../../lib/viewopts";
import { cn } from "../../lib/cn";

/** The same page the view loads, so the cache is shared and the people
 *  counts describe exactly the feed on screen. */
export const TIMELINE_FEED_LIMIT = 500;

const RANGES: ReadonlyArray<[TimelineRange, string]> = [
  ["7d", "7d"],
  ["30d", "30d"],
  ["90d", "90d"],
  ["all", "All"],
];

const KINDS: ReadonlyArray<[TimelineBucket, string, LucideIcon]> = [
  ["status", "Status changes", GitCommitHorizontal],
  ["priority", "Priority", Zap],
  ["assignees", "Assignment", User],
  ["labels", "Labels", Tag],
  ["comment", "Comments", MessageSquare],
  ["created", "Created", Plus],
  ["other", "Other fields (epic/estimate/dates/title)", Pencil],
];

export function TimelinePane(_props: { seq: number | null }) {
  const { timeline, setTimelineRange, toggleTimelineKind, setTimelineWho } = useViewOptions();
  const feed = useActivity({ limit: TIMELINE_FEED_LIMIT });
  const comments = useWorkspaceComments(TIMELINE_FEED_LIMIT);

  // Authors seen in the loaded feed, busiest first.
  const people = new Map<string, number>();
  for (const row of workspaceTimeline(feed.data?.events ?? [], comments.data ?? [])) {
    people.set(row.author, (people.get(row.author) ?? 0) + 1);
  }
  const authors = [...people.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const nothingPicked = timeline.kinds.size === 0;

  return (
    <>
      <PaneSection id="timeline.range" title="Range">
        <div style={{ display: "flex", gap: 4, padding: "2px 8px 6px" }}>
          {RANGES.map(([value, label]) => (
            <Btn
              key={value}
              primary={timeline.range === value}
              style={{ flex: 1, justifyContent: "center" }}
              onClick={() => setTimelineRange(value)}
            >
              {label}
            </Btn>
          ))}
        </div>
      </PaneSection>

      <PaneSection
        id="timeline.kinds"
        title="Kinds"
        count={nothingPicked ? "all" : `${timeline.kinds.size} of ${KINDS.length}`}
      >
        {KINDS.map(([kind, label, Icon]) => (
          <Row key={kind} onClick={() => toggleTimelineKind(kind)} title={label}>
            {/* With nothing picked every kind is shown, so every box reads as
                checked — faintly, to say "default" rather than "chosen". */}
            <CheckSquare
              on={nothingPicked || timeline.kinds.has(kind)}
              className={cn(nothingPicked && "opacity-45")}
            />
            <Icon className="i" aria-hidden />
            <span className="lbl">{label}</span>
          </Row>
        ))}
      </PaneSection>

      <PaneSection id="timeline.people" title="People" count={authors.length || null} fill>
        {authors.length === 0 ? (
          <p className="empty" style={{ padding: "2px 8px 0", fontSize: 11.5 }}>
            Nobody yet — people appear here once the feed has loaded.
          </p>
        ) : null}
        {authors.map(([author, count]) => (
          <Row
            key={author}
            on={timeline.who === author}
            onClick={() => setTimelineWho(author)}
            title={timeline.who === author ? "Show everyone" : `Only ${author}`}
          >
            <Avatar name={author} />
            <span className="lbl">{author}</span>
            <span className="cnt">{count}</span>
          </Row>
        ))}
      </PaneSection>

      <PaneSection id="timeline.source" title="Source" defaultCollapsed>
        <p
          className="empty"
          style={{ padding: "2px 8px 0", fontSize: 11.5, lineHeight: 1.5, color: "var(--muted)" }}
        >
          Everything here is read from git: <span className="mono">field_events</span> ordered by{" "}
          <span className="mono">seq</span> and comment files. Nothing on this screen is stored.
        </p>
      </PaneSection>
    </>
  );
}
